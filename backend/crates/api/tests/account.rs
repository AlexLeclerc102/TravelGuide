mod common;

use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;

use common::{session_token, TestApp, PASSWORD};

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn update_display_name(db: PgPool) {
    let app = TestApp::new(db);
    let token = app.register("ana@example.com", "Ana").await;
    let res = app
        .request(
            Method::PATCH,
            "/api/v1/me",
            Some(json!({ "display_name": "Ana María" })),
            Some(&token),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["display_name"], "Ana María");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn change_password_requires_current_and_closes_other_sessions(db: PgPool) {
    let app = TestApp::new(db);
    let first = app.register("ana@example.com", "Ana").await;
    let login = app
        .post(
            "/api/v1/auth/login",
            json!({ "email": "ana@example.com", "password": PASSWORD }),
            None,
        )
        .await;
    let second = session_token(&login).unwrap();

    let bad = app
        .request(
            Method::PUT,
            "/api/v1/me/password",
            Some(json!({ "current_password": "faux-mot-de-passe", "new_password": "nouveau-mot-de-passe" })),
            Some(&first),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    let ok = app
        .request(
            Method::PUT,
            "/api/v1/me/password",
            Some(json!({ "current_password": PASSWORD, "new_password": "nouveau-mot-de-passe" })),
            Some(&first),
        )
        .await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get("/api/v1/me", Some(&first)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.get("/api/v1/me", Some(&second)).await.status,
        StatusCode::UNAUTHORIZED
    );

    let relogin = app
        .post(
            "/api/v1/auth/login",
            json!({ "email": "ana@example.com", "password": "nouveau-mot-de-passe" }),
            None,
        )
        .await;
    assert_eq!(relogin.status, StatusCode::OK);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn passwordless_account_can_set_password(db: PgPool) {
    let app = TestApp::new(db.clone());
    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, display_name) VALUES ('invite@example.com', 'Invité') RETURNING id",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    let token = api::auth::session::create(&db, user_id, None)
        .await
        .unwrap();
    let me = app.get("/api/v1/me", Some(&token)).await;
    assert_eq!(me.body["has_password"], false);

    let res = app
        .request(
            Method::PUT,
            "/api/v1/me/password",
            Some(json!({ "new_password": "mon-premier-mdp" })),
            Some(&token),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get("/api/v1/me", Some(&token)).await.body["has_password"],
        true
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn export_contains_personal_data_but_no_secrets(db: PgPool) {
    let app = TestApp::new(db);
    let token = app.register("ana@example.com", "Ana").await;
    let res = app.get("/api/v1/me/export", Some(&token)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["user"]["email"], "ana@example.com");
    assert_eq!(res.body["sessions"].as_array().unwrap().len(), 1);
    let raw = res.body.to_string();
    assert!(
        !raw.contains("argon2"),
        "l'export ne doit pas contenir d'empreinte"
    );
    assert!(
        !raw.contains(&token),
        "l'export ne doit pas contenir de jeton"
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn delete_account_requires_confirmation_and_removes_everything(db: PgPool) {
    let app = TestApp::new(db.clone());
    let token = app.register("ana@example.com", "Ana").await;

    let wrong_email = app
        .request(
            Method::DELETE,
            "/api/v1/me",
            Some(json!({ "confirm_email": "autre@example.com", "password": PASSWORD })),
            Some(&token),
        )
        .await;
    assert_eq!(wrong_email.status, StatusCode::BAD_REQUEST);

    let wrong_password = app
        .request(
            Method::DELETE,
            "/api/v1/me",
            Some(json!({ "confirm_email": "ana@example.com", "password": "pas-le-bon-mdp" })),
            Some(&token),
        )
        .await;
    assert_eq!(wrong_password.status, StatusCode::BAD_REQUEST);

    let ok = app
        .request(
            Method::DELETE,
            "/api/v1/me",
            Some(json!({ "confirm_email": "ana@example.com", "password": PASSWORD })),
            Some(&token),
        )
        .await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get("/api/v1/me", Some(&token)).await.status,
        StatusCode::UNAUTHORIZED
    );

    let counts: (i64, i64) =
        sqlx::query_as("SELECT (SELECT count(*) FROM users), (SELECT count(*) FROM sessions)")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(counts, (0, 0));
}
