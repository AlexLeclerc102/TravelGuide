mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use serde_json::json;
use sqlx::PgPool;

use common::{session_token, TestApp, PASSWORD};

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn register_then_me(db: PgPool) {
    let app = TestApp::new(db);
    let res = app
        .post(
            "/api/v1/auth/register",
            json!({ "email": " Ana@Example.com ", "password": PASSWORD, "display_name": " Ana " }),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
    assert_eq!(res.body["email"], "ana@example.com");
    assert_eq!(res.body["display_name"], "Ana");
    assert_eq!(res.body["plan"], "free");
    assert_eq!(res.body["has_password"], true);
    assert!(res.body.get("password_hash").is_none());

    let cookie = res.set_cookie.clone().unwrap();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));

    let token = session_token(&res).unwrap();
    let me = app.get("/api/v1/me", Some(&token)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["email"], "ana@example.com");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn session_token_is_stored_hashed(db: PgPool) {
    let app = TestApp::new(db.clone());
    let token = app.register("ana@example.com", "Ana").await;
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_ne!(stored, token.as_bytes());
    assert_eq!(stored.len(), 32);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn register_rejects_invalid_input(db: PgPool) {
    let app = TestApp::new(db);
    for body in [
        json!({ "email": "pas-un-email", "password": PASSWORD, "display_name": "Ana" }),
        json!({ "email": "ana@example.com", "password": "court", "display_name": "Ana" }),
        json!({ "email": "ana@example.com", "password": PASSWORD, "display_name": "  " }),
    ] {
        let res = app.post("/api/v1/auth/register", body, None).await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST, "{:?}", res.body);
        assert_eq!(res.body["code"], "bad_request");
    }
    // Champ manquant : rejeté par la désérialisation.
    let res = app
        .post("/api/v1/auth/register", json!({ "email": "a@b.co" }), None)
        .await;
    assert!(res.status.is_client_error());
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn register_duplicate_email_is_conflict_case_insensitive(db: PgPool) {
    let app = TestApp::new(db);
    app.register("ana@example.com", "Ana").await;
    let res = app
        .post(
            "/api/v1/auth/register",
            json!({ "email": "ANA@example.com", "password": PASSWORD, "display_name": "Ana 2" }),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn login_success_and_failures(db: PgPool) {
    let app = TestApp::new(db);
    app.register("ana@example.com", "Ana").await;

    let ok = app
        .post(
            "/api/v1/auth/login",
            json!({ "email": "ANA@example.com", "password": PASSWORD }),
            None,
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert!(session_token(&ok).is_some());

    let wrong = app
        .post(
            "/api/v1/auth/login",
            json!({ "email": "ana@example.com", "password": "mauvais-mot-de-passe" }),
            None,
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);

    let unknown = app
        .post(
            "/api/v1/auth/login",
            json!({ "email": "personne@example.com", "password": PASSWORD }),
            None,
        )
        .await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    // Même message : on ne révèle pas si le compte existe.
    assert_eq!(wrong.body, unknown.body);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn login_is_rate_limited_per_email(db: PgPool) {
    let app = TestApp::new(db);
    let body = json!({ "email": "cible@example.com", "password": "mauvais-mot-de-passe" });
    for _ in 0..10 {
        let res = app.post("/api/v1/auth/login", body.clone(), None).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    }
    let res = app.post("/api/v1/auth/login", body, None).await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn logout_invalidates_session(db: PgPool) {
    let app = TestApp::new(db);
    let token = app.register("ana@example.com", "Ana").await;
    let res = app
        .post("/api/v1/auth/logout", json!({}), Some(&token))
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert!(res.set_cookie.unwrap().contains("Max-Age=0"));
    let me = app.get("/api/v1/me", Some(&token)).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn me_requires_authentication(db: PgPool) {
    let app = TestApp::new(db);
    assert_eq!(
        app.get("/api/v1/me", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    let res = app.get("/api/v1/me", Some("jeton-bidon")).await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    assert_eq!(res.body["code"], "unauthorized");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn expired_session_is_rejected(db: PgPool) {
    let app = TestApp::new(db.clone());
    let token = app.register("ana@example.com", "Ana").await;
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 minute'")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        app.get("/api/v1/me", Some(&token)).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn writes_without_allowed_origin_are_rejected(db: PgPool) {
    let app = TestApp::new(db);
    for origin in [None, Some("https://evil.example")] {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/auth/register")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        let body = json!({ "email": "a@example.com", "password": PASSWORD, "display_name": "A" });
        let res = app
            .send(req.body(Body::from(body.to_string())).unwrap())
            .await;
        assert_eq!(res.status, StatusCode::FORBIDDEN);
        assert_eq!(res.body["code"], "bad_origin");
    }
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn magic_link_full_flow(db: PgPool) {
    let app = TestApp::new(db);
    app.register("ana@example.com", "Ana").await;

    let res = app
        .post(
            "/api/v1/auth/magic-link",
            json!({ "email": "Ana@example.com" }),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::ACCEPTED);

    let mails = app.wait_for_mail(1).await;
    assert_eq!(mails[0].to, "ana@example.com");
    let token = mails[0]
        .text
        .split("token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    assert!(mails[0]
        .text
        .contains("http://localhost:5173/auth/magic?token="));

    let verify = app
        .post(
            "/api/v1/auth/magic-link/verify",
            json!({ "token": token }),
            None,
        )
        .await;
    assert_eq!(verify.status, StatusCode::OK);
    assert_eq!(verify.body["email"], "ana@example.com");
    let session = session_token(&verify).unwrap();
    assert_eq!(
        app.get("/api/v1/me", Some(&session)).await.status,
        StatusCode::OK
    );

    // Usage unique.
    let again = app
        .post(
            "/api/v1/auth/magic-link/verify",
            json!({ "token": token }),
            None,
        )
        .await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn magic_link_unknown_email_sends_nothing_but_same_answer(db: PgPool) {
    let app = TestApp::new(db);
    app.register("ana@example.com", "Ana").await;
    let known = app
        .post(
            "/api/v1/auth/magic-link",
            json!({ "email": "ana@example.com" }),
            None,
        )
        .await;
    let unknown = app
        .post(
            "/api/v1/auth/magic-link",
            json!({ "email": "inconnu@example.com" }),
            None,
        )
        .await;
    assert_eq!(known.status, unknown.status);
    assert_eq!(known.body, unknown.body);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(app.mailer.sent.lock().unwrap().len(), 1);
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn magic_link_expired_is_rejected(db: PgPool) {
    let app = TestApp::new(db.clone());
    app.register("ana@example.com", "Ana").await;
    app.post(
        "/api/v1/auth/magic-link",
        json!({ "email": "ana@example.com" }),
        None,
    )
    .await;
    let mails = app.wait_for_mail(1).await;
    let token = mails[0]
        .text
        .split("token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    sqlx::query("UPDATE magic_links SET expires_at = now() - interval '1 second'")
        .execute(&db)
        .await
        .unwrap();
    let res = app
        .post(
            "/api/v1/auth/magic-link/verify",
            json!({ "token": token }),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);
}
