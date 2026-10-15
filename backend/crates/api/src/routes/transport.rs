//! Moyens de transport par jour et calcul de trajets entre lieux.

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use planner::geo::LatLon;
use planner::travel::{self, Mode};
use serde::{Deserialize, Serialize};
use time::Date;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::days::{self, TripDay};
use crate::error::{AppError, AppResult, ErrorBody};
use crate::routing::RoutedLeg;
use crate::state::AppState;
use crate::travel::{TravelOption, TravelSource};
use crate::trips::{self, normalize_modes, Role, TransportMode};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_days))
        .routes(routes!(update_day))
        .routes(routes!(compute_route))
}

/// Calculs de trajets par utilisateur et par minute.
const ROUTES_PER_USER_PER_MINUTE: u32 = 60;
/// Nombre maximal de lieux reliés en une requête.
const MAX_ROUTE_POIS: usize = 25;

/// Réglages d'un jour (remplacement complet).
#[derive(Deserialize, ToSchema)]
pub struct UpdateTripDay {
    /// Modes autorisés ce jour-là ; `null` : reprendre ceux du voyage.
    pub modes: Option<Vec<TransportMode>>,
    /// Hébergement du jour (un lieu du voyage) ; `null` : aucun.
    pub base_poi_id: Option<Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub struct RouteRequest {
    /// Lieux du voyage à relier, dans l'ordre (2 à 25).
    pub poi_ids: Vec<Uuid>,
    /// Modes à comparer. Défaut : ceux du jour `date`, sinon ceux du voyage.
    pub modes: Option<Vec<TransportMode>>,
    #[schema(value_type = Option<String>, format = Date)]
    pub date: Option<Date>,
}

/// Trajet entre deux lieux consécutifs.
#[derive(Serialize, ToSchema)]
pub struct RouteLeg {
    pub from_poi_id: Uuid,
    pub to_poi_id: Uuid,
    /// Option retenue : marche si ≤ 20 min, sinon vélo si ≤ 45 min, sinon la plus rapide.
    pub chosen: TravelOption,
    /// Toutes les options calculées (ordre : marche, vélo, voiture, transports en commun).
    pub options: Vec<TravelOption>,
    /// Tracé de l'option retenue, en `[lon, lat]` (ordre GeoJSON).
    pub geometry: Vec<[f64; 2]>,
}

#[derive(Serialize, ToSchema)]
pub struct RoutePlan {
    /// Modes comparés.
    pub modes: Vec<TransportMode>,
    pub legs: Vec<RouteLeg>,
    pub total_duration_s: u32,
    /// Au moins un trajet retenu est une estimation (vol d'oiseau ou transports en commun).
    pub has_estimates: bool,
}

/// Jours du voyage et leurs réglages effectifs.
#[utoipa::path(get, path = "/trips/{trip_id}/days", tag = "transports",
    params(("trip_id" = Uuid, Path)),
    responses((status = 200, body = Vec<TripDay>), (status = 404, body = ErrorBody)))]
async fn list_days(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<Json<Vec<TripDay>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    Ok(Json(days::list(&state.db, trip_id).await?))
}

/// Règle les modes et l'hébergement d'un jour (éditeur). Dernière écriture gagnante.
#[utoipa::path(put, path = "/trips/{trip_id}/days/{date}", tag = "transports",
    params(("trip_id" = Uuid, Path), ("date" = String, Path, format = Date)),
    request_body = UpdateTripDay,
    responses((status = 200, body = TripDay), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn update_day(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, date)): Path<(Uuid, Date)>,
    Json(req): Json<UpdateTripDay>,
) -> AppResult<Json<TripDay>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    let (start, end) = crate::pois::trip_dates(&state.db, trip_id).await?;
    if !(start..=end).contains(&date) {
        return Err(days::not_in_trip());
    }
    let modes = req.modes.as_deref().map(normalize_modes).transpose()?;
    if let Some(poi_id) = req.base_poi_id {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pois WHERE id = $1 AND trip_id = $2)")
                .bind(poi_id)
                .bind(trip_id)
                .fetch_one(&state.db)
                .await?;
        if !exists {
            return Err(AppError::BadRequest(
                "l'hébergement doit être un lieu du voyage".into(),
            ));
        }
    }

    if modes.is_none() && req.base_poi_id.is_none() {
        // Retour aux valeurs par défaut : rien à stocker.
        sqlx::query("DELETE FROM trip_days WHERE trip_id = $1 AND date = $2")
            .bind(trip_id)
            .bind(date)
            .execute(&state.db)
            .await?;
    } else {
        sqlx::query(
            "INSERT INTO trip_days (trip_id, date, modes, base_poi_id) VALUES ($1, $2, $3, $4)
             ON CONFLICT (trip_id, date)
             DO UPDATE SET modes = EXCLUDED.modes, base_poi_id = EXCLUDED.base_poi_id",
        )
        .bind(trip_id)
        .bind(date)
        .bind(&modes)
        .bind(req.base_poi_id)
        .execute(&state.db)
        .await?;
    }
    Ok(Json(days::get(&state.db, trip_id, date).await?))
}

/// Calcule les trajets entre des lieux du voyage, pour chaque mode autorisé, et retient
/// le plus adapté pour chaque étape. Les estimations sont signalées (`source`).
#[utoipa::path(post, path = "/trips/{trip_id}/routes", tag = "transports",
    params(("trip_id" = Uuid, Path)), request_body = RouteRequest,
    responses((status = 200, body = RoutePlan), (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody), (status = 429, body = ErrorBody)))]
async fn compute_route(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<RouteRequest>,
) -> AppResult<Json<RoutePlan>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    if !(2..=MAX_ROUTE_POIS).contains(&req.poi_ids.len()) {
        return Err(AppError::BadRequest(format!(
            "un trajet relie entre 2 et {MAX_ROUTE_POIS} lieux"
        )));
    }
    if req.poi_ids.windows(2).any(|w| w[0] == w[1]) {
        return Err(AppError::BadRequest(
            "deux étapes consécutives doivent être des lieux différents".into(),
        ));
    }
    let modes: Vec<TransportMode> = match (req.modes, req.date) {
        (Some(m), _) => trips::parse_modes(&normalize_modes(&m)?),
        (None, Some(date)) => days::get(&state.db, trip_id, date).await?.modes,
        (None, None) => {
            trips::get_for_user(&state.db, trip_id, current.user.id)
                .await?
                .default_modes
        }
    };
    state.rate_limiter.check(
        &format!("routes:{}", current.user.id),
        ROUTES_PER_USER_PER_MINUTE,
        Duration::from_secs(60),
    )?;

    let rows: Vec<(Uuid, f64, f64)> = sqlx::query_as(
        "SELECT id, ST_Y(location::geometry), ST_X(location::geometry)
         FROM pois WHERE trip_id = $1 AND id = ANY($2)",
    )
    .bind(trip_id)
    .bind(&req.poi_ids)
    .fetch_all(&state.db)
    .await?;
    let coords: HashMap<Uuid, LatLon> = rows
        .into_iter()
        .map(|(id, lat, lon)| (id, LatLon::new(lat, lon)))
        .collect();
    let points: Vec<LatLon> = req
        .poi_ids
        .iter()
        .map(|id| coords.get(id).copied())
        .collect::<Option<_>>()
        .ok_or_else(|| AppError::BadRequest("lieu inconnu dans ce voyage".into()))?;

    let legs_by_mode = route_all_modes(&state, &modes, &points).await;
    let mut legs = Vec::with_capacity(points.len() - 1);
    for (i, ids) in req.poi_ids.windows(2).enumerate() {
        let options: Vec<(Mode, travel::Leg)> = legs_by_mode
            .iter()
            .map(|(mode, legs)| (*mode, legs[i].leg))
            .collect();
        let (mode, leg) = travel::choose_mode(&options).expect("au moins un mode");
        let geometry = legs_by_mode
            .iter()
            .find(|(m, _)| *m == mode)
            .map(|(_, legs)| legs[i].geometry.iter().map(|p| [p.lon, p.lat]).collect())
            .unwrap_or_default();
        legs.push(RouteLeg {
            from_poi_id: ids[0],
            to_poi_id: ids[1],
            chosen: TravelOption::new(mode, leg),
            options: options
                .into_iter()
                .map(|(m, l)| TravelOption::new(m, l))
                .collect(),
            geometry,
        });
    }
    Ok(Json(RoutePlan {
        modes,
        total_duration_s: legs.iter().map(|l| l.chosen.duration_s).sum(),
        has_estimates: legs
            .iter()
            .any(|l| !matches!(l.chosen.source, TravelSource::Router)),
        legs,
    }))
}

/// Trajets pour chaque mode demandé, dans l'ordre de [`Mode`]. Les requêtes au moteur
/// partent en parallèle ; la voiture n'est calculée qu'une fois, même si les transports
/// en commun (qui en sont dérivés) sont aussi demandés.
async fn route_all_modes(
    state: &AppState,
    modes: &[TransportMode],
    points: &[LatLon],
) -> Vec<(Mode, Vec<RoutedLeg>)> {
    let wanted: Vec<Mode> = modes.iter().map(|&m| Mode::from(m)).collect();
    let needs =
        |m: Mode| wanted.contains(&m) || (m == Mode::Car && wanted.contains(&Mode::Transit));
    let run = |m: Mode| async move {
        if needs(m) {
            Some(state.routing.route(m, points).await)
        } else {
            None
        }
    };
    let (walk, bike, car) = tokio::join!(run(Mode::Walk), run(Mode::Bike), run(Mode::Car));
    let transit = wanted.contains(&Mode::Transit).then(|| {
        car.iter()
            .flatten()
            .map(|r| RoutedLeg {
                leg: travel::transit_from_car(r.leg),
                geometry: r.geometry.clone(),
            })
            .collect::<Vec<_>>()
    });
    [
        (Mode::Walk, walk),
        (Mode::Bike, bike),
        (Mode::Car, car.filter(|_| wanted.contains(&Mode::Car))),
        (Mode::Transit, transit),
    ]
    .into_iter()
    .filter_map(|(m, legs)| Some((m, legs?)))
    .collect()
}
