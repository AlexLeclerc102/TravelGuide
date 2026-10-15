//! Itinéraire : consultation par tous les membres, génération par les éditeurs.

use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use time::Date;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::itinerary::{self, Adjustment, Itinerary};
use crate::realtime::{ClientId, Topic};
use crate::state::AppState;
use crate::trips::{self, Role};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_itinerary, generate_itinerary))
        .routes(routes!(move_stop))
        .routes(routes!(lock_stop))
        .routes(routes!(recompute_day))
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
    client: ClientId,
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
    state
        .realtime
        .changed(trip_id, &[Topic::Itinerary], &current.user, &client);
    Ok(Json(generated))
}

/// Déplacement d'un lieu dans l'itinéraire.
#[derive(Deserialize, ToSchema)]
pub struct MoveRequest {
    /// Version actuelle de l'itinéraire.
    pub version: i64,
    pub poi_id: Uuid,
    /// Jour d'arrivée ; `null` retire le lieu de l'itinéraire (il y reste à la régénération).
    #[schema(value_type = Option<String>, format = Date)]
    pub date: Option<Date>,
    /// Position dans la journée (0 = première étape) ; au-delà, en dernier.
    #[serde(default)]
    pub position: usize,
}

#[derive(Deserialize, ToSchema)]
pub struct LockRequest {
    pub version: i64,
    pub locked: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct VersionRequest {
    pub version: i64,
}

async fn apply(
    state: &AppState,
    current: &CurrentUser,
    client: &ClientId,
    trip_id: Uuid,
    version: i64,
    adjustment: Adjustment,
) -> AppResult<Json<Itinerary>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    itinerary::adjust(
        &state.db,
        &state.routing,
        trip_id,
        current.user.id,
        version,
        adjustment,
    )
    .await?;
    let trip = trips::get_for_user(&state.db, trip_id, current.user.id).await?;
    let updated = itinerary::load(&state.db, &trip).await?.ok_or_else(|| {
        AppError::Internal(anyhow::anyhow!("itinéraire introuvable après ajustement"))
    })?;
    state
        .realtime
        .changed(trip_id, &[Topic::Itinerary], &current.user, client);
    Ok(Json(updated))
}

/// Déplace un lieu (éditeur) : réordonner une journée, changer de jour, ajouter un lieu
/// hors itinéraire, ou le retirer (`date: null`). Seuls les jours touchés sont recalculés,
/// dans l'ordre choisi : une contrainte non respectée est signalée sur l'étape.
#[utoipa::path(post, path = "/trips/{trip_id}/itinerary/moves", tag = "itinéraire",
    params(("trip_id" = Uuid, Path)), request_body = MoveRequest,
    responses((status = 200, body = Itinerary), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody, description = "Version obsolète : rechargez l'itinéraire")))]
async fn move_stop(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<MoveRequest>,
) -> AppResult<Json<Itinerary>> {
    let adjustment = Adjustment::Move {
        poi_id: req.poi_id,
        date: req.date,
        position: req.position,
    };
    apply(&state, &current, &client, trip_id, req.version, adjustment).await
}

/// Verrouille ou déverrouille une étape (éditeur) : une étape verrouillée garde son jour
/// et son ordre quand on régénère l'itinéraire.
#[utoipa::path(patch, path = "/trips/{trip_id}/itinerary/stops/{poi_id}", tag = "itinéraire",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)), request_body = LockRequest,
    responses((status = 200, body = Itinerary), (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody), (status = 409, body = ErrorBody)))]
async fn lock_stop(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<LockRequest>,
) -> AppResult<Json<Itinerary>> {
    let adjustment = Adjustment::Lock {
        poi_id,
        locked: req.locked,
    };
    apply(&state, &current, &client, trip_id, req.version, adjustment).await
}

/// Recalcule les horaires et trajets d'une journée dans son ordre actuel, avec les lieux et
/// réglages à jour (éditeur). Les autres jours ne changent pas.
#[utoipa::path(post, path = "/trips/{trip_id}/itinerary/days/{date}/recompute", tag = "itinéraire",
    params(("trip_id" = Uuid, Path), ("date" = String, Path, format = Date)),
    request_body = VersionRequest,
    responses((status = 200, body = Itinerary), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody)))]
async fn recompute_day(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, date)): Path<(Uuid, Date)>,
    Json(req): Json<VersionRequest>,
) -> AppResult<Json<Itinerary>> {
    apply(
        &state,
        &current,
        &client,
        trip_id,
        req.version,
        Adjustment::Recompute { date },
    )
    .await
}
