mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use api::assistant::AssistantCall;
use common::{TestApp, ORIGIN, PASSWORD};

struct Ctx {
    orga: String,
    editor: String,
    viewer: String,
    trip: String,
    poi: String,
}

impl Ctx {
    fn uri(&self, what: &str) -> String {
        format!("/api/v1/trips/{}/pois/{}/{what}", self.trip, self.poi)
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
            json!({ "name": "Volcán Arenal", "category": "nature", "lat": 10.46, "lon": -84.70,
                    "notes": "  Prévoir des chaussures de marche  " }),
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
async fn notes_are_shared_and_editable_by_editors(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;

    // La note donnée à l'ajout du lieu devient sa première note.
    let notes = app.get(&c.uri("notes"), Some(&c.viewer)).await;
    assert_eq!(notes.status, StatusCode::OK);
    assert_eq!(notes.body.as_array().unwrap().len(), 1);
    assert_eq!(notes.body[0]["body"], "Prévoir des chaussures de marche");
    assert_eq!(notes.body[0]["author_name"], "Orga");
    assert_eq!(notes.body[0]["via_assistant"], false);

    let created = app
        .post(
            &c.uri("notes"),
            json!({ "body": "Ouvert de 8 h à 16 h", "source_url": "https://arenal.net/horaires" }),
            Some(&c.editor),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    assert_eq!(created.body["author_name"], "Éditrice");
    assert_eq!(created.body["source_url"], "https://arenal.net/horaires");
    let note_uri = format!(
        "{}/{}",
        c.uri("notes"),
        created.body["id"].as_str().unwrap()
    );

    let updated = put(
        &app,
        &note_uri,
        json!({ "body": "Ouvert de 8 h à 17 h", "source_url": "" }),
        &c.orga,
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{:?}", updated.body);
    assert_eq!(updated.body["body"], "Ouvert de 8 h à 17 h");
    assert_eq!(updated.body["source_url"], Value::Null);
    // L'auteur reste celui qui a écrit la note.
    assert_eq!(updated.body["author_name"], "Éditrice");

    let notes = app.get(&c.uri("notes"), Some(&c.viewer)).await;
    assert_eq!(notes.body[1]["body"], "Ouvert de 8 h à 17 h");

    assert_eq!(
        app.delete(&note_uri, Some(&c.editor)).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete(&note_uri, Some(&c.editor)).await.status,
        StatusCode::NOT_FOUND
    );
    let notes = app.get(&c.uri("notes"), Some(&c.viewer)).await;
    assert_eq!(notes.body.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn viewers_and_outsiders_cannot_write(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    let outsider = app.register("autre@example.com", "Autre").await;

    let note = json!({ "body": "Hors sujet" });
    assert_eq!(
        app.post(&c.uri("notes"), note.clone(), Some(&c.viewer))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.post(&c.uri("notes"), note, Some(&outsider))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get(&c.uri("notes"), Some(&outsider)).await.status,
        StatusCode::NOT_FOUND
    );
    let link = json!({ "url": "https://arenal.net" });
    assert_eq!(
        app.post(&c.uri("links"), link, Some(&c.viewer))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let existing = app.get(&c.uri("notes"), Some(&c.viewer)).await.body[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let note_uri = format!("{}/{existing}", c.uri("notes"));
    assert_eq!(
        app.delete(&note_uri, Some(&c.viewer)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put(&app, &note_uri, json!({ "body": "x" }), &c.viewer)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn content_of_another_trip_is_not_found(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    let other_trip = app.create_trip(&c.orga, "Autre voyage").await;
    let note_id = app.get(&c.uri("notes"), Some(&c.orga)).await.body[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Le lieu existe, mais pas dans ce voyage.
    let wrong_trip = format!("/api/v1/trips/{other_trip}/pois/{}/notes", c.poi);
    assert_eq!(
        app.get(&wrong_trip, Some(&c.orga)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete(&format!("{wrong_trip}/{note_id}"), Some(&c.orga))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // La note existe, mais pas sous ce lieu.
    let other_poi = app
        .post(
            &format!("/api/v1/trips/{}/pois", c.trip),
            json!({ "name": "Tabacón", "category": "nature", "lat": 10.49, "lon": -84.72 }),
            Some(&c.orga),
        )
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let wrong_poi = format!("/api/v1/trips/{}/pois/{other_poi}/notes/{note_id}", c.trip);
    assert_eq!(
        app.delete(&wrong_poi, Some(&c.orga)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        put(&app, &wrong_poi, json!({ "body": "x" }), &c.orga)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn invalid_notes_and_links_are_rejected(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;

    for body in [
        json!({ "body": "   " }),
        json!({ "body": "x".repeat(4001) }),
        json!({ "body": "ok", "source_url": "javascript:alert(1)" }),
    ] {
        let res = app
            .post(&c.uri("notes"), body.clone(), Some(&c.editor))
            .await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST, "{body}");
    }
    for body in [
        json!({ "url": "http://arenal.net" }),
        json!({ "url": "https://" }),
        json!({ "url": "https://arenal.net/a b" }),
        json!({ "url": format!("https://{}", "a".repeat(500)) }),
        json!({ "url": "https://arenal.net", "title": "deux\nlignes" }),
        json!({ "url": "https://arenal.net", "kind": "sponsored" }),
    ] {
        let res = app
            .post(&c.uri("links"), body.clone(), Some(&c.editor))
            .await;
        assert!(res.status.is_client_error(), "{body} : {:?}", res.status);
    }
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn links_have_a_default_title_and_are_unique(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;

    let created = app
        .post(
            &c.uri("links"),
            json!({ "url": "https://www.arenal.net/fr/" }),
            Some(&c.editor),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    assert_eq!(created.body["title"], "arenal.net");
    assert_eq!(created.body["kind"], "other");
    assert_eq!(created.body["author_name"], "Éditrice");

    let booking = app
        .post(
            &c.uri("links"),
            json!({ "url": "https://tickets.cr/arenal", "title": "Billets", "kind": "booking" }),
            Some(&c.orga),
        )
        .await;
    assert_eq!(booking.status, StatusCode::CREATED);
    assert_eq!(booking.body["kind"], "booking");

    let duplicate = app
        .post(
            &c.uri("links"),
            json!({ "url": "https://www.arenal.net/fr/", "title": "Encore" }),
            Some(&c.orga),
        )
        .await;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);

    let links = app.get(&c.uri("links"), Some(&c.viewer)).await;
    assert_eq!(links.body.as_array().unwrap().len(), 2);
    assert_eq!(links.body[1]["title"], "Billets");

    let link_uri = format!(
        "{}/{}",
        c.uri("links"),
        created.body["id"].as_str().unwrap()
    );
    assert_eq!(
        app.delete(&link_uri, Some(&c.editor)).await.status,
        StatusCode::NO_CONTENT
    );
    let links = app.get(&c.uri("links"), Some(&c.viewer)).await;
    assert_eq!(links.body.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn notes_per_place_are_limited(db: PgPool) {
    let app = TestApp::new(db.clone());
    let c = setup(&app).await;
    let poi: Uuid = c.poi.parse().unwrap();
    // 1 note à l'ajout du lieu + 99 : la limite de 100 est atteinte.
    sqlx::query("INSERT INTO poi_notes (poi_id, body) SELECT $1, 'n' FROM generate_series(1, 99)")
        .bind(poi)
        .execute(&db)
        .await
        .unwrap();

    let res = app
        .post(
            &c.uri("notes"),
            json!({ "body": "une de trop" }),
            Some(&c.editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.body["message"].as_str().unwrap().contains("100"));
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn assistant_calls_are_attributed_but_headers_cannot_fake_it(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;

    let post = |assistant: bool| {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri(c.uri("notes"))
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("tg_session={}", c.editor))
            .header(header::CONTENT_TYPE, "application/json")
            // Un client ne peut pas se faire passer pour l'assistant.
            .header("x-assistant", "true")
            .body(Body::from(json!({ "body": "Note" }).to_string()))
            .unwrap();
        if assistant {
            req.extensions_mut().insert(AssistantCall {
                conversation_id: Uuid::new_v4(),
            });
        }
        req
    };
    let by_user = app.send(post(false)).await;
    assert_eq!(by_user.status, StatusCode::CREATED);
    assert_eq!(by_user.body["via_assistant"], false);

    let by_assistant = app.send(post(true)).await;
    assert_eq!(by_assistant.status, StatusCode::CREATED);
    assert_eq!(by_assistant.body["via_assistant"], true);
    assert_eq!(by_assistant.body["author_name"], "Éditrice");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn content_is_exported_and_kept_without_author_after_account_deletion(db: PgPool) {
    let app = TestApp::new(db);
    let c = setup(&app).await;
    app.post(
        &c.uri("notes"),
        json!({ "body": "Parking payant" }),
        Some(&c.editor),
    )
    .await;
    app.post(
        &c.uri("links"),
        json!({ "url": "https://arenal.net", "kind": "official" }),
        Some(&c.editor),
    )
    .await;

    let export = app.get("/api/v1/me/export", Some(&c.editor)).await;
    assert_eq!(export.body["poi_notes"][0]["body"], "Parking payant");
    assert_eq!(export.body["poi_notes"][0]["poi_name"], "Volcán Arenal");
    assert_eq!(export.body["poi_links"][0]["url"], "https://arenal.net");
    // La note de l'organisatrice n'est pas dans l'export de l'éditrice.
    assert_eq!(export.body["poi_notes"].as_array().unwrap().len(), 1);

    let deleted = app
        .request(
            Method::DELETE,
            "/api/v1/me",
            Some(json!({ "confirm_email": "editor@example.com", "password": PASSWORD })),
            Some(&c.editor),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let notes = app.get(&c.uri("notes"), Some(&c.orga)).await;
    assert_eq!(notes.body[1]["body"], "Parking payant");
    assert_eq!(notes.body[1]["author_id"], Value::Null);
    assert_eq!(notes.body[1]["author_name"], Value::Null);
    let links = app.get(&c.uri("links"), Some(&c.orga)).await;
    assert_eq!(links.body[0]["author_id"], Value::Null);
}
