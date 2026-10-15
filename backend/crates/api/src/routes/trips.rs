//! Création, consultation, modification et suppression des voyages.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use sqlx::PgPool;
use time::{Date, Time};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::state::AppState;
use crate::trips::{self, hhmm, normalize_modes, Role, TransportMode, Trip};
use crate::validate;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_trips, create_trip))
        .routes(routes!(get_trip, update_trip, delete_trip))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateTrip {
    pub name: String,
    pub description: Option<String>,
    #[schema(value_type = String, format = Date, example = "2026-12-01")]
    pub start_date: Date,
    #[schema(value_type = String, format = Date, example = "2026-12-10")]
    pub end_date: Date,
    /// Défaut : `America/Costa_Rica`.
    pub timezone: Option<String>,
    #[serde(default, with = "hhmm::option")]
    #[schema(value_type = Option<String>, example = "09:00")]
    pub day_start: Option<Time>,
    #[serde(default, with = "hhmm::option")]
    #[schema(value_type = Option<String>, example = "19:00")]
    pub day_end: Option<Time>,
    /// Défaut : `["car"]`.
    pub default_modes: Option<Vec<TransportMode>>,
}

/// Modification partielle ; `version` doit correspondre à la version actuelle.
#[derive(Deserialize, ToSchema)]
pub struct UpdateTrip {
    pub version: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    #[schema(value_type = Option<String>, format = Date)]
    pub start_date: Option<Date>,
    #[schema(value_type = Option<String>, format = Date)]
    pub end_date: Option<Date>,
    pub timezone: Option<String>,
    #[serde(default, with = "hhmm::option")]
    #[schema(value_type = Option<String>)]
    pub day_start: Option<Time>,
    #[serde(default, with = "hhmm::option")]
    #[schema(value_type = Option<String>)]
    pub day_end: Option<Time>,
    pub default_modes: Option<Vec<TransportMode>>,
}

async fn validate_timezone(db: &PgPool, tz: &str) -> AppResult<()> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_timezone_names WHERE name = $1)")
            .bind(tz)
            .fetch_one(db)
            .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "fuseau horaire inconnu : {tz}"
        )))
    }
}

/// Voyages dont je suis membre.
#[utoipa::path(get, path = "/trips", tag = "voyages",
    responses((status = 200, body = Vec<Trip>), (status = 401, body = ErrorBody)))]
async fn list_trips(
    State(state): State<AppState>,
    current: CurrentUser,
) -> AppResult<Json<Vec<Trip>>> {
    Ok(Json(
        trips::list_for_user(&state.db, current.user.id).await?,
    ))
}

/// Crée un voyage (abonnement organisateur requis) ; le créateur en devient l'organisateur.
#[utoipa::path(post, path = "/trips", tag = "voyages", request_body = CreateTrip,
    responses((status = 201, body = Trip), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody, description = "`plan_required` : abonnement nécessaire")))]
async fn create_trip(
    State(state): State<AppState>,
    current: CurrentUser,
    Json(req): Json<CreateTrip>,
) -> AppResult<(StatusCode, Json<Trip>)> {
    if current.user.plan != "organizer" {
        return Err(AppError::PlanRequired);
    }
    let name = validate::trip_name(&req.name)?;
    let description = validate::description(req.description.as_deref().unwrap_or(""))?;
    validate::trip_dates(req.start_date, req.end_date)?;
    let timezone = req.timezone.unwrap_or_else(|| "America/Costa_Rica".into());
    validate_timezone(&state.db, &timezone).await?;
    let day_start = req.day_start.unwrap_or(time::macros::time!(9:00));
    let day_end = req.day_end.unwrap_or(time::macros::time!(19:00));
    validate::day_window(day_start, day_end)?;
    let modes = normalize_modes(&req.default_modes.unwrap_or(vec![TransportMode::Car]))?;

    let mut tx = state.db.begin().await?;
    let trip_id: Uuid = sqlx::query_scalar(
        "INSERT INTO trips (name, description, start_date, end_date, timezone,
                            day_start, day_end, default_modes, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
    )
    .bind(&name)
    .bind(&description)
    .bind(req.start_date)
    .bind(req.end_date)
    .bind(&timezone)
    .bind(day_start)
    .bind(day_end)
    .bind(&modes)
    .bind(current.user.id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO trip_members (trip_id, user_id, role) VALUES ($1, $2, 'organizer')")
        .bind(trip_id)
        .bind(current.user.id)
        .execute(&mut *tx)
        .await?;
    let trip = trips::get_for_user(&mut *tx, trip_id, current.user.id).await?;
    tx.commit().await?;
    tracing::info!(%trip_id, "voyage créé");
    Ok((StatusCode::CREATED, Json(trip)))
}

/// Détail d'un voyage (membres uniquement).
#[utoipa::path(get, path = "/trips/{trip_id}", tag = "voyages",
    params(("trip_id" = Uuid, Path)),
    responses((status = 200, body = Trip), (status = 404, body = ErrorBody)))]
async fn get_trip(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<Json<Trip>> {
    Ok(Json(
        trips::get_for_user(&state.db, trip_id, current.user.id).await?,
    ))
}

/// Modifie les paramètres du voyage (organisateur).
#[utoipa::path(patch, path = "/trips/{trip_id}", tag = "voyages",
    params(("trip_id" = Uuid, Path)), request_body = UpdateTrip,
    responses((status = 200, body = Trip), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody, description = "Version obsolète : rechargez le voyage")))]
async fn update_trip(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<UpdateTrip>,
) -> AppResult<Json<Trip>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Organizer).await?;
    let cur = trips::get_for_user(&state.db, trip_id, current.user.id).await?;

    let name = match req.name {
        Some(n) => validate::trip_name(&n)?,
        None => cur.name,
    };
    let description = match req.description {
        Some(d) => validate::description(&d)?,
        None => cur.description,
    };
    let start_date = req.start_date.unwrap_or(cur.start_date);
    let end_date = req.end_date.unwrap_or(cur.end_date);
    validate::trip_dates(start_date, end_date)?;
    let timezone = req.timezone.unwrap_or(cur.timezone);
    validate_timezone(&state.db, &timezone).await?;
    let day_start = req.day_start.unwrap_or(cur.day_start);
    let day_end = req.day_end.unwrap_or(cur.day_end);
    validate::day_window(day_start, day_end)?;
    let modes = normalize_modes(&req.default_modes.unwrap_or(cur.default_modes))?;

    let updated = sqlx::query(
        "UPDATE trips SET name = $3, description = $4, start_date = $5, end_date = $6,
             timezone = $7, day_start = $8, day_end = $9, default_modes = $10,
             version = version + 1
         WHERE id = $1 AND version = $2",
    )
    .bind(trip_id)
    .bind(req.version)
    .bind(&name)
    .bind(&description)
    .bind(start_date)
    .bind(end_date)
    .bind(&timezone)
    .bind(day_start)
    .bind(day_end)
    .bind(&modes)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "le voyage a été modifié entre-temps, rechargez-le".into(),
        ));
    }
    Ok(Json(
        trips::get_for_user(&state.db, trip_id, current.user.id).await?,
    ))
}

/// Supprime définitivement le voyage et tout son contenu (organisateur).
#[utoipa::path(delete, path = "/trips/{trip_id}", tag = "voyages",
    params(("trip_id" = Uuid, Path)),
    responses((status = 204), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn delete_trip(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Organizer).await?;
    sqlx::query("DELETE FROM trips WHERE id = $1")
        .bind(trip_id)
        .execute(&state.db)
        .await?;
    tracing::info!(%trip_id, "voyage supprimé");
    Ok(StatusCode::NO_CONTENT)
}
