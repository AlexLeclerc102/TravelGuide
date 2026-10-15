use utoipa_axum::router::OpenApiRouter;

use crate::state::AppState;

pub mod auth;
pub mod health;
pub mod invitations;
pub mod me;
pub mod members;
pub mod trips;

/// Toutes les routes de `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(me::router())
        .merge(trips::router())
        .merge(members::router())
        .merge(invitations::router())
}
