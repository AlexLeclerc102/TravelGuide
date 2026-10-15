//! Itinéraire : consultation par tous les membres, génération par les éditeurs.

use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::itinerary::{self, Itinerary};
use crate::state::AppState;
use crate::trips::{self, Role};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_itinerary, generate_itinerary))
}

/// Générations par utilisateur et par minute (chacune sollicite le moteur de routage).
const GENERATIONS_PER_USER_PER_MINUTE: u32 = 10;

/// Itinéraire du voyage ; `null` s'il n'a pas encore été généré.
#[utoipa::path(get, path = "/trips/{trip_id}/itinerary", tag = "itinéraire",
    params(("trip_id" = Uuid, Path)),
    responses((status = 200, body = Option<Itinerary>), (status = 404, body = ErrorBody)))]
async fn get_itinerary(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<Json<Option<Itinerary>>> {
    let trip = trips::get_for_user(&state.db, trip_id, current.user.id).await?;
    Ok(Json(itinerary::load(&state.db, &trip).await?))
}

/// Génère l'itinéraire (éditeur) et remplace le précédent : répartition des lieux par jour,
/// ordre de visite, horaires et trajets. Déterministe : mêmes données, même résultat.
#[utoipa::path(post, path = "/trips/{trip_id}/itinerary", tag = "itinéraire",
    params(("trip_id" = Uuid, Path)),
    responses((status = 200, body = Itinerary), (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody), (status = 429, body = ErrorBody)))]
async fn generate_itinerary(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<Json<Itinerary>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    state.rate_limiter.check(
        &format!("itinerary:{}", current.user.id),
        GENERATIONS_PER_USER_PER_MINUTE,
        Duration::from_secs(60),
    )?;
    itinerary::generate(&state.db, &state.routing, trip_id, current.user.id).await?;
    let trip = trips::get_for_user(&state.db, trip_id, current.user.id).await?;
    let generated = itinerary::load(&state.db, &trip).await?.ok_or_else(|| {
        AppError::Internal(anyhow::anyhow!("itinéraire introuvable après génération"))
    })?;
    Ok(Json(generated))
}
