use utoipa_axum::router::OpenApiRouter;

use crate::state::AppState;

pub mod health;

/// Toutes les routes de `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().merge(health::router())
}
