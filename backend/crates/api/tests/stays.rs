mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;

use common::TestApp;

/// Voyage du 1er au 5 décembre 2026, organisateur + éditrice + lecteur.
async fn setup(app: &TestApp) -> (String, String, String) {
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let editor = app.register("editor@example.com", "Éditrice").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let trip = app.create_trip(&orga, "Pura Vida").await;
    app.join(&orga, &trip, &editor, "editor").await;
    app.join(&orga, &trip, &viewer, "viewer").await;
    let res = app
        .patch(
            &format!("/api/v1/trips/{trip}"),
            json!({ "version": 1, "end_date": "2026-12-05" }),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    (editor, viewer, trip)
}

async fn add(app: &TestApp, session: &str, trip: &str, body: Value) -> String {
    let res = app
        .post(&format!("/api/v1/trips/{trip}/pois"), body, Some(session))
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    res.body["id"].as_str().unwrap().to_owned()
}

fn bases(days: &Value) -> Vec<Option<String>> {
    days.as_array()
        .unwrap()
        .iter()
        .map(|d| d["base_poi_id"].as_str().map(str::to_owned))
        .collect()
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn areas_are_planned_by_nights_then_replaced_by_a_hotel(db: PgPool) {
    let app = TestApp::new(db);
    let (editor, viewer, trip) = setup(&app).await;
    let area = add(
        &app,
        &editor,
        &trip,
        json!({ "name": "La Fortuna", "category": "lodging", "placeholder": true,
                "lat": 10.47, "lon": -84.64 }),
    )
    .await;
    let stays = format!("/api/v1/trips/{trip}/stays");

    // Nuits du 2 au 4 dans la zone ; les modes de transport du jour restent intacts.
    app.request(
        Method::PUT,
        &format!("/api/v1/trips/{trip}/days/2026-12-03"),
        Some(json!({ "modes": ["walk"], "base_poi_id": null })),
        Some(&editor),
    )
    .await;
    let res = app
        .request(
            Method::PUT,
            &stays,
            Some(
                json!({ "poi_id": area, "first_night": "2026-12-02", "last_night": "2026-12-04" }),
            ),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let a = Some(area.clone());
    assert_eq!(bases(&res.body), vec![None, a.clone(), a.clone(), a, None]);
    assert_eq!(res.body[2]["modes"], json!(["walk"]));

    // Hôtel choisi : il prend toutes les nuits de la zone, qui disparaît.
    let hotel = add(
        &app,
        &editor,
        &trip,
        json!({ "name": "Hotel Arenal", "category": "lodging", "lat": 10.46, "lon": -84.65 }),
    )
    .await;
    let res = app
        .post(
            &format!("{stays}/replace"),
            json!({ "from_poi_id": area, "to_poi_id": hotel, "remove_from": true }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let h = Some(hotel.clone());
    assert_eq!(bases(&res.body), vec![None, h.clone(), h.clone(), h, None]);
    let pois = app
        .get(&format!("/api/v1/trips/{trip}/pois"), Some(&viewer))
        .await
        .body;
    let names: Vec<&str> = pois
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Hotel Arenal"]);

    // Effacer une nuit : la ligne du jour sans réglage disparaît.
    let res = app
        .request(
            Method::PUT,
            &stays,
            Some(
                json!({ "poi_id": null, "first_night": "2026-12-02", "last_night": "2026-12-02" }),
            ),
            Some(&editor),
        )
        .await;
    assert_eq!(bases(&res.body)[1], None);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM trip_days WHERE date = '2026-12-02'")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn stays_are_validated(db: PgPool) {
    let app = TestApp::new(db);
    let (editor, viewer, trip) = setup(&app).await;
    let hotel = add(
        &app,
        &editor,
        &trip,
        json!({ "name": "Hotel", "category": "lodging", "lat": 10.0, "lon": -84.0 }),
    )
    .await;
    let museum = add(
        &app,
        &editor,
        &trip,
        json!({ "name": "Musée", "category": "museum", "lat": 10.0, "lon": -84.0 }),
    )
    .await;
    let stays = format!("/api/v1/trips/{trip}/stays");
    let put = |body: Value, session: String| {
        let app = &app;
        let stays = stays.clone();
        async move {
            app.request(Method::PUT, &stays, Some(body), Some(&session))
                .await
                .status
        }
    };

    // Seule une zone d'hébergement peut être « à préciser ».
    let res = app
        .post(
            &format!("/api/v1/trips/{trip}/pois"),
            json!({ "name": "Zone", "category": "museum", "placeholder": true, "lat": 10.0, "lon": -84.0 }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    // Dates hors voyage ou inversées.
    for (first, last) in [("2026-11-30", "2026-12-01"), ("2026-12-03", "2026-12-02")] {
        let body = json!({ "poi_id": hotel, "first_night": first, "last_night": last });
        assert_eq!(put(body, editor.clone()).await, StatusCode::BAD_REQUEST);
    }
    // Lecteur : refus.
    let body = json!({ "poi_id": hotel, "first_night": "2026-12-01", "last_night": "2026-12-01" });
    assert_eq!(put(body, viewer.clone()).await, StatusCode::FORBIDDEN);
    // Un lieu qui n'est pas une zone n'est jamais supprimé par un remplacement.
    let res = app
        .post(
            &format!("{stays}/replace"),
            json!({ "from_poi_id": museum, "to_poi_id": hotel, "remove_from": true }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let count = app
        .get(&format!("/api/v1/trips/{trip}/pois"), Some(&viewer))
        .await
        .body
        .as_array()
        .unwrap()
        .len();
    assert_eq!(count, 2);
    let res = app
        .post(
            &format!("{stays}/replace"),
            json!({ "from_poi_id": hotel, "to_poi_id": hotel }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);

    // Une zone qui devient un musée perd son statut de zone.
    let area = add(
        &app,
        &editor,
        &trip,
        json!({ "name": "Zone", "category": "lodging", "placeholder": true, "lat": 10.0, "lon": -84.0 }),
    )
    .await;
    let res = app
        .patch(
            &format!("/api/v1/trips/{trip}/pois/{area}"),
            json!({ "version": 1, "category": "sight" }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["placeholder"], false);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn stays_are_proposed_from_the_places_then_applied(db: PgPool) {
    use api::osm_import::{store, OsmPlace};
    use api::places::{Category, OsmDetails, OsmType};

    let app = TestApp::new(db);
    let (editor, viewer, trip) = setup(&app).await; // 5 nuits
    let town = |id: i64, name: &str, tag: &str, lat: f64, lon: f64| OsmPlace {
        osm_type: OsmType::Node,
        osm_id: id,
        name: name.into(),
        category: Category::Other,
        osm_tag: tag.into(),
        lat,
        lon,
        details: OsmDetails::default(),
    };
    store(
        &app.db,
        "test",
        None,
        &[
            town(1, "San José", "place=city", 9.933, -84.08),
            town(2, "La Fortuna", "place=town", 10.47, -84.645),
            town(3, "El Tanque", "place=village", 10.49, -84.66),
            // Des hôtels à La Fortuna, aucun à El Tanque, pourtant plus proche des visites.
            OsmPlace {
                category: Category::Lodging,
                ..town(4, "Hotel A", "tourism=hotel", 10.471, -84.646)
            },
            OsmPlace {
                category: Category::Lodging,
                ..town(5, "Hotel B", "tourism=hotel", 10.469, -84.644)
            },
        ],
    )
    .await
    .unwrap();
    // Une demi-journée à San José, beaucoup à voir autour de La Fortuna.
    for (i, (lat, lon)) in [(9.93, -84.07), (9.935, -84.075)].into_iter().enumerate() {
        add(
            &app,
            &editor,
            &trip,
            json!({ "name": format!("Musée {i}"), "category": "museum", "duration_min": 90,
                    "lat": lat, "lon": lon }),
        )
        .await;
    }
    for i in 0..5 {
        add(
            &app,
            &editor,
            &trip,
            json!({ "name": format!("Nature {i}"), "category": "nature", "duration_min": 300,
                    "lat": 10.45 + 0.01 * f64::from(i), "lon": -84.68 }),
        )
        .await;
    }
    let airport = add(
        &app,
        &editor,
        &trip,
        json!({ "name": "Aéroport", "category": "other", "lat": 9.998, "lon": -84.204 }),
    )
    .await;

    let uri = format!("/api/v1/trips/{trip}/stays/proposal");
    let res = app
        .post(
            &uri,
            json!({ "min_nights": 2, "start_poi_id": airport, "end_poi_id": airport }),
            Some(&viewer),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let stays = res.body["stays"].as_array().unwrap().clone();
    let names: Vec<&str> = stays.iter().map(|s| s["name"].as_str().unwrap()).collect();
    // On loge là où il y a des hébergements ; on commence près de l'aéroport.
    assert_eq!(names, vec!["San José", "La Fortuna"], "{stays:?}");
    assert_eq!(stays[0]["first_night"], "2026-12-01");
    assert_eq!(stays[1]["last_night"], "2026-12-05");
    assert!(stays[1]["nights"].as_u64() > stays[0]["nights"].as_u64());
    assert_eq!(stays[1]["poi_ids"].as_array().unwrap().len(), 5);
    assert!(stays[1]["transfer_min"].as_u64().unwrap() > 60);

    // Un lecteur peut demander une proposition, pas l'appliquer.
    let apply = format!("{uri}/apply");
    let body = json!({ "stays": stays });
    assert_eq!(
        app.post(&apply, body.clone(), Some(&viewer)).await.status,
        StatusCode::FORBIDDEN
    );
    let res = app.post(&apply, body, Some(&editor)).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let days = res.body.as_array().unwrap();
    assert!(days.iter().all(|d| d["base_poi_id"].is_string()));
    let pois = app
        .get(&format!("/api/v1/trips/{trip}/pois"), Some(&viewer))
        .await
        .body;
    let mut areas: Vec<&str> = pois
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["placeholder"] == true)
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    // Créées dans la même transaction (même date de création) : ordre indifférent.
    areas.sort();
    assert_eq!(areas, vec!["La Fortuna", "San José"]);

    // Nuits hors du voyage : refus.
    let res = app
        .post(
            &apply,
            json!({ "stays": [{ "name": "X", "lat": 10.0, "lon": -84.0,
                                "first_night": "2026-12-04", "last_night": "2026-12-09", "nights": 6 }] }),
            Some(&editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
}
