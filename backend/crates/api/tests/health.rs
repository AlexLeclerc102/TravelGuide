mod common;

use axum::http::StatusCode;
use sqlx::PgPool;

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn health_reports_database_ok(db: PgPool) {
    let app = common::TestApp::new(db);
    let res = app.get("/api/v1/health", None).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["status"], "ok");
    assert_eq!(res.body["database"], "ok");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn openapi_document_lists_routes(db: PgPool) {
    let app = common::TestApp::new(db);
    let res = app.get("/api/openapi.json", None).await;
    assert_eq!(res.status, StatusCode::OK);
    for path in [
        "/health",
        "/auth/register",
        "/auth/login",
        "/me",
        "/me/export",
    ] {
        assert!(res.body["paths"][path].is_object(), "{path} absent");
    }
}
