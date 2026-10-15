//! API de l'application collaborative de planification de voyage.

pub mod config;
pub mod error;
pub mod routes;
pub mod state;

use std::sync::Arc;

use axum::http::{header, HeaderValue, Method};
use axum::Router;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

use crate::config::Config;
use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    info(title = "TravelGuide API", description = "Planification collaborative de voyage"),
    servers((url = "/api/v1"))
)]
struct ApiDoc;

/// Migrations SQL embarquées dans le binaire.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Construit l'application Axum complète (utilisée par `main` et par les tests).
pub fn build_app(db: PgPool, config: Config) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(HeaderValue::from_str(&config.public_origin).expect("PUBLIC_ORIGIN invalide"))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([header::CONTENT_TYPE]);

    let state = AppState {
        db,
        config: Arc::new(config),
    };

    let (api, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(routes::router())
        .split_for_parts();

    Router::new()
        .nest("/api/v1", api)
        .merge(Scalar::with_url("/api/docs", openapi.clone()))
        .route(
            "/api/openapi.json",
            axum::routing::get(move || async move { axum::Json(openapi) }),
        )
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
