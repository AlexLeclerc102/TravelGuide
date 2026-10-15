mod common;

use std::sync::atomic::Ordering;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;

use common::TestApp;

/// Organisateur + éditrice + lecteur sur un même voyage (1er au 8 décembre 2026, en voiture).
async fn setup(app: &TestApp) -> (String, String, String, String) {
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let editor = app.register("editor@example.com", "Éditrice").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let trip = app.create_trip(&orga, "Pura Vida").await;
    app.join(&orga, &trip, &editor, "editor").await;
    app.join(&orga, &trip, &viewer, "viewer").await;
    (orga, editor, viewer, trip)
}

async fn add_poi(
    app: &TestApp,
    session: &str,
    trip: &str,
    name: &str,
    lat: f64,
    lon: f64,
) -> String {
    let res = app
        .post(
            &format!("/api/v1/trips/{trip}/pois"),
            json!({ "name": name, "category": "sight", "lat": lat, "lon": lon }),
            Some(session),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    res.body["id"].as_str().unwrap().to_owned()
}

async fn put_day(
    app: &TestApp,
    session: &str,
    trip: &str,
    date: &str,
    body: Value,
) -> common::Response {
    app.request(
        Method::PUT,
        &format!("/api/v1/trips/{trip}/days/{date}"),
        Some(body),
        Some(session),
    )
    .await
}

fn day<'a>(days: &'a Value, date: &str) -> &'a Value {
    days.as_array()
        .unwrap()
        .iter()
        .find(|d| d["date"] == date)
        .unwrap_or_else(|| panic!("jour {date} absent"))
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn days_inherit_trip_modes_until_overridden(db: PgPool) {
    let app = TestApp::new(db);
    let (_orga, editor, viewer, trip) = setup(&app).await;
    let hotel = add_poi(&app, &editor, &trip, "Hotel", 10.47, -84.64).await;
    let uri = format!("/api/v1/trips/{trip}/days");

    let res = app.get(&uri, Some(&viewer)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body.as_array().unwrap().len(), 8);
    let first = &res.body[0];
    assert_eq!(first["date"], "2026-12-01");
    assert_eq!(first["modes"], json!(["car"]));
    assert_eq!(first["modes_inherited"], true);
    assert_eq!(first["base_poi_id"], Value::Null);

    let res = put_day(
        &app,
        &editor,
        &trip,
        "2026-12-03",
        json!({ "modes": ["walk", "transit", "walk"], "base_poi_id": hotel }),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["modes"], json!(["walk", "transit"]));
    assert_eq!(res.body["modes_inherited"], false);
    assert_eq!(res.body["base_poi_id"], hotel.as_str());

    let days = app.get(&uri, Some(&viewer)).await.body;
    assert_eq!(
        day(&days, "2026-12-03")["modes"],
        json!(["walk", "transit"])
    );
    assert_eq!(day(&days, "2026-12-04")["modes_inherited"], true);

    // Hébergement seul : les modes redeviennent ceux du voyage.
    let res = put_day(
        &app,
        &editor,
        &trip,
        "2026-12-03",
        json!({ "modes": null, "base_poi_id": hotel }),
    )
    .await;
    assert_eq!(res.body["modes"], json!(["car"]));
    assert_eq!(res.body["modes_inherited"], true);
    assert_eq!(res.body["base_poi_id"], hotel.as_str());

    // Retour complet aux valeurs par défaut : plus rien n'est stocké.
    let res = put_day(
        &app,
        &editor,
        &trip,
        "2026-12-03",
        json!({ "modes": null, "base_poi_id": null }),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM trip_days")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn day_settings_are_validated_and_restricted(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, editor, viewer, trip) = setup(&app).await;
    let outsider = app.register_organizer("out@example.com", "Dehors").await;
    let other_trip = app.create_trip(&outsider, "Autre").await;
    let foreign_poi = add_poi(&app, &outsider, &other_trip, "Ailleurs", 9.9, -84.0).await;
    let ok = json!({ "modes": ["bike"], "base_poi_id": null });

    assert_eq!(
        put_day(&app, &viewer, &trip, "2026-12-02", ok.clone())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put_day(&app, &outsider, &trip, "2026-12-02", ok.clone())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get(&format!("/api/v1/trips/{trip}/days"), Some(&outsider))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // Hors des dates du voyage.
    assert_eq!(
        put_day(&app, &editor, &trip, "2026-12-09", ok.clone())
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    // Aucun mode, mode inconnu.
    assert_eq!(
        put_day(
            &app,
            &editor,
            &trip,
            "2026-12-02",
            json!({ "modes": [], "base_poi_id": null })
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert!(put_day(
        &app,
        &editor,
        &trip,
        "2026-12-02",
        json!({ "modes": ["plane"] })
    )
    .await
    .status
    .is_client_error());
    // Hébergement d'un autre voyage.
    assert_eq!(
        put_day(
            &app,
            &orga,
            &trip,
            "2026-12-02",
            json!({ "modes": null, "base_poi_id": foreign_poi })
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        put_day(&app, &orga, &trip, "2026-12-02", ok).await.status,
        StatusCode::OK
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn day_settings_follow_pois_and_trip_dates(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, editor, _viewer, trip) = setup(&app).await;
    let hotel = add_poi(&app, &editor, &trip, "Hotel", 10.47, -84.64).await;
    let uri = format!("/api/v1/trips/{trip}/days");
    put_day(
        &app,
        &editor,
        &trip,
        "2026-12-02",
        json!({ "modes": null, "base_poi_id": hotel }),
    )
    .await;
    put_day(
        &app,
        &editor,
        &trip,
        "2026-12-05",
        json!({ "modes": ["walk"] }),
    )
    .await;

    // Supprimer l'hébergement le retire du jour.
    let res = app
        .delete(&format!("/api/v1/trips/{trip}/pois/{hotel}"), Some(&editor))
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let days = app.get(&uri, Some(&editor)).await.body;
    assert_eq!(day(&days, "2026-12-02")["base_poi_id"], Value::Null);

    // Raccourcir puis rallonger le voyage : le réglage du 5 ne revient pas.
    let res = app
        .patch(
            &format!("/api/v1/trips/{trip}"),
            json!({ "version": 1, "end_date": "2026-12-03" }),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(
        app.get(&uri, Some(&editor))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        3
    );
    app.patch(
        &format!("/api/v1/trips/{trip}"),
        json!({ "version": 2, "end_date": "2026-12-08" }),
        Some(&orga),
    )
    .await;
    let days = app.get(&uri, Some(&editor)).await.body;
    assert_eq!(day(&days, "2026-12-05")["modes_inherited"], true);
}

/// Musée, café à ~800 m, puis La Fortuna (~90 km).
async fn three_pois(app: &TestApp, session: &str, trip: &str) -> [String; 3] {
    [
        add_poi(app, session, trip, "Museo Nacional", 9.9327, -84.0716).await,
        add_poi(app, session, trip, "Café", 9.9340, -84.0790).await,
        add_poi(app, session, trip, "La Fortuna", 10.4710, -84.6450).await,
    ]
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn route_compares_modes_and_picks_one_per_leg(db: PgPool) {
    let app = TestApp::new(db);
    let (_orga, editor, viewer, trip) = setup(&app).await;
    let [museum, cafe, fortuna] = three_pois(&app, &editor, &trip).await;
    let uri = format!("/api/v1/trips/{trip}/routes");

    // Les lecteurs peuvent consulter les trajets.
    let res = app
        .post(
            &uri,
            json!({ "poi_ids": [museum, cafe, fortuna], "modes": ["transit", "walk", "car"] }),
            Some(&viewer),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let plan = &res.body;
    assert_eq!(plan["modes"], json!(["transit", "walk", "car"]));
    let legs = plan["legs"].as_array().unwrap();
    assert_eq!(legs.len(), 2);

    // Court trajet : à pied ; long trajet : en voiture (le bus estimé est plus lent).
    assert_eq!(legs[0]["from_poi_id"], museum.as_str());
    assert_eq!(legs[0]["to_poi_id"], cafe.as_str());
    assert_eq!(legs[0]["chosen"]["mode"], "walk");
    assert_eq!(legs[1]["chosen"]["mode"], "car");
    assert_eq!(legs[1]["chosen"]["source"], "router");
    let options: Vec<&str> = legs[1]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["mode"].as_str().unwrap())
        .collect();
    assert_eq!(options, ["walk", "car", "transit"]);
    assert_eq!(legs[1]["options"][2]["source"], "transit_estimate");
    assert!(
        legs[1]["options"][2]["duration_s"].as_u64() > legs[1]["options"][1]["duration_s"].as_u64()
    );

    // Tracé GeoJSON [lon, lat] du départ à l'arrivée.
    let geometry = legs[1]["geometry"].as_array().unwrap();
    assert!((geometry[0][0].as_f64().unwrap() + 84.0790).abs() < 1e-6);
    assert!((geometry.last().unwrap()[1].as_f64().unwrap() - 10.4710).abs() < 1e-6);

    let total = legs[0]["chosen"]["duration_s"].as_u64().unwrap()
        + legs[1]["chosen"]["duration_s"].as_u64().unwrap();
    assert_eq!(plan["total_duration_s"].as_u64().unwrap(), total);
    assert_eq!(plan["has_estimates"], false);
    // Une requête marche + une voiture : les transports en commun en sont dérivés.
    assert_eq!(app.routing.calls.load(Ordering::SeqCst), 2);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn route_modes_default_to_day_then_trip(db: PgPool) {
    let app = TestApp::new(db);
    let (_orga, editor, _viewer, trip) = setup(&app).await;
    let [museum, cafe, _] = three_pois(&app, &editor, &trip).await;
    let uri = format!("/api/v1/trips/{trip}/routes");
    put_day(
        &app,
        &editor,
        &trip,
        "2026-12-04",
        json!({ "modes": ["bike", "walk"] }),
    )
    .await;

    let res = app
        .post(&uri, json!({ "poi_ids": [museum, cafe] }), Some(&editor))
        .await;
    assert_eq!(res.body["modes"], json!(["car"]));
    let res = app
        .post(
            &uri,
            json!({ "poi_ids": [museum, cafe], "date": "2026-12-04" }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.body["modes"], json!(["bike", "walk"]));
    assert_eq!(res.body["legs"][0]["chosen"]["mode"], "walk");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn route_requests_are_validated(db: PgPool) {
    let app = TestApp::new(db);
    let (_orga, editor, _viewer, trip) = setup(&app).await;
    let outsider = app.register_organizer("out@example.com", "Dehors").await;
    let other_trip = app.create_trip(&outsider, "Autre").await;
    let foreign = add_poi(&app, &outsider, &other_trip, "Ailleurs", 9.9, -84.0).await;
    let [museum, cafe, _] = three_pois(&app, &editor, &trip).await;
    let uri = format!("/api/v1/trips/{trip}/routes");

    for body in [
        json!({ "poi_ids": [museum] }),
        json!({ "poi_ids": [museum, museum] }),
        json!({ "poi_ids": [museum, foreign] }),
        json!({ "poi_ids": [museum, cafe], "modes": [] }),
        json!({ "poi_ids": [museum, cafe], "date": "2027-01-01" }),
        json!({ "poi_ids": ([museum.as_str(), cafe.as_str()].repeat(13)) }),
    ] {
        let res = app.post(&uri, body.clone(), Some(&editor)).await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST, "{body}");
    }
    // Aller-retour : un même lieu peut revenir s'il n'est pas consécutif.
    let res = app
        .post(
            &uri,
            json!({ "poi_ids": [museum, cafe, museum] }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let res = app
        .post(&uri, json!({ "poi_ids": [museum, cafe] }), Some(&outsider))
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn routing_outage_falls_back_to_flagged_estimates(db: PgPool) {
    let app = TestApp::new(db);
    let (_orga, editor, _viewer, trip) = setup(&app).await;
    let [museum, _, fortuna] = three_pois(&app, &editor, &trip).await;
    app.routing.down.store(true, Ordering::SeqCst);

    let res = app
        .post(
            &format!("/api/v1/trips/{trip}/routes"),
            json!({ "poi_ids": [museum, fortuna], "modes": ["car"] }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let leg = &res.body["legs"][0];
    assert_eq!(leg["chosen"]["source"], "straight_line");
    assert!(leg["chosen"]["duration_s"].as_u64().unwrap() > 3600);
    assert_eq!(leg["geometry"].as_array().unwrap().len(), 2);
    assert_eq!(res.body["has_estimates"], true);
}
