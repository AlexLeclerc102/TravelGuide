mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;

use api::osm_import::{store, OsmPlace};
use api::places::{Category, OsmDetails, OsmType};
use common::TestApp;

fn osm(id: i64, name: &str, category: Category, tag: &str, lat: f64, lon: f64) -> OsmPlace {
    OsmPlace {
        osm_type: OsmType::Node,
        osm_id: id,
        name: name.into(),
        category,
        osm_tag: tag.into(),
        lat,
        lon,
        details: OsmDetails::default(),
    }
}

/// Autour de La Fortuna, plus un lieu loin de tout et un hôtel.
async fn seed(app: &TestApp) {
    let mut arenal = osm(
        1,
        "Volcán Arenal",
        Category::Nature,
        "natural=volcano",
        10.4621,
        -84.7034,
    );
    arenal.details.wikidata = Some("Q641588".into());
    let mut closed = osm(
        4,
        "Museo fermé",
        Category::Museum,
        "tourism=museum",
        10.47,
        -84.65,
    );
    closed.details.opening_hours = Some("Mo-Su off".into());
    let places = vec![
        arenal,
        osm(
            2,
            "Catarata Río Fortuna",
            Category::Nature,
            "waterway=waterfall",
            10.4392,
            -84.6693,
        ),
        osm(
            3,
            "Soda La Parada",
            Category::Food,
            "amenity=restaurant",
            10.4710,
            -84.6450,
        ),
        closed,
        osm(
            5,
            "Corcovado",
            Category::Nature,
            "boundary=national_park",
            8.54,
            -83.57,
        ),
        osm(
            6,
            "Hotel Arenal Springs",
            Category::Lodging,
            "tourism=hotel",
            10.4700,
            -84.6500,
        ),
    ];
    store(&app.db, "test", None, &places).await.unwrap();
}

/// Voyage du 1er au 3 décembre 2026, deux nuits à La Fortuna (zone à préciser).
async fn setup(app: &TestApp) -> (String, String, String, String) {
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let trip = app.create_trip(&orga, "Pura Vida").await;
    app.join(&orga, &trip, &viewer, "viewer").await;
    app.patch(
        &format!("/api/v1/trips/{trip}"),
        json!({ "version": 1, "end_date": "2026-12-03" }),
        Some(&orga),
    )
    .await;
    let area = app
        .post(
            &format!("/api/v1/trips/{trip}/pois"),
            json!({ "name": "La Fortuna", "category": "lodging", "placeholder": true,
                    "lat": 10.4706, "lon": -84.6453, "country_code": "CR" }),
            Some(&orga),
        )
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let res = app
        .request(
            Method::PUT,
            &format!("/api/v1/trips/{trip}/stays"),
            Some(
                json!({ "poi_id": area, "first_night": "2026-12-01", "last_night": "2026-12-02" }),
            ),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    (orga, viewer, trip, area)
}

fn names(body: &Value) -> Vec<String> {
    body["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["place"]["name"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn suggests_nearby_places_with_reasons(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, viewer, trip, _area) = setup(&app).await;
    let uri = format!("/api/v1/trips/{trip}/suggestions");

    // Avant l'import des lieux OSM : rien, mais un statut clair.
    let res = app.get(&uri, Some(&viewer)).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["status"], "not_imported");

    seed(&app).await;
    let res = app.get(&uri, Some(&viewer)).await;
    assert_eq!(res.body["status"], "ready");
    let got = names(&res.body);
    // Ni l'hôtel, ni le restaurant (filtre à part), ni le musée fermé tout le voyage, ni le
    // parc à plus de 2 h 30.
    assert_eq!(got, vec!["Volcán Arenal", "Catarata Río Fortuna"]);
    let first = &res.body["suggestions"][0];
    assert!(first["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "near" && r["date"] == "2026-12-01"));
    assert_eq!(first["place"]["country_code"], "CR");

    // Le groupe adore la nature : les lieux nature passent devant le restaurant.
    for name in ["Cascade A", "Cascade B"] {
        let poi = app
            .post(
                &format!("/api/v1/trips/{trip}/pois"),
                json!({ "name": name, "category": "nature", "lat": 10.44, "lon": -84.67 }),
                Some(&orga),
            )
            .await
            .body["id"]
            .as_str()
            .unwrap()
            .to_owned();
        app.request(
            Method::PUT,
            &format!("/api/v1/trips/{trip}/pois/{poi}/reviews/me"),
            Some(json!({ "rating": 5, "comment": "" })),
            Some(&orga),
        )
        .await;
    }
    let res = app.get(&uri, Some(&viewer)).await;
    assert!(res.body["suggestions"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "taste" && r["category"] == "nature" && r["average"] == 5.0));

    // Filtre par catégorie.
    let food = app
        .get(&format!("{uri}?category=food"), Some(&viewer))
        .await;
    assert_eq!(names(&food.body), vec!["Soda La Parada"]);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn dismissed_and_added_places_are_no_longer_suggested(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, viewer, trip, _area) = setup(&app).await;
    seed(&app).await;
    let uri = format!("/api/v1/trips/{trip}/suggestions");

    // Ajouté au voyage : n'est plus suggéré.
    let res = app
        .post(
            &format!("/api/v1/trips/{trip}/pois"),
            json!({ "name": "Volcán Arenal", "category": "nature", "lat": 10.4621, "lon": -84.7034,
                    "osm": { "type": "node", "id": 1 } }),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
    // Écarté par le groupe (pas par un lecteur).
    let dismiss = json!({ "osm": { "type": "node", "id": 3 }, "category": "food" });
    let res = app
        .post(&format!("{uri}/dismissals"), dismiss.clone(), Some(&viewer))
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    let res = app
        .post(&format!("{uri}/dismissals"), dismiss, Some(&orga))
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let food = format!("{uri}?category=food");
    assert!(names(&app.get(&food, Some(&viewer)).await.body).is_empty());
    assert_eq!(
        names(&app.get(&uri, Some(&viewer)).await.body),
        vec!["Catarata Río Fortuna"]
    );

    // Tout reproposer.
    let res = app
        .request(
            Method::DELETE,
            &format!("{uri}/dismissals"),
            None,
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        names(&app.get(&food, Some(&viewer)).await.body),
        vec!["Soda La Parada"]
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn lodging_around_a_stay_and_free_time(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, viewer, trip, area) = setup(&app).await;
    seed(&app).await;
    let uri = format!("/api/v1/trips/{trip}/suggestions");

    let res = app
        .get(&format!("{uri}?lodging_near={area}"), Some(&viewer))
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(names(&res.body), vec!["Hotel Arenal Springs"]);
    assert_eq!(res.body["suggestions"][0]["date"], Value::Null);

    // Itinéraire généré (journées vides) : le temps libre devient une raison.
    let res = app
        .post(
            &format!("/api/v1/trips/{trip}/itinerary"),
            json!({}),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let res = app.get(&uri, Some(&viewer)).await;
    assert!(res.body["suggestions"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "free_time" && r["minutes"].as_u64().unwrap() >= 600));

    // Lieu d'un autre voyage : 404.
    let other = app.create_trip(&orga, "Autre").await;
    let res = app
        .get(
            &format!("/api/v1/trips/{other}/suggestions?lodging_near={area}"),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn nothing_to_search_around(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let trip = app.create_trip(&orga, "Vide").await;
    seed(&app).await;
    let res = app
        .get(&format!("/api/v1/trips/{trip}/suggestions"), Some(&orga))
        .await;
    assert_eq!(res.body["status"], "no_anchor");
    assert_eq!(res.body["suggestions"], json!([]));
}
