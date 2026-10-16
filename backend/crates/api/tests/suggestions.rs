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
        osm(
            7,
            "La Fortuna",
            Category::Other,
            "place=town",
            10.4706,
            -84.6453,
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

/// Notoriété stockée : Arenal (40 langues) et Corcovado (30 langues).
async fn seed_notability(app: &TestApp) {
    sqlx::raw_sql(
        "UPDATE osm_places SET wikidata = 'Q1130598' WHERE osm_id = 5;
         INSERT INTO wikidata_notability (qid, languages)
         VALUES ('Q641588', 40), ('Q1130598', 30)",
    )
    .execute(&app.db)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn an_empty_trip_discovers_the_most_notable_places_of_the_country(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let trip = app.create_trip(&orga, "Vide").await;
    seed(&app).await;
    seed_notability(&app).await;
    let res = app
        .get(&format!("/api/v1/trips/{trip}/suggestions"), Some(&orga))
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["status"], "discovery");
    // Tout le pays, par notoriété (Corcovado, loin de tout, est proposé) ; ni restaurant, ni
    // hôtel, ni localité, ni musée fermé.
    assert_eq!(
        names(&res.body),
        vec!["Volcán Arenal", "Corcovado", "Catarata Río Fortuna"]
    );
    let first = &res.body["suggestions"][0];
    assert_eq!(first["date"], Value::Null);
    assert!(first["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["kind"] == "notable" && r["languages"] == 40));
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn regions_group_notable_places_around_a_place_to_stay(db: PgPool) {
    let app = TestApp::new(db);
    let (orga, viewer, trip, _area) = setup(&app).await;
    let uri = format!("/api/v1/trips/{trip}/regions");

    seed(&app).await;
    // Lieux importés, notoriété pas encore connue.
    let res = app.get(&uri, Some(&viewer)).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["status"], "not_imported");

    seed_notability(&app).await;
    let res = app.get(&uri, Some(&viewer)).await;
    assert_eq!(res.body["status"], "ready");
    let regions = res.body["regions"].as_array().unwrap();
    assert_eq!(regions.len(), 2, "{regions:?}");
    // Nommée d'après la localité où loger ; le parc isolé, d'après lui-même.
    assert_eq!(regions[0]["name"], "La Fortuna");
    assert_eq!(regions[0]["highlights"][0]["name"], "Volcán Arenal");
    assert_eq!(regions[0]["themes"], json!(["nature"]));
    assert_eq!(regions[0]["nights"], 1);
    assert_eq!(regions[1]["name"], "Corcovado");
    assert_eq!(regions[1]["in_trip"], 0);
    // Une seule base connue : rien à quoi la relier.
    assert_eq!(regions[0]["access"], "unknown");

    // Un lieu du voyage dans la région ; un point fort écarté disparaît.
    app.post(
        &format!("/api/v1/trips/{trip}/pois"),
        json!({ "name": "Cascade", "category": "nature", "lat": 10.44, "lon": -84.67 }),
        Some(&orga),
    )
    .await;
    app.post(
        &format!("/api/v1/trips/{trip}/suggestions/dismissals"),
        json!({ "osm": { "type": "node", "id": 5 }, "category": "nature" }),
        Some(&orga),
    )
    .await;
    let res = app.get(&uri, Some(&viewer)).await;
    let regions = res.body["regions"].as_array().unwrap();
    assert_eq!(regions.len(), 1, "{regions:?}");
    assert_eq!(regions[0]["in_trip"], 1);

    // Voyage d'un autre groupe : 404.
    let outsider = app.register("dehors@example.com", "Dehors").await;
    let res = app.get(&uri, Some(&outsider)).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn regions_without_road_access_are_reached_by_boat_or_plane(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let trip = app.create_trip(&orga, "Vide").await;
    seed(&app).await;
    seed_notability(&app).await;
    // Une localité près de Corcovado, et une île très connue (sans route : le faux moteur
    // ne relie rien à l'hémisphère sud).
    sqlx::raw_sql(
        "INSERT INTO osm_places (osm_type, osm_id, name, category, osm_tag, location, wikidata)
         VALUES ('node', 20, 'Puerto Jiménez', 'other', 'place=town',
                 ST_SetSRID(ST_MakePoint(-83.30, 8.535), 4326)::geography, NULL),
                ('node', 21, 'Isla Lejana', 'nature', 'natural=beach',
                 ST_SetSRID(ST_MakePoint(-90.31, -0.75), 4326)::geography, 'Q999'),
                ('node', 22, 'Puerto Isla', 'other', 'place=village',
                 ST_SetSRID(ST_MakePoint(-90.30, -0.74), 4326)::geography, NULL);
         INSERT INTO wikidata_notability (qid, languages) VALUES ('Q999', 59);",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let uri = format!("/api/v1/trips/{trip}/regions");
    let res = app.get(&uri, Some(&orga)).await;
    let access: Vec<(&str, &str)> = res.body["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap(), r["access"].as_str().unwrap()))
        .collect();
    assert_eq!(
        access,
        vec![
            ("Puerto Isla", "boat_or_plane"),
            ("La Fortuna", "road"),
            ("Puerto Jiménez", "road"),
        ]
    );

    // Moteur de routage en panne : on ne sait pas (rien n'est affirmé).
    app.routing
        .down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let res = app.get(&uri, Some(&orga)).await;
    assert!(res.body["regions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["access"] == "unknown"));
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn notability_is_refreshed_in_the_background(db: PgPool) {
    use std::sync::Arc;

    use api::wiki::{StaticWiki, WikiService};

    let app = TestApp::new(db);
    seed(&app).await;
    sqlx::query("UPDATE osm_places SET wikidata = 'Q1130598' WHERE osm_id = 5")
        .execute(&app.db)
        .await
        .unwrap();
    let mut wiki = StaticWiki::default();
    wiki.language_counts.insert("Q641588".into(), 40);
    let wiki = Arc::new(wiki);
    let service = WikiService::new(wiki.clone());

    // Corcovado est inconnu de Wikidata : 0 langue, mais stocké (pas redemandé).
    assert_eq!(
        api::notability::refresh(&app.db, &service).await.unwrap(),
        2
    );
    let stored: Vec<(String, i32)> =
        sqlx::query_as("SELECT qid, languages FROM wikidata_notability ORDER BY qid")
            .fetch_all(&app.db)
            .await
            .unwrap();
    assert_eq!(stored, vec![("Q1130598".into(), 0), ("Q641588".into(), 40)]);
    assert_eq!(
        api::notability::refresh(&app.db, &service).await.unwrap(),
        0
    );

    // Wikidata en panne : rien n'est écrit, rien n'échoue.
    sqlx::query("DELETE FROM wikidata_notability")
        .execute(&app.db)
        .await
        .unwrap();
    wiki.down.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        api::notability::refresh(&app.db, &service).await.unwrap(),
        0
    );
}
