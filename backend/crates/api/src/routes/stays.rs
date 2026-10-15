//! Séjours : l'hébergement d'une suite de nuits, et le remplacement d'une zone
//! d'hébergement (hôtel pas encore choisi) par l'hébergement retenu.
//!
//! L'hébergement d'un jour est celui de sa nuit : il sert d'arrivée ce jour-là et de départ
//! le lendemain (voir `itinerary::day_bases`).

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use sqlx::PgConnection;
use time::Date;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::days::{self, TripDay};
use crate::error::{AppError, AppResult, ErrorBody};
use crate::realtime::{ClientId, Topic};
use crate::state::AppState;
use crate::trips::{self, Role};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(set_stay))
        .routes(routes!(replace_stay))
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
