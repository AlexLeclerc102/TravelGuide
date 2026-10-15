//! Recherche de lieux OpenStreetMap, relayée par le backend (cache, débit limité).

use std::time::Duration;

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::places::{Place, Viewbox};
use crate::state::AppState;
use crate::validate;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(search_places))
        .routes(routes!(reverse_geocode))
}

/// Requêtes de recherche par utilisateur et par minute (le cache n'est pas décompté à part).
const PER_USER_PER_MINUTE: u32 = 30;

#[derive(Deserialize, IntoParams)]
pub struct SearchQuery {
    /// Texte recherché (2 à 200 caractères).
    pub q: String,
    /// Zone affichée `min_lon,min_lat,max_lon,max_lat` : les résultats proches sont privilégiés.
    pub viewbox: Option<String>,
}

#[derive(Deserialize, IntoParams)]
pub struct ReverseQuery {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Serialize, ToSchema)]
pub struct ReverseResult {
    /// Lieu le plus proche du point, ou `null`.
    pub place: Option<Place>,
}

fn rate_limit(state: &AppState, current: &CurrentUser) -> AppResult<()> {
    state.rate_limiter.check(
        &format!("places:{}", current.user.id),
        PER_USER_PER_MINUTE,
        Duration::from_secs(60),
    )
}

/// Recherche de lieux (OpenStreetMap / Nominatim). Données © contributeurs OpenStreetMap, ODbL.
#[utoipa::path(get, path = "/places/search", tag = "lieux", params(SearchQuery),
    responses((status = 200, body = Vec<Place>), (status = 400, body = ErrorBody),
        (status = 429, body = ErrorBody), (status = 503, body = ErrorBody)))]
async fn search_places(
    State(state): State<AppState>,
    current: CurrentUser,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Vec<Place>>> {
    let q = query.q.trim();
    let len = q.chars().count();
    if !(2..=200).contains(&len) || q.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "la recherche doit contenir entre 2 et 200 caractères".into(),
        ));
    }
    let viewbox = query
        .viewbox
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(Viewbox::parse)
        .transpose()?;
    rate_limit(&state, &current)?;
    Ok(Json(state.places.search(q, viewbox).await?))
}

/// Lieu le plus proche d'un point (ajout par clic sur la carte).
#[utoipa::path(get, path = "/places/reverse", tag = "lieux", params(ReverseQuery),
    responses((status = 200, body = ReverseResult), (status = 400, body = ErrorBody),
        (status = 429, body = ErrorBody), (status = 503, body = ErrorBody)))]
async fn reverse_geocode(
    State(state): State<AppState>,
    current: CurrentUser,
    Query(query): Query<ReverseQuery>,
) -> AppResult<Json<ReverseResult>> {
    validate::coordinates(query.lat, query.lon)?;
    rate_limit(&state, &current)?;
    Ok(Json(ReverseResult {
        place: state.places.reverse(query.lat, query.lon).await?,
    }))
}
