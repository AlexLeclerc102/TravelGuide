use utoipa_axum::router::OpenApiRouter;

use crate::state::AppState;

pub mod auth;
pub mod health;
pub mod invitations;
pub mod itinerary;
pub mod me;
pub mod members;
pub mod places;
pub mod pois;
pub mod realtime;
pub mod reviews;
pub mod transport;
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
        .merge(places::router())
        .merge(pois::router())
        .merge(reviews::router())
        .merge(transport::router())
        .merge(itinerary::router())
        .merge(realtime::router())
}
