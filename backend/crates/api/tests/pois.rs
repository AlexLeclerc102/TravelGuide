mod common;

use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use serde_json::{json, Value};
use sqlx::PgPool;

use common::TestApp;

/// Organisateur + éditrice + lecteur sur un même voyage (1er au 8 décembre 2026).
async fn setup(app: &TestApp) -> (String, String, String, String) {
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let editor = app.register("editor@example.com", "Éditrice").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let trip = app.create_trip(&orga, "Pura Vida").await;
    app.join(&orga, &trip, &editor, "editor").await;
    app.join(&orga, &trip, &viewer, "viewer").await;
    (orga, editor, viewer, trip)
}

fn museum() -> Value {
    json!({
        "name": "Museo Nacional",
        "category": "museum",
        "lat": 9.9327,
        "lon": -84.0716,
        "opening_hours": "Tu-Sa 08:30-16:30; Su 09:00-16:30",
        "country_code": "cr",
        "osm": { "type": "relation", "id": 4144800 }
    })
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn editor_adds_poi_and_everyone_sees_it(db: PgPool) {
    let app = TestApp::new(db);
    let (_orga, editor, viewer, trip) = setup(&app).await;
    let uri = format!("/api/v1/trips/{trip}/pois");

    let res = app.post(&uri, museum(), Some(&editor)).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    let poi = &res.body;
    assert_eq!(poi["name"], "Museo Nacional");
    assert_eq!(poi["category"], "museum");
    assert_eq!(poi["duration_min"], 90); // durée type d'un musée
    assert_eq!(poi["country_code"], "CR");
    assert_eq!(poi["osm"]["type"], "relation");
    assert_eq!(poi["created_by_name"], "Éditrice");
    assert!((poi["lat"].as_f64().unwrap() - 9.9327).abs() < 1e-6);
    assert!((poi["lon"].as_f64().unwrap() + 84.0716).abs() < 1e-6);
    assert_eq!(poi["opening_hours_status"], "valid");
    // Fermé les lundis du voyage (1er–8 décembre 2026 : lundi 7).
    assert_eq!(poi["closed_dates"], json!(["2026-12-07"]));

    let list = app.get(&uri, Some(&viewer)).await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body.as_array().unwrap().len(), 1);
    let one = app
        .get(
            &format!("{uri}/{}", poi["id"].as_str().unwrap()),
            Some(&viewer),
        )
        .await;
    assert_eq!(one.body["name"], "Museo Nacional");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn roles_are_enforced_on_pois(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, editor, viewer, trip) = setup(&app).await;
    let outsider = app.register("out@example.com", "Dehors").await;
    let uri = format!("/api/v1/trips/{trip}/pois");

    // Lecteur : lecture seule.
    assert_eq!(
        app.post(&uri, museum(), Some(&viewer)).await.status,
        StatusCode::FORBIDDEN
    );
    // Non-membre : le voyage « n'existe pas ».
    assert_eq!(
        app.get(&uri, Some(&outsider)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.post(&uri, museum(), Some(&outsider)).await.status,
        StatusCode::NOT_FOUND
    );
    // Anonyme.
    assert_eq!(app.get(&uri, None).await.status, StatusCode::UNAUTHORIZED);

    let id = app.post(&uri, museum(), Some(&orga)).await.body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let poi_uri = format!("{uri}/{id}");
    let patch = json!({ "version": 1, "name": "Renommé" });
    assert_eq!(
        app.patch(&poi_uri, patch.clone(), Some(&viewer))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.delete(&poi_uri, Some(&viewer)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.delete(&poi_uri, Some(&outsider)).await.status,
        StatusCode::NOT_FOUND
    );

    // Un éditeur modifie et supprime les POI ajoutés par les autres.
    assert_eq!(
        app.patch(&poi_uri, patch, Some(&editor)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.delete(&poi_uri, Some(&editor)).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&poi_uri, Some(&editor)).await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn poi_of_another_trip_is_not_reachable(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let other = app.register_organizer("other@example.com", "Autre").await;
    let trip_a = app.create_trip(&orga, "A").await;
    let trip_b = app.create_trip(&other, "B").await;
    let poi_b = app
        .post(
            &format!("/api/v1/trips/{trip_b}/pois"),
            museum(),
            Some(&other),
        )
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Membre du voyage A, mais POI du voyage B : introuvable, et intact.
    let cross = format!("/api/v1/trips/{trip_a}/pois/{poi_b}");
    assert_eq!(
        app.get(&cross, Some(&orga)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.patch(&cross, json!({ "version": 1, "name": "x" }), Some(&orga))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete(&cross, Some(&orga)).await.status,
        StatusCode::NOT_FOUND
    );
    let still = app
        .get(
            &format!("/api/v1/trips/{trip_b}/pois/{poi_b}"),
            Some(&other),
        )
        .await;
    assert_eq!(still.body["name"], "Museo Nacional");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn invalid_input_is_rejected(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, _, _, trip) = setup(&app).await;
    let uri = format!("/api/v1/trips/{trip}/pois");
    let base = json!({ "name": "Lieu", "lat": 9.9, "lon": -84.0 });

    let mut cases = vec![];
    for (field, value) in [
        ("name", json!("  ")),
        ("lat", json!(91.0)),
        ("lon", json!(-181.0)),
        ("duration_min", json!(721)),
        ("duration_min", json!(-5)),
        ("website", json!("javascript:alert(1)")),
        ("country_code", json!("CRI")),
        ("phone", json!("tel:2257")),
        ("email", json!("museo")),
        ("wikidata", json!("190622")),
        ("wikipedia", json!("Volcán Arenal")),
        ("notes", json!("x".repeat(2001))),
        ("opening_hours", json!("x".repeat(501))),
        ("osm", json!({ "type": "node", "id": -1 })),
    ] {
        let mut body = base.clone();
        body[field] = value;
        cases.push((field, body));
    }
    for (field, body) in cases {
        let res = app.post(&uri, body, Some(&orga)).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "{field}: {:?}",
            res.body
        );
    }
    // Catégorie inconnue : rejet à la désérialisation.
    let mut body = base.clone();
    body["category"] = json!("casino");
    assert!(app
        .post(&uri, body, Some(&orga))
        .await
        .status
        .is_client_error());

    // Valeurs par défaut : catégorie « other », durée 60 min.
    let res = app.post(&uri, base, Some(&orga)).await;
    assert_eq!(res.status, StatusCode::CREATED);
    assert_eq!(res.body["category"], "other");
    assert_eq!(res.body["duration_min"], 60);
    assert_eq!(res.body["opening_hours_status"], "none");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn invalid_opening_hours_are_kept_and_flagged(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, _, _, trip) = setup(&app).await;
    let mut body = museum();
    body["opening_hours"] = json!("ouvert le matin");
    let res = app
        .post(&format!("/api/v1/trips/{trip}/pois"), body, Some(&orga))
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
    assert_eq!(res.body["opening_hours"], "ouvert le matin");
    assert_eq!(res.body["opening_hours_status"], "invalid");
    assert_eq!(res.body["closed_dates"], json!([]));
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn same_osm_place_is_added_once(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, editor, _, trip) = setup(&app).await;
    let uri = format!("/api/v1/trips/{trip}/pois");
    assert_eq!(
        app.post(&uri, museum(), Some(&orga)).await.status,
        StatusCode::CREATED
    );
    let dup = app.post(&uri, museum(), Some(&editor)).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    assert_eq!(dup.body["message"], "ce lieu est déjà dans le voyage");

    // Le même lieu peut figurer dans un autre voyage.
    let other = app.create_trip(&orga, "Autre").await;
    assert_eq!(
        app.post(
            &format!("/api/v1/trips/{other}/pois"),
            museum(),
            Some(&orga)
        )
        .await
        .status,
        StatusCode::CREATED
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn update_uses_optimistic_locking_and_clears_fields(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, editor, _, trip) = setup(&app).await;
    let mut body = museum();
    body["website"] = json!("https://example.cr");
    body["phone"] = json!("+506 2257 1433");
    body["email"] = json!("Info@Museo.cr");
    body["wikidata"] = json!("Q2638513");
    body["wikipedia"] = json!("es:Museo Nacional de Costa Rica");
    let created = app
        .post(&format!("/api/v1/trips/{trip}/pois"), body, Some(&orga))
        .await
        .body;
    assert_eq!(created["phone"], "+506 2257 1433");
    assert_eq!(created["email"], "info@museo.cr");
    assert_eq!(created["wikidata"], "Q2638513");
    assert_eq!(created["wikipedia"], "es:Museo Nacional de Costa Rica");
    let uri = format!(
        "/api/v1/trips/{trip}/pois/{}",
        created["id"].as_str().unwrap()
    );

    let res = app
        .patch(
            &uri,
            json!({ "version": 1, "duration_min": 120, "lat": 10.0, "lon": -84.2,
                    "opening_hours": "", "website": "", "phone": "", "notes": "Gratuit le dimanche" }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["version"], 2);
    assert_eq!(res.body["duration_min"], 120);
    assert_eq!(res.body["lat"], 10.0);
    assert_eq!(res.body["opening_hours"], Value::Null);
    assert_eq!(res.body["opening_hours_status"], "none");
    assert_eq!(res.body["website"], Value::Null);
    assert_eq!(res.body["phone"], Value::Null);
    assert_eq!(res.body["wikipedia"], "es:Museo Nacional de Costa Rica"); // inchangé
    assert_eq!(res.body["notes"], "Gratuit le dimanche");
    assert_eq!(res.body["name"], "Museo Nacional"); // inchangé

    // Version obsolète : refus.
    let stale = app
        .patch(&uri, json!({ "version": 1, "name": "Écrasé" }), Some(&orga))
        .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(
        app.get(&uri, Some(&orga)).await.body["name"],
        "Museo Nacional"
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn opening_hours_preview_covers_trip_dates(db: PgPool) {
    let app = TestApp::new(db);
    let (_, _, viewer, trip) = setup(&app).await;
    let uri = format!("/api/v1/trips/{trip}/opening-hours/preview");

    let res = app
        .post(
            &uri,
            json!({ "opening_hours": "Mo-Fr 09:00-12:00,14:00-18:00; PH off", "country_code": "CR" }),
            Some(&viewer),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["status"], "valid");
    let days = res.body["days"].as_array().unwrap();
    assert_eq!(days.len(), 8); // 1er au 8 décembre inclus
    assert_eq!(days[0]["date"], "2026-12-01");
    // 1er décembre : férié au Costa Rica (abolition de l'armée), donc `PH off`.
    assert_eq!(days[0]["holiday"], true);
    assert_eq!(days[0]["intervals"], json!([]));
    assert_eq!(days[1]["holiday"], false);
    assert_eq!(
        days[1]["intervals"],
        json!([
            { "start": "09:00", "end": "12:00", "unknown": false },
            { "start": "14:00", "end": "18:00", "unknown": false }
        ])
    );
    assert_eq!(days[4]["intervals"], json!([])); // samedi 5

    let bad = app
        .post(&uri, json!({ "opening_hours": "le matin" }), Some(&viewer))
        .await;
    assert_eq!(bad.body["status"], "invalid");
    assert!(bad.body["error"].is_string());

    let outsider = app.register("out@example.com", "Dehors").await;
    assert_eq!(
        app.post(&uri, json!({ "opening_hours": "24/7" }), Some(&outsider))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn poi_limit_per_trip(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, _, _, trip) = setup(&app).await;
    let trip_uuid: uuid::Uuid = trip.parse().unwrap();
    sqlx::query(
        "INSERT INTO pois (trip_id, name, category, location, duration_min)
         SELECT $1, 'Lieu ' || g, 'other', ST_SetSRID(ST_MakePoint(-84, 9.9), 4326)::geography, 60
         FROM generate_series(1, 300) g",
    )
    .bind(trip_uuid)
    .execute(&app.db)
    .await
    .unwrap();
    let res = app
        .post(&format!("/api/v1/trips/{trip}/pois"), museum(), Some(&orga))
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn pois_survive_author_deletion_and_appear_in_export(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, editor, _, trip) = setup(&app).await;
    let uri = format!("/api/v1/trips/{trip}/pois");
    app.post(&uri, museum(), Some(&editor)).await;

    let export = app.get("/api/v1/me/export", Some(&editor)).await;
    assert_eq!(export.body["pois_created"][0]["name"], "Museo Nacional");

    let res = app
        .request(
            axum::http::Method::DELETE,
            "/api/v1/me",
            Some(json!({ "confirm_email": "editor@example.com", "password": common::PASSWORD })),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT, "{:?}", res.body);

    let list = app.get(&uri, Some(&orga)).await.body;
    assert_eq!(list[0]["name"], "Museo Nacional");
    assert_eq!(list[0]["created_by"], Value::Null);
    assert_eq!(list[0]["created_by_name"], Value::Null);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn place_search_and_reverse(db: PgPool) {
    let app = TestApp::new(db);
    let user = app.register("ana@example.com", "Ana").await;

    let res = app
        .get(
            "/api/v1/places/search?q=arenal&viewbox=-85.1,8.0,-82.5,11.2",
            Some(&user),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body[0]["name"], "Volcán Arenal");
    assert_eq!(res.body[0]["category"], "nature");
    assert_eq!(res.body[0]["osm"]["id"], 276216895);

    // Deuxième recherche identique : servie par le cache.
    app.get(
        "/api/v1/places/search?q=Arenal&viewbox=-85.1,8.0,-82.5,11.2",
        Some(&user),
    )
    .await;
    assert_eq!(app.places.calls.load(Ordering::SeqCst), 1);

    let rev = app
        .get(
            "/api/v1/places/reverse?lat=9.93271&lon=-84.07161",
            Some(&user),
        )
        .await;
    assert_eq!(rev.body["place"]["name"], "Museo Nacional de Costa Rica");
    let none = app
        .get("/api/v1/places/reverse?lat=0&lon=0", Some(&user))
        .await;
    assert_eq!(none.status, StatusCode::OK);
    assert_eq!(none.body["place"], Value::Null);

    // Validation et authentification.
    for bad in [
        "/api/v1/places/search?q=a",
        "/api/v1/places/search?q=museo&viewbox=1,2,3",
        "/api/v1/places/reverse?lat=95&lon=0",
    ] {
        assert_eq!(
            app.get(bad, Some(&user)).await.status,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    assert_eq!(
        app.get("/api/v1/places/search?q=museo", None).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn place_search_is_rate_limited_per_user(db: PgPool) {
    let app = TestApp::new(db);
    let ana = app.register("ana@example.com", "Ana").await;
    let bob = app.register("bob@example.com", "Bob").await;
    for i in 0..30 {
        let res = app
            .get(&format!("/api/v1/places/search?q=lieu{i}"), Some(&ana))
            .await;
        assert_eq!(res.status, StatusCode::OK);
    }
    let res = app.get("/api/v1/places/search?q=encore", Some(&ana)).await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    // Les autres utilisateurs ne sont pas affectés.
    assert_eq!(
        app.get("/api/v1/places/search?q=museo", Some(&bob))
            .await
            .status,
        StatusCode::OK
    );
}
