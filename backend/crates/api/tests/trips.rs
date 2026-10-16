mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

use common::TestApp;

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn free_plan_cannot_create_trip(db: PgPool) {
    let app = TestApp::new(db);
    let token = app.register("ana@example.com", "Ana").await;
    let res = app
        .post(
            "/api/v1/trips",
            json!({ "name": "Costa Rica", "start_date": "2026-12-01", "end_date": "2026-12-08" }),
            Some(&token),
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body["code"], "plan_required");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn organizer_emails_config_grants_plan(db: PgPool) {
    let mut config = api::config::Config::for_tests();
    config.organizer_emails = vec!["orga@example.com".into()];
    let app = TestApp::with_config(db, config);
    app.register("orga@example.com", "Orga").await;
    let token = app.register("autre@example.com", "Autre").await;
    let me = app.get("/api/v1/me", Some(&token)).await;
    assert_eq!(me.body["plan"], "free");
    let login = app
        .post(
            "/api/v1/auth/login",
            json!({ "email": "orga@example.com", "password": common::PASSWORD }),
            None,
        )
        .await;
    assert_eq!(login.body["plan"], "organizer");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn create_with_defaults_and_list(db: PgPool) {
    let app = TestApp::new(db);
    let token = app.register_organizer("orga@example.com", "Orga").await;
    let res = app
        .post(
            "/api/v1/trips",
            json!({ "name": "  Pura Vida  ", "start_date": "2026-12-01", "end_date": "2026-12-08" }),
            Some(&token),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
    let trip = &res.body;
    assert_eq!(trip["name"], "Pura Vida");
    assert_eq!(trip["timezone"], "America/Costa_Rica");
    assert_eq!(trip["day_start"], "09:00");
    assert_eq!(trip["day_end"], "19:00");
    assert_eq!(trip["default_modes"], json!(["car"]));
    assert_eq!(trip["my_role"], "organizer");
    assert_eq!(trip["member_count"], 1);
    assert_eq!(trip["start_date"], "2026-12-01");
    assert_eq!(trip["version"], 1);

    let list = app.get("/api/v1/trips", Some(&token)).await;
    assert_eq!(list.body.as_array().unwrap().len(), 1);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn create_rejects_invalid_input(db: PgPool) {
    let app = TestApp::new(db);
    let token = app.register_organizer("orga@example.com", "Orga").await;
    let base = json!({ "name": "Voyage", "start_date": "2026-12-01", "end_date": "2026-12-08" });
    let cases = [
        json!({ "name": "" }),
        json!({ "end_date": "2026-11-30" }),
        json!({ "end_date": "2027-03-01" }),
        json!({ "timezone": "Mars/Olympus_Mons" }),
        json!({ "day_start": "19:00", "day_end": "09:00" }),
        json!({ "default_modes": [] }),
    ];
    for patch in cases {
        let mut body = base.clone();
        for (k, v) in patch.as_object().unwrap() {
            body[k] = v.clone();
        }
        let res = app.post("/api/v1/trips", body.clone(), Some(&token)).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "{body} -> {:?}",
            res.body
        );
    }
    // Format invalide : rejeté à la désérialisation.
    let mut body = base.clone();
    body["day_start"] = json!("9h");
    assert!(app
        .post("/api/v1/trips", body, Some(&token))
        .await
        .status
        .is_client_error());
    let mut body = base;
    body["default_modes"] = json!(["teleportation"]);
    assert!(app
        .post("/api/v1/trips", body, Some(&token))
        .await
        .status
        .is_client_error());
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn update_uses_optimistic_locking(db: PgPool) {
    let app = TestApp::new(db);
    let token = app.register_organizer("orga@example.com", "Orga").await;
    let id = app.create_trip(&token, "Voyage").await;
    let uri = format!("/api/v1/trips/{id}");

    let ok = app
        .patch(
            &uri,
            json!({ "version": 1, "name": "Nouveau nom", "default_modes": ["walk", "transit"], "day_start": "08:00" }),
            Some(&token),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(ok.body["name"], "Nouveau nom");
    assert_eq!(ok.body["default_modes"], json!(["walk", "transit"]));
    assert_eq!(ok.body["day_start"], "08:00");
    assert_eq!(ok.body["version"], 2);

    let stale = app
        .patch(
            &uri,
            json!({ "version": 1, "name": "Conflit" }),
            Some(&token),
        )
        .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(
        app.get(&uri, Some(&token)).await.body["name"],
        "Nouveau nom"
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn role_based_access_on_trip_routes(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let editor = app.register("editor@example.com", "Éditrice").await;
    let viewer = app.register("viewer@example.com", "Lecteur").await;
    let stranger = app.register("stranger@example.com", "Inconnu").await;
    let id = app.create_trip(&orga, "Voyage").await;
    app.join(&orga, &id, &editor, "editor").await;
    app.join(&orga, &id, &viewer, "viewer").await;
    let uri = format!("/api/v1/trips/{id}");

    // Lecture : tous les membres, pas les autres (404 pour ne pas révéler l'existence).
    for s in [&orga, &editor, &viewer] {
        assert_eq!(app.get(&uri, Some(s)).await.status, StatusCode::OK);
    }
    assert_eq!(
        app.get(&uri, Some(&stranger)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(app.get(&uri, None).await.status, StatusCode::UNAUTHORIZED);

    // Paramètres et suppression : organisateur seulement.
    for s in [&editor, &viewer] {
        let res = app
            .patch(&uri, json!({ "version": 1, "name": "x" }), Some(s))
            .await;
        assert_eq!(res.status, StatusCode::FORBIDDEN);
        assert_eq!(
            app.delete(&uri, Some(s)).await.status,
            StatusCode::FORBIDDEN
        );
        let inv = app
            .post(
                &format!("{uri}/invitations"),
                json!({ "role": "viewer" }),
                Some(s),
            )
            .await;
        assert_eq!(inv.status, StatusCode::FORBIDDEN);
    }
    assert_eq!(
        app.delete(&uri, Some(&stranger)).await.status,
        StatusCode::NOT_FOUND
    );

    // L'éditeur voit son rôle.
    assert_eq!(app.get(&uri, Some(&editor)).await.body["my_role"], "editor");
    assert_eq!(app.get(&uri, Some(&orga)).await.body["member_count"], 3);

    assert_eq!(
        app.delete(&uri, Some(&orga)).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.get(&uri, Some(&orga)).await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn preferences_are_set_at_creation_then_edited(db: PgPool) {
    let app = TestApp::new(db);
    let orga = app.register_organizer("orga@example.com", "Orga").await;
    let res = app
        .post(
            "/api/v1/trips",
            json!({ "name": "Pura Vida", "start_date": "2026-12-01", "end_date": "2026-12-08",
                    "interests": ["food", "beach", "nature", "beach"], "pace": "relaxed",
                    "budget": "budget" }),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    // Sans doublon, dans un ordre stable.
    assert_eq!(res.body["interests"], json!(["nature", "beach", "food"]));
    assert_eq!(res.body["pace"], "relaxed");
    assert_eq!(res.body["budget"], "budget");
    let trip = res.body["id"].as_str().unwrap().to_owned();

    // Champ absent : inchangé ; `null` : plus de préférence.
    let res = app
        .patch(
            &format!("/api/v1/trips/{trip}"),
            json!({ "version": 1, "name": "Pura Vida !" }),
            Some(&orga),
        )
        .await;
    assert_eq!(res.body["budget"], "budget");
    let res = app
        .patch(
            &format!("/api/v1/trips/{trip}"),
            json!({ "version": 2, "budget": null, "pace": "packed", "interests": [] }),
            Some(&orga),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["budget"], serde_json::Value::Null);
    assert_eq!(res.body["pace"], "packed");
    assert_eq!(res.body["interests"], json!([]));

    // Valeur inconnue : refusée.
    let res = app
        .post(
            "/api/v1/trips",
            json!({ "name": "X", "start_date": "2026-12-01", "end_date": "2026-12-08",
                    "interests": ["casino"] }),
            Some(&orga),
        )
        .await;
    assert!(res.status.is_client_error(), "{:?}", res.status);

    // Défauts.
    let id = app.create_trip(&orga, "Défauts").await;
    let res = app.get(&format!("/api/v1/trips/{id}"), Some(&orga)).await;
    assert_eq!(res.body["interests"], json!([]));
    assert_eq!(res.body["pace"], "balanced");
    assert_eq!(res.body["budget"], serde_json::Value::Null);
}
