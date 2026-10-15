//! Séjours : l'hébergement d'une suite de nuits, et le remplacement d'une zone
//! d'hébergement (hôtel pas encore choisi) par l'hébergement retenu.
//!
//! L'hébergement d'un jour est celui de sa nuit : il sert d'arrivée ce jour-là et de départ
//! le lendemain (voir `itinerary::day_bases`).

use axum::extract::{Path, State};
use axum::Json;
use planner::geo::LatLon;
use planner::itinerary::fastest_mode;
use planner::stays::{self as plan, StayProblem};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use time::{Date, Duration as Days, Time};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::days::{self, TripDay};
use crate::error::{AppError, AppResult, ErrorBody};
use crate::places::{Category, OsmRef, OsmType};
use crate::pois;
use crate::realtime::{ClientId, Topic};
use crate::state::AppState;
use crate::trips::{self, Role};
use crate::validate;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(set_stay))
        .routes(routes!(replace_stay))
        .routes(routes!(propose_stays))
        .routes(routes!(apply_proposal))
}

/// Hébergement des nuits `first_night` à `last_night` incluses.
#[derive(Deserialize, ToSchema)]
pub struct SetStay {
    /// Un lieu du voyage ; `null` : aucun hébergement ces nuits-là.
    pub poi_id: Option<Uuid>,
    #[schema(value_type = String, format = Date)]
    pub first_night: Date,
    #[schema(value_type = String, format = Date)]
    pub last_night: Date,
}

#[derive(Deserialize, ToSchema)]
pub struct ReplaceStay {
    /// Hébergement (souvent une zone à préciser) à remplacer sur toutes ses nuits.
    pub from_poi_id: Uuid,
    pub to_poi_id: Uuid,
    /// Supprime ensuite `from_poi_id` s'il s'agit d'une zone d'hébergement.
    #[serde(default)]
    pub remove_from: bool,
}

/// Vérifie que le lieu appartient au voyage ; renvoie s'il s'agit d'une zone d'hébergement.
async fn trip_poi(conn: &mut PgConnection, trip_id: Uuid, poi_id: Uuid) -> AppResult<bool> {
    sqlx::query_scalar("SELECT placeholder FROM pois WHERE id = $1 AND trip_id = $2")
        .bind(poi_id)
        .bind(trip_id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| AppError::BadRequest("l'hébergement doit être un lieu du voyage".into()))
}

/// Les jours revenus aux valeurs par défaut n'ont plus de ligne.
async fn prune_days(conn: &mut PgConnection, trip_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM trip_days WHERE trip_id = $1 AND modes IS NULL AND base_poi_id IS NULL",
    )
    .bind(trip_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Règle l'hébergement d'une suite de nuits (éditeur), sans toucher aux modes de transport.
#[utoipa::path(put, path = "/trips/{trip_id}/stays", tag = "transports",
    params(("trip_id" = Uuid, Path)), request_body = SetStay,
    responses((status = 200, body = Vec<TripDay>), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn set_stay(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<SetStay>,
) -> AppResult<Json<Vec<TripDay>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    let (start, end) = crate::pois::trip_dates(&state.db, trip_id).await?;
    if req.first_night > req.last_night
        || !(start..=end).contains(&req.first_night)
        || !(start..=end).contains(&req.last_night)
    {
        return Err(days::not_in_trip());
    }
    let mut tx = state.db.begin().await?;
    if let Some(poi_id) = req.poi_id {
        trip_poi(&mut tx, trip_id, poi_id).await?;
    }
    sqlx::query(
        "INSERT INTO trip_days (trip_id, date, base_poi_id)
         SELECT $1, d::date, $4 FROM generate_series($2::date, $3::date, interval '1 day') AS d
         ON CONFLICT (trip_id, date) DO UPDATE SET base_poi_id = EXCLUDED.base_poi_id",
    )
    .bind(trip_id)
    .bind(req.first_night)
    .bind(req.last_night)
    .bind(req.poi_id)
    .execute(&mut *tx)
    .await?;
    prune_days(&mut tx, trip_id).await?;
    tx.commit().await?;
    state.realtime.changed(
        trip_id,
        &[Topic::Days, Topic::Itinerary],
        &current.user,
        &client,
    );
    Ok(Json(days::list(&state.db, trip_id).await?))
}

/// Remplace un hébergement par un autre sur toutes ses nuits (éditeur) ; typiquement une
/// zone d'hébergement par l'hôtel finalement choisi, la zone étant alors supprimée.
#[utoipa::path(post, path = "/trips/{trip_id}/stays/replace", tag = "transports",
    params(("trip_id" = Uuid, Path)), request_body = ReplaceStay,
    responses((status = 200, body = Vec<TripDay>), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn replace_stay(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<ReplaceStay>,
) -> AppResult<Json<Vec<TripDay>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    if req.from_poi_id == req.to_poi_id {
        return Err(AppError::BadRequest(
            "choisissez un autre hébergement".into(),
        ));
    }
    let mut tx = state.db.begin().await?;
    let from_is_area = trip_poi(&mut tx, trip_id, req.from_poi_id).await?;
    trip_poi(&mut tx, trip_id, req.to_poi_id).await?;
    sqlx::query("UPDATE trip_days SET base_poi_id = $3 WHERE trip_id = $1 AND base_poi_id = $2")
        .bind(trip_id)
        .bind(req.from_poi_id)
        .bind(req.to_poi_id)
        .execute(&mut *tx)
        .await?;
    let removed = req.remove_from && from_is_area;
    if removed {
        sqlx::query("DELETE FROM pois WHERE id = $1 AND trip_id = $2")
            .bind(req.from_poi_id)
            .bind(trip_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    tracing::info!(%trip_id, from = %req.from_poi_id, to = %req.to_poi_id, removed, "hébergement remplacé");
    let topics: &[Topic] = if removed {
        &[Topic::Days, Topic::Pois, Topic::Itinerary]
    } else {
        &[Topic::Days, Topic::Itinerary]
    };
    state
        .realtime
        .changed(trip_id, topics, &current.user, &client);
    Ok(Json(days::list(&state.db, trip_id).await?))
}

// --- Proposition automatique ---------------------------------------------------------------

#[derive(Deserialize, ToSchema)]
pub struct ProposalRequest {
    /// Nuits minimum par étape (défaut 2).
    pub min_nights: Option<u8>,
    /// Lieux du voyage servant d'arrivée et de départ (ex. aéroport), s'ils sont connus.
    pub start_poi_id: Option<Uuid>,
    pub end_poi_id: Option<Uuid>,
}

/// Une étape proposée : une localité, ses nuits et les lieux visités depuis elle.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProposedStay {
    /// Localité la plus proche du centre de l'étape (sinon « Près de … »).
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub osm: Option<OsmRef>,
    #[schema(value_type = String, format = Date)]
    pub first_night: Date,
    #[schema(value_type = String, format = Date)]
    pub last_night: Date,
    pub nights: u32,
    /// Lieux du voyage rattachés à l'étape.
    #[serde(default)]
    pub poi_ids: Vec<Uuid>,
    /// Trajet estimé depuis l'étape précédente (ou l'arrivée).
    #[serde(default)]
    pub transfer_min: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StayProposal {
    /// Vide s'il n'y a encore aucun lieu à visiter.
    pub stays: Vec<ProposedStay>,
    /// Temps de trajet estimé sur le voyage (allers-retours quotidiens et changements d'étape).
    pub travel_min: u32,
}

#[derive(Deserialize, ToSchema)]
pub struct ApplyProposal {
    pub stays: Vec<ProposedStay>,
}

fn seconds(t: Time) -> u32 {
    u32::from(t.hour()) * 3600 + u32::from(t.minute()) * 60
}

/// Localité où loger près d'un point : la plus proche, rapportée au nombre d'hébergements
/// connus autour d'elle (on dort à La Fortuna plutôt que dans le hameau voisin), les villes
/// passant devant leurs quartiers.
async fn settlement(db: &PgPool, at: LatLon) -> AppResult<Option<(String, f64, f64, OsmRef)>> {
    let row: Option<(String, f64, f64, String, i64)> = sqlx::query_as(
        "WITH here AS (SELECT ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography AS g)
         SELECT s.name, ST_Y(s.location::geometry), ST_X(s.location::geometry), s.osm_type, s.osm_id
         FROM osm_places s, here
         WHERE s.osm_tag IN ('place=city', 'place=town', 'place=village')
           AND ST_DWithin(s.location, here.g, 25000)
         ORDER BY (ST_Distance(s.location, here.g) + 500)
                  -- Les quartiers d'une ville sont souvent des « villages » : la ville prime.
                  * CASE s.osm_tag WHEN 'place=city' THEN 0.5 WHEN 'place=town' THEN 0.8 ELSE 1 END
                  / (1 + (SELECT count(*) FROM osm_places h
                          WHERE h.category = 'lodging' AND ST_DWithin(h.location, s.location, 2000))),
                  s.osm_type, s.osm_id
         LIMIT 1",
    )
    .bind(at.lon)
    .bind(at.lat)
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|(name, lat, lon, t, id)| {
        Some((
            name,
            lat,
            lon,
            OsmRef {
                osm_type: OsmType::parse(&t)?,
                id,
            },
        ))
    }))
}

/// Propose où dormir et combien de nuits, d'après les lieux à visiter (calcul seul : rien
/// n'est enregistré). Trajets estimés à vol d'oiseau.
#[utoipa::path(post, path = "/trips/{trip_id}/stays/proposal", tag = "transports",
    params(("trip_id" = Uuid, Path)), request_body = ProposalRequest,
    responses((status = 200, body = StayProposal), (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody)))]
async fn propose_stays(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<ProposalRequest>,
) -> AppResult<Json<StayProposal>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    let trip = trips::get_for_user(&state.db, trip_id, current.user.id).await?;
    let pois = pois::list(
        &state.db,
        trip_id,
        current.user.id,
        (trip.start_date, trip.end_date),
    )
    .await?;
    let point = |id: Option<Uuid>| -> AppResult<Option<LatLon>> {
        id.map(|id| {
            pois.iter()
                .find(|p| p.id == id)
                .map(|p| LatLon::new(p.lat, p.lon))
                .ok_or_else(|| {
                    AppError::BadRequest(
                        "l'arrivée et le départ doivent être des lieux du voyage".into(),
                    )
                })
        })
        .transpose()
    };
    let (start, end) = (point(req.start_poi_id)?, point(req.end_poi_id)?);
    let ends = [req.start_poi_id, req.end_poi_id];
    let candidates: Vec<&pois::Poi> = pois
        .iter()
        .filter(|p| p.category != Category::Lodging && !ends.contains(&Some(p.id)))
        .collect();
    let visits: Vec<plan::Visit> = candidates
        .iter()
        .map(|p| plan::Visit {
            duration_s: p.duration_min.max(0) as u32 * 60,
            // Un lieu bien noté attire davantage la base ; sans avis, poids neutre.
            weight: p.rating.average.map_or(1.0, |a| a / 3.0),
        })
        .collect();
    let nights = ((trip.end_date - trip.start_date).whole_days() + 1) as usize;
    if visits.is_empty() {
        return Ok(Json(StayProposal {
            stays: vec![],
            travel_min: 0,
        }));
    }
    // Nœuds : les visites, puis l'arrivée et le départ ; temps du moteur de routage (repli à
    // vol d'oiseau pour ce qu'il ne calcule pas).
    let mut nodes: Vec<LatLon> = candidates
        .iter()
        .map(|p| LatLon::new(p.lat, p.lon))
        .collect();
    let mut node = |at: Option<LatLon>| {
        at.map(|a| {
            nodes.push(a);
            nodes.len() - 1
        })
    };
    let (start, end) = (node(start), node(end));
    let modes: Vec<_> = trip.default_modes.iter().map(|&m| m.into()).collect();
    let times: Vec<Vec<u32>> = state
        .routing
        .matrix(fastest_mode(&modes), &nodes)
        .await
        .into_iter()
        .map(|row| row.into_iter().map(|leg| leg.duration_s).collect())
        .collect();
    let problem = StayProblem {
        visits: &visits,
        nights,
        min_nights: usize::from(req.min_nights.unwrap_or(2).max(1)),
        day_budget_s: seconds(trip.day_end).saturating_sub(seconds(trip.day_start)),
        start,
        end,
        times: &times,
    };
    let Some(result) = plan::propose(&problem) else {
        return Ok(Json(StayProposal {
            stays: vec![],
            travel_min: 0,
        }));
    };
    let mut stays = Vec::with_capacity(result.stops.len());
    let mut night = trip.start_date;
    for stop in result.stops {
        let medoid = candidates[stop.base];
        let (name, lat, lon, osm) = match settlement(&state.db, nodes[stop.base]).await? {
            Some((name, lat, lon, osm)) => (name, lat, lon, Some(osm)),
            None => (
                format!("Près de {}", medoid.name),
                medoid.lat,
                medoid.lon,
                None,
            ),
        };
        let last = night + Days::days(stop.nights as i64 - 1);
        stays.push(ProposedStay {
            name,
            lat,
            lon,
            osm,
            first_night: night,
            last_night: last,
            nights: stop.nights as u32,
            poi_ids: stop.visits.iter().map(|&i| candidates[i].id).collect(),
            transfer_min: stop.transfer_s.div_ceil(60),
        });
        night = last + Days::days(1);
    }
    Ok(Json(StayProposal {
        stays,
        travel_min: (result.travel_s / 60) as u32,
    }))
}

/// Applique une proposition (éditeur) : une zone d'hébergement par étape, réglée sur ses
/// nuits. Les hébergements déjà prévus ces nuits-là sont remplacés.
#[utoipa::path(post, path = "/trips/{trip_id}/stays/proposal/apply", tag = "transports",
    params(("trip_id" = Uuid, Path)), request_body = ApplyProposal,
    responses((status = 200, body = Vec<TripDay>), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn apply_proposal(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<ApplyProposal>,
) -> AppResult<Json<Vec<TripDay>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    let (start, end) = pois::trip_dates(&state.db, trip_id).await?;
    if req.stays.is_empty() || req.stays.len() > 31 {
        return Err(AppError::BadRequest(
            "proposition vide ou trop longue".into(),
        ));
    }
    for s in &req.stays {
        validate::poi_name(&s.name)?;
        validate::coordinates(s.lat, s.lon)?;
        if s.first_night > s.last_night || s.first_night < start || s.last_night > end {
            return Err(days::not_in_trip());
        }
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT 1 FROM trips WHERE id = $1 FOR UPDATE")
        .bind(trip_id)
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pois WHERE trip_id = $1")
        .bind(trip_id)
        .fetch_one(&mut *tx)
        .await?;
    if count + req.stays.len() as i64 > pois::MAX_PER_TRIP {
        return Err(AppError::BadRequest(format!(
            "un voyage ne peut pas contenir plus de {} lieux",
            pois::MAX_PER_TRIP
        )));
    }
    for s in &req.stays {
        let area: Uuid = sqlx::query_scalar(
            "INSERT INTO pois (trip_id, name, category, placeholder, location, duration_min, created_by)
             VALUES ($1, $2, 'lodging', true, ST_SetSRID(ST_MakePoint($3, $4), 4326)::geography, 0, $5)
             RETURNING id",
        )
        .bind(trip_id)
        .bind(validate::poi_name(&s.name)?)
        .bind(s.lon)
        .bind(s.lat)
        .bind(current.user.id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO trip_days (trip_id, date, base_poi_id)
             SELECT $1, d::date, $4 FROM generate_series($2::date, $3::date, interval '1 day') AS d
             ON CONFLICT (trip_id, date) DO UPDATE SET base_poi_id = EXCLUDED.base_poi_id",
        )
        .bind(trip_id)
        .bind(s.first_night)
        .bind(s.last_night)
        .bind(area)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    tracing::info!(%trip_id, stays = req.stays.len(), "proposition de séjours appliquée");
    state.realtime.changed(
        trip_id,
        &[Topic::Days, Topic::Pois, Topic::Itinerary],
        &current.user,
        &client,
    );
    Ok(Json(days::list(&state.db, trip_id).await?))
}
