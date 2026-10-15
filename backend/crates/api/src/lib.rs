//! API de l'application collaborative de planification de voyage.

pub mod account;
pub mod auth;
pub mod client_ip;
pub mod config;
pub mod csrf;
pub mod days;
pub mod error;
pub mod itinerary;
pub mod mail;
pub mod opening;
pub mod places;
pub mod pois;
pub mod rate_limit;
pub mod realtime;
pub mod routes;
pub mod routing;
pub mod state;
pub mod travel;
pub mod trips;
pub mod users;
pub mod validate;
pub mod wiki;

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
use crate::mail::Mailer;
use crate::places::PlaceService;
use crate::rate_limit::RateLimiter;
use crate::routing::RoutingService;
use crate::state::AppState;
use crate::wiki::WikiService;

#[derive(OpenApi)]
#[openapi(
    info(title = "TravelGuide API", description = "Planification collaborative de voyage.\n\n\
        Authentification par cookie de session `tg_session` (HttpOnly). Les requêtes \
        d'écriture doivent porter un en-tête `Origin` autorisé."),
    servers((url = "/api/v1"))
)]
struct ApiDoc;

/// Migrations SQL embarquées dans le binaire.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Construit l'application Axum complète (utilisée par `main` et par les tests).
pub fn build_app(
    db: PgPool,
    config: Config,
    mailer: Arc<dyn Mailer>,
    places: Arc<PlaceService>,
    routing: Arc<RoutingService>,
    wiki: Arc<WikiService>,
) -> Router {
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
        .allow_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static(realtime::CLIENT_ID_HEADER),
        ]);

    let state = AppState {
        db,
        config: Arc::new(config),
        mailer,
        rate_limiter: Arc::new(RateLimiter::default()),
        places,
        routing,
        wiki,
        realtime: Arc::new(realtime::Hub::default()),
    };

    let (api, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(routes::router())
        .split_for_parts();

    Router::new()
        .nest(
            "/api/v1",
            api.layer(axum::middleware::from_fn_with_state(
                state.clone(),
                csrf::verify_origin,
            )),
        )
        .merge(Scalar::with_url("/api/docs", openapi.clone()))
        .route(
            "/api/openapi.json",
            axum::routing::get(move || async move { axum::Json(openapi) }),
        )
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
