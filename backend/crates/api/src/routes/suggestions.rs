//! Suggestions de lieux et d'hébergements, et suggestions écartées par le groupe.

use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{AppResult, ErrorBody};
use crate::places::{Category, OsmRef};
use crate::realtime::{ClientId, Topic};
use crate::regions::{self, Regions};
use crate::state::AppState;
use crate::suggest::{self, Request, Suggestions};
use crate::trips::{self, Role};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_suggestions))
        .routes(routes!(list_regions))
        .routes(routes!(dismiss_suggestion, reset_dismissals))
}

/// Calculs de suggestions par utilisateur et par minute.
const PER_USER_PER_MINUTE: u32 = 30;

#[derive(Deserialize, IntoParams)]
pub struct SuggestionQuery {
    /// Lieux d'une seule catégorie (hors `lodging`).
    pub category: Option<Category>,
    /// Hébergements autour de ce lieu du voyage (zone d'hébergement, hôtel…).
    pub lodging_near: Option<Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub struct Dismissal {
    pub osm: OsmRef,
    pub category: Category,
}

/// Lieux à ajouter au voyage, ou hébergements autour d'un séjour, avec les raisons de
/// chaque suggestion. Données © contributeurs OpenStreetMap (ODbL).
#[utoipa::path(get, path = "/trips/{trip_id}/suggestions", tag = "suggestions",
    params(("trip_id" = Uuid, Path), SuggestionQuery),
    responses((status = 200, body = Suggestions), (status = 404, body = ErrorBody),
        (status = 429, body = ErrorBody)))]
async fn list_suggestions(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
    Query(query): Query<SuggestionQuery>,
) -> AppResult<Json<Suggestions>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    state.rate_limiter.check(
        &format!("suggestions:{}", current.user.id),
        PER_USER_PER_MINUTE,
        Duration::from_secs(60),
    )?;
    let request = match query.lodging_near {
        Some(poi_id) => Request::LodgingNear(poi_id),
        None => Request::Places(query.category.filter(|&c| c != Category::Lodging)),
    };
    Ok(Json(
        suggest::suggest(&state.db, &state.wiki, trip_id, current.user.id, request).await?,
    ))
}

/// Régions du pays à découvrir : lieux notables regroupés autour d'une localité où loger,
/// avec leurs points forts et les nuits conseillées. Données © contributeurs OpenStreetMap
/// (ODbL) et Wikidata.
#[utoipa::path(get, path = "/trips/{trip_id}/regions", tag = "suggestions",
    params(("trip_id" = Uuid, Path)),
    responses((status = 200, body = Regions), (status = 404, body = ErrorBody),
        (status = 429, body = ErrorBody)))]
async fn list_regions(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<Json<Regions>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    state.rate_limiter.check(
        &format!("regions:{}", current.user.id),
        PER_USER_PER_MINUTE,
        Duration::from_secs(60),
    )?;
    Ok(Json(
        regions::regions(&state.db, &state.routing, trip_id, current.user.id).await?,
    ))
}

/// Écarte une suggestion pour tout le groupe (éditeur).
#[utoipa::path(post, path = "/trips/{trip_id}/suggestions/dismissals", tag = "suggestions",
    params(("trip_id" = Uuid, Path)), request_body = Dismissal,
    responses((status = 204), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn dismiss_suggestion(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<Dismissal>,
) -> AppResult<StatusCode> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    suggest::dismiss(&state.db, trip_id, current.user.id, req.osm, req.category).await?;
    state
        .realtime
        .changed(trip_id, &[Topic::Suggestions], &current.user, &client);
    Ok(StatusCode::NO_CONTENT)
}

/// Propose de nouveau toutes les suggestions écartées (éditeur).
#[utoipa::path(delete, path = "/trips/{trip_id}/suggestions/dismissals", tag = "suggestions",
    params(("trip_id" = Uuid, Path)),
    responses((status = 204), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn reset_dismissals(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path(trip_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    suggest::reset_dismissals(&state.db, trip_id).await?;
    state
        .realtime
        .changed(trip_id, &[Topic::Suggestions], &current.user, &client);
    Ok(StatusCode::NO_CONTENT)
}
