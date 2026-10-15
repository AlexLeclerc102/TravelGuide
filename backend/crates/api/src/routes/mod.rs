use utoipa_axum::router::OpenApiRouter;

use crate::state::AppState;

pub mod auth;
pub mod health;
pub mod me;

/// Toutes les routes de `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(me::router())
}
