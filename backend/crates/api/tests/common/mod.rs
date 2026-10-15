//! Outils partagés par les tests d'intégration.

#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

pub fn app(db: PgPool) -> Router {
    let config = api::config::Config {
        database_url: String::new(),
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        public_origin: "http://localhost:5173".into(),
    };
    api::build_app(db, config)
}

pub async fn get(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let res = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}
