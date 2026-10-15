mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use sqlx::PgPool;

use common::TestApp;

struct Trip {
    orga: String,
    editor: String,
    viewer: String,
    id: String,
    hotel_sj: String,
    hotel_fortuna: String,
    museum: String,
    cafe: String,
    arenal: String,
    springs: String,
    closed: String,
}

async fn add_poi(app: &TestApp, session: &str, trip: &str, body: Value) -> String {
    let res = app
        .post(&format!("/api/v1/trips/{trip}/pois"), body, Some(session))
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    res.body["id"].as_str().unwrap().to_owned()
}

/// Voyage de deux jours (mardi 1er et mercredi 2 décembre 2026) : nuit à San José,
/// puis à La Fortuna. Lieux autour des deux villes, plus un lieu toujours fermé.
async fn setup(app: &TestApp) -> Trip {
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let editor = app.register("editor@example.com", "Éditrice").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let id = app.create_trip(&orga, "Pura Vida").await;
    app.join(&orga, &id, &editor, "editor").await;
    app.join(&orga, &id, &viewer, "viewer").await;
    let res = app
        .patch(
            &format!("/api/v1/trips/{id}"),
            json!({ "version": 1, "end_date": "2026-12-02" }),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);

    let poi = |name: &str, category: &str, lat: f64, lon: f64| json!({ "name": name, "category": category, "lat": lat, "lon": lon, "country_code": "CR" });
    let hotel_sj = add_poi(
        app,
        &editor,
        &id,
        poi("Hotel San José", "lodging", 9.935, -84.08),
    )
    .await;
    let hotel_fortuna = add_poi(
        app,
        &editor,
        &id,
        poi("Hotel Fortuna", "lodging", 10.47, -84.64),
    )
    .await;
    let mut museum_body = poi("Museo Nacional", "museum", 9.9327, -84.0716);
    museum_body["opening_hours"] = json!("Tu-Sa 08:30-16:30; Su 09:00-16:30");
    let museum = add_poi(app, &editor, &id, museum_body).await;
    let cafe = add_poi(app, &editor, &id, poi("Café", "food", 9.934, -84.079)).await;
    let arenal = add_poi(
        app,
        &editor,
        &id,
        poi("Volcán Arenal", "nature", 10.4621, -84.7034),
    )
    .await;
    let springs = add_poi(
        app,
        &editor,
        &id,
        poi("Termales", "activity", 10.49, -84.68),
    )
    .await;
    let mut closed_body = poi("Toujours fermé", "sight", 9.94, -84.09);
    closed_body["opening_hours"] = json!("Mo-Su off");
    let closed = add_poi(app, &editor, &id, closed_body).await;

    for (date, hotel) in [("2026-12-01", &hotel_sj), ("2026-12-02", &hotel_fortuna)] {
        let res = app
            .request(
                Method::PUT,
                &format!("/api/v1/trips/{id}/days/{date}"),
                Some(json!({ "modes": null, "base_poi_id": hotel })),
                Some(&editor),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    }
    Trip {
        orga,
        editor,
        viewer,
        id,
        hotel_sj,
        hotel_fortuna,
        museum,
        cafe,
        arenal,
        springs,
        closed,
    }
}

fn uri(t: &Trip) -> String {
    format!("/api/v1/trips/{}/itinerary", t.id)
}

fn stop_ids(day: &Value) -> Vec<String> {
    day["stops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["poi_id"].as_str().unwrap().to_owned())
        .collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn editors_generate_and_everyone_reads(db: PgPool) {
    let app = TestApp::new(db);
    let t = setup(&app).await;
    let outsider = app.register("out@example.com", "Dehors").await;

    let res = app.get(&uri(&t), Some(&t.viewer)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body, Value::Null);
    assert_eq!(
        app.post(&uri(&t), json!({}), Some(&t.viewer)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.get(&uri(&t), Some(&outsider)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.post(&uri(&t), json!({}), Some(&outsider)).await.status,
        StatusCode::NOT_FOUND
    );

    let res = app.post(&uri(&t), json!({}), Some(&t.editor)).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["version"], 1);
    assert_eq!(res.body["generated_by_name"], "Éditrice");
    assert_eq!(res.body["stale"], false);
    let read = app.get(&uri(&t), Some(&t.viewer)).await;
    assert_eq!(read.body, res.body);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn itinerary_groups_places_by_lodging_and_respects_hours(db: PgPool) {
    let app = TestApp::new(db);
    let t = setup(&app).await;
    let it = app.post(&uri(&t), json!({}), Some(&t.editor)).await.body;
    let days = it["days"].as_array().unwrap();
    assert_eq!(days.len(), 2);

    // Jour 1 : nuit à San José, visites en ville.
    let d1 = &days[0];
    assert_eq!(d1["date"], "2026-12-01");
    assert_eq!(d1["start_poi_id"], t.hotel_sj.as_str());
    assert_eq!(d1["end_poi_id"], t.hotel_sj.as_str());
    assert_eq!(
        sorted(stop_ids(d1)),
        sorted(vec![t.museum.clone(), t.cafe.clone()])
    );
    // Jour 2 : départ de San José, arrivée à La Fortuna, visites autour du volcan.
    let d2 = &days[1];
    assert_eq!(d2["start_poi_id"], t.hotel_sj.as_str());
    assert_eq!(d2["end_poi_id"], t.hotel_fortuna.as_str());
    assert_eq!(
        sorted(stop_ids(d2)),
        sorted(vec![t.arenal.clone(), t.springs.clone()])
    );

    for day in days {
        let stops = day["stops"].as_array().unwrap();
        // Toutes les étapes ont un trajet (on part toujours d'un hébergement).
        assert!(stops.iter().all(|s| s["travel"]["mode"] == "car"), "{day}");
        assert!(day["back"].is_object());
        // Horaires dans la journée et dans l'ordre.
        let mut previous = "09:00".to_owned();
        for s in stops {
            let (arrival, start, end) = (
                s["arrival"].as_str().unwrap(),
                s["start"].as_str().unwrap(),
                s["end"].as_str().unwrap(),
            );
            assert!(
                previous.as_str() <= arrival && arrival <= start && start <= end,
                "{s}"
            );
            previous = end.to_owned();
        }
        assert!(day["finish"].as_str().unwrap() <= "19:00");
        assert!(day["total_travel_s"].as_u64().unwrap() > 0);
    }
    // Le musée ouvre à 8 h 30 : visite entre 8 h 30 et 16 h 30.
    let museum = d1["stops"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["poi_id"] == t.museum.as_str())
        .unwrap();
    assert!(museum["end"].as_str().unwrap() <= "16:30");
    // Le long trajet San José → La Fortuna ouvre la deuxième journée.
    assert!(d2["stops"][0]["travel"]["duration_s"].as_u64().unwrap() > 2 * 3600);

    // Hébergements exclus des visites ; le lieu toujours fermé est signalé.
    assert_eq!(
        it["unscheduled"],
        json!([{ "poi_id": t.closed, "reason": "closed_all_days" }])
    );
    assert_eq!(it["has_estimates"], false);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn generation_is_deterministic(db: PgPool) {
    let app = TestApp::new(db);
    let t = setup(&app).await;
    let first = app.post(&uri(&t), json!({}), Some(&t.editor)).await.body;
    let second = app.post(&uri(&t), json!({}), Some(&t.orga)).await.body;
    assert_eq!(second["version"], 2);
    assert_eq!(first["days"], second["days"]);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn changes_since_generation_are_flagged(db: PgPool) {
    let app = TestApp::new(db);
    let t = setup(&app).await;
    app.post(&uri(&t), json!({}), Some(&t.editor)).await;

    // Un lieu du jour 2 change de durée : seul ce jour est à recalculer.
    let poi_uri = format!("/api/v1/trips/{}/pois/{}", t.id, t.springs);
    let res = app
        .patch(
            &poi_uri,
            json!({ "version": 1, "duration_min": 30 }),
            Some(&t.editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let it = app.get(&uri(&t), Some(&t.viewer)).await.body;
    assert_eq!(it["stale"], true);
    assert_eq!(it["days"][0]["stale"], false);
    assert_eq!(it["days"][1]["stale"], true);

    // Un nouveau lieu apparaît comme ajouté depuis la génération.
    let new_poi = add_poi(
        &app,
        &t.editor,
        &t.id,
        json!({ "name": "Catarata", "category": "nature", "lat": 10.44, "lon": -84.67 }),
    )
    .await;
    let it = app.get(&uri(&t), Some(&t.viewer)).await.body;
    assert!(it["unscheduled"]
        .as_array()
        .unwrap()
        .contains(&json!({ "poi_id": new_poi, "reason": "added_after" })));

    // Supprimer une étape du jour 1 le rend périmé, sans erreur.
    let res = app
        .delete(
            &format!("/api/v1/trips/{}/pois/{}", t.id, t.museum),
            Some(&t.editor),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let it = app.get(&uri(&t), Some(&t.viewer)).await.body;
    assert_eq!(it["days"][0]["stale"], true);
    assert_eq!(stop_ids(&it["days"][0]), vec![t.cafe.clone()]);

    // Régénérer intègre tout et efface les signalements.
    let it = app.post(&uri(&t), json!({}), Some(&t.editor)).await.body;
    assert_eq!(it["stale"], false);
    assert!(stop_ids(&it["days"][1]).contains(&new_poi));
    assert!(it["days"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["stale"] == false));
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn setting_changes_make_days_stale(db: PgPool) {
    let app = TestApp::new(db);
    let t = setup(&app).await;
    app.post(&uri(&t), json!({}), Some(&t.editor)).await;

    // Modes du jour 1 changés (en ville, en transports en commun).
    app.request(
        Method::PUT,
        &format!("/api/v1/trips/{}/days/2026-12-01", t.id),
        Some(json!({ "modes": ["transit"], "base_poi_id": t.hotel_sj })),
        Some(&t.editor),
    )
    .await;
    let it = app.get(&uri(&t), Some(&t.viewer)).await.body;
    assert_eq!(it["days"][0]["stale"], true);
    assert_eq!(it["days"][1]["stale"], false);

    // Voyage rallongé d'un jour : jour vide à générer.
    let res = app
        .patch(
            &format!("/api/v1/trips/{}", t.id),
            json!({ "version": 2, "end_date": "2026-12-03" }),
            Some(&t.orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let it = app.get(&uri(&t), Some(&t.viewer)).await.body;
    assert_eq!(it["days"].as_array().unwrap().len(), 3);
    assert_eq!(it["days"][2]["stale"], true);
    assert_eq!(it["days"][2]["stops"], json!([]));

    // Après régénération : transports en commun estimés et signalés.
    let it = app.post(&uri(&t), json!({}), Some(&t.editor)).await.body;
    assert_eq!(it["days"][0]["stops"][0]["travel"]["mode"], "transit");
    assert_eq!(
        it["days"][0]["stops"][0]["travel"]["source"],
        "transit_estimate"
    );
    assert_eq!(it["has_estimates"], true);
}
