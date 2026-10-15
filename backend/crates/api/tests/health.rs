mod common;

use axum::http::StatusCode;
use sqlx::PgPool;

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn health_reports_database_ok(db: PgPool) {
    let app = common::app(db);
    let (status, body) = common::get(&app, "/api/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "ok");
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn openapi_document_lists_health(db: PgPool) {
    let app = common::app(db);
    let (status, body) = common::get(&app, "/api/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["paths"]["/health"]["get"].is_object(), "{body}");
}
