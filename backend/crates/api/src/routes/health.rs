use axum::extract::State;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct Health {
    pub status: &'static str,
    pub database: &'static str,
    pub version: &'static str,
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(health))
}

/// État du service et de sa base de données.
#[utoipa::path(get, path = "/health", tag = "système",
    responses((status = 200, body = Health)))]
async fn health(State(state): State<AppState>) -> AppResult<Json<Health>> {
    sqlx::query("SELECT 1").execute(&state.db).await?;
    Ok(Json(Health {
        status: "ok",
        database: "ok",
        version: env!("CARGO_PKG_VERSION"),
    }))
}
