mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;

use common::TestApp;

struct Ctx {
    orga: String,
    editor: String,
    viewer: String,
    trip: String,
    poi: String,
}

impl Ctx {
    fn review_uri(&self) -> String {
        format!("/api/v1/trips/{}/pois/{}/reviews", self.trip, self.poi)
    }
    fn me_uri(&self) -> String {
        format!("{}/me", self.review_uri())
    }
}

async fn setup(app: &TestApp) -> Ctx {
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let editor = app.register("editor@example.com", "Éditrice").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let trip = app.create_trip(&orga, "Pura Vida").await;
    app.join(&orga, &trip, &editor, "editor").await;
    app.join(&orga, &trip, &viewer, "viewer").await;
    let poi = app
        .post(
            &format!("/api/v1/trips/{trip}/pois"),
            json!({ "name": "Volcán Arenal", "category": "nature", "lat": 10.46, "lon": -84.70 }),
            Some(&orga),
        )
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    Ctx {
        orga,
        editor,
        viewer,
        trip,
        poi,
    }
}

async fn put(app: &TestApp, uri: &str, body: Value, session: &str) -> common::Response {
    app.request(Method::PUT, uri, Some(body), Some(session))
        .await
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn each_member_rates_and_group_summary_updates(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;

    let res = put(
        &app,
        &c.me_uri(),
        json!({ "rating": 5, "comment": "Incroyable" }),
        &c.orga,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["rating"]["count"], 1);
    assert_eq!(res.body["rating"]["average"], 5.0);
    assert_eq!(res.body["rating"]["disagreement"], false);
    assert_eq!(
        res.body["my_review"],
        json!({ "rating": 5, "comment": "Incroyable" })
    );

    // Les lecteurs donnent aussi leur avis.
    let res = put(
        &app,
        &c.me_uri(),
        json!({ "rating": 2, "comment": "Trop de monde" }),
        &c.viewer,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["rating"]["count"], 2);
    assert_eq!(res.body["rating"]["average"], 3.5);
    assert_eq!(res.body["rating"]["min"], 2);
    assert_eq!(res.body["rating"]["max"], 5);
    assert_eq!(res.body["rating"]["disagreement"], true);
    assert_eq!(res.body["my_review"]["rating"], 2);

    // La liste des POI porte la synthèse et l'avis de chacun selon qui la consulte.
    let list = app
        .get(&format!("/api/v1/trips/{}/pois", c.trip), Some(&c.editor))
        .await
        .body;
    assert_eq!(list[0]["rating"]["count"], 2);
    assert_eq!(list[0]["my_review"], Value::Null);

    // Chacun voit l'avis de tous.
    let reviews = app.get(&c.review_uri(), Some(&c.editor)).await.body;
    let mut by_name: Vec<(String, i64, String)> = reviews
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["display_name"].as_str().unwrap().to_owned(),
                r["rating"].as_i64().unwrap(),
                r["comment"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    by_name.sort();
    assert_eq!(
        by_name,
        vec![
            ("Lecteur".into(), 2, "Trop de monde".into()),
            ("Orga".into(), 5, "Incroyable".into())
        ]
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn review_is_updated_in_place_and_can_be_removed(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    put(&app, &c.me_uri(), json!({ "rating": 3 }), &c.editor).await;
    let res = put(
        &app,
        &c.me_uri(),
        json!({ "rating": 4, "comment": "Mieux au lever du soleil" }),
        &c.editor,
    )
    .await;
    assert_eq!(res.body["rating"]["count"], 1);
    assert_eq!(res.body["my_review"]["rating"], 4);

    let res = app
        .request(Method::DELETE, &c.me_uri(), None, Some(&c.editor))
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["rating"]["count"], 0);
    assert_eq!(res.body["rating"]["average"], Value::Null);
    assert_eq!(res.body["my_review"], Value::Null);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn invalid_reviews_are_rejected(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    for rating in [0, 6] {
        let res = put(&app, &c.me_uri(), json!({ "rating": rating }), &c.orga).await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST, "{rating}");
    }
    for body in [json!({ "rating": -1 }), json!({ "rating": 4.5 }), json!({})] {
        let res = put(&app, &c.me_uri(), body.clone(), &c.orga).await;
        assert!(res.status.is_client_error(), "{body}");
    }
    let res = put(
        &app,
        &c.me_uri(),
        json!({ "rating": 4, "comment": "x".repeat(2001) }),
        &c.orga,
    )
    .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn reviews_are_private_to_trip_members(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    put(&app, &c.me_uri(), json!({ "rating": 5 }), &c.orga).await;
    let outsider = app.register_organizer("out@example.com", "Dehors").await;

    assert_eq!(
        app.get(&c.review_uri(), Some(&outsider)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        put(&app, &c.me_uri(), json!({ "rating": 1 }), &outsider)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get(&c.review_uri(), None).await.status,
        StatusCode::UNAUTHORIZED
    );

    // POI d'un autre voyage via un voyage dont on est membre : introuvable.
    let other_trip = app.create_trip(&outsider, "Autre").await;
    let cross = format!("/api/v1/trips/{other_trip}/pois/{}/reviews/me", c.poi);
    assert_eq!(
        put(&app, &cross, json!({ "rating": 1 }), &outsider)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let list = app.get(&c.review_uri(), Some(&c.orga)).await.body;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn leaving_the_trip_removes_my_reviews(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    put(&app, &c.me_uri(), json!({ "rating": 1 }), &c.viewer).await;
    put(&app, &c.me_uri(), json!({ "rating": 5 }), &c.orga).await;

    let viewer_id = app.user_id(&c.viewer).await;
    let res = app
        .delete(
            &format!("/api/v1/trips/{}/members/{viewer_id}", c.trip),
            Some(&c.viewer),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);

    let poi = app
        .get(
            &format!("/api/v1/trips/{}/pois/{}", c.trip, c.poi),
            Some(&c.orga),
        )
        .await
        .body;
    assert_eq!(poi["rating"]["count"], 1);
    assert_eq!(poi["rating"]["disagreement"], false);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn reviews_are_exported_and_deleted_with_the_account(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    put(
        &app,
        &c.me_uri(),
        json!({ "rating": 4, "comment": "Beau" }),
        &c.editor,
    )
    .await;

    let export = app.get("/api/v1/me/export", Some(&c.editor)).await.body;
    assert_eq!(export["reviews"][0]["poi_name"], "Volcán Arenal");
    assert_eq!(export["reviews"][0]["rating"], 4);
    assert_eq!(export["reviews"][0]["comment"], "Beau");

    let res = app
        .request(
            Method::DELETE,
            "/api/v1/me",
            Some(json!({ "confirm_email": "editor@example.com", "password": common::PASSWORD })),
            Some(&c.editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let reviews = app.get(&c.review_uri(), Some(&c.orga)).await.body;
    assert_eq!(reviews, json!([]));
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn deleting_a_poi_deletes_its_reviews(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    put(&app, &c.me_uri(), json!({ "rating": 4 }), &c.editor).await;
    app.delete(
        &format!("/api/v1/trips/{}/pois/{}", c.trip, c.poi),
        Some(&c.orga),
    )
    .await;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM reviews")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
