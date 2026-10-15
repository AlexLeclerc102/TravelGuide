//! Avis des membres : chacun note (1 à 5) et commente chaque POI, et voit l'avis des autres.
//!
//! Tous les membres, lecteurs compris, peuvent donner leur avis : c'est une opinion
//! personnelle, pas une modification du voyage.

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::pois::{self, Poi};
use crate::realtime::{ClientId, Topic};
use crate::state::AppState;
use crate::trips::{self, Role};
use crate::validate;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_reviews))
        .routes(routes!(put_my_review, delete_my_review))
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Review {
    pub user_id: Uuid,
    pub display_name: String,
    pub rating: i16,
    pub comment: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
pub struct PutReview {
    /// Note de 1 à 5.
    pub rating: u8,
    pub comment: Option<String>,
}

/// Vérifie l'appartenance au voyage et que le POI en fait partie.
async fn check_access(
    state: &AppState,
    current: &CurrentUser,
    trip_id: Uuid,
    poi_id: Uuid,
) -> AppResult<()> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pois WHERE id = $1 AND trip_id = $2)")
            .bind(poi_id)
            .bind(trip_id)
            .fetch_one(&state.db)
            .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

async fn poi_for(
    state: &AppState,
    current: &CurrentUser,
    trip_id: Uuid,
    poi_id: Uuid,
) -> AppResult<Json<Poi>> {
    let dates = pois::trip_dates(&state.db, trip_id).await?;
    Ok(Json(
        pois::get(&state.db, trip_id, poi_id, current.user.id, dates).await?,
    ))
}

/// Avis de chaque membre sur un POI, les plus récents d'abord.
#[utoipa::path(get, path = "/trips/{trip_id}/pois/{poi_id}/reviews", tag = "avis",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    responses((status = 200, body = Vec<Review>), (status = 404, body = ErrorBody)))]
async fn list_reviews(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<Review>>> {
    check_access(&state, &current, trip_id, poi_id).await?;
    let reviews = sqlx::query_as(
        "SELECT r.user_id, u.display_name, r.rating, r.comment, r.updated_at
         FROM reviews r JOIN users u ON u.id = r.user_id
         WHERE r.poi_id = $1
         ORDER BY r.updated_at DESC, r.user_id",
    )
    .bind(poi_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(reviews))
}

/// Donne ou modifie mon avis ; renvoie le POI avec les notes du groupe recalculées.
#[utoipa::path(put, path = "/trips/{trip_id}/pois/{poi_id}/reviews/me", tag = "avis",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)), request_body = PutReview,
    responses((status = 200, body = Poi), (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody)))]
async fn put_my_review(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<PutReview>,
) -> AppResult<Json<Poi>> {
    check_access(&state, &current, trip_id, poi_id).await?;
    if !planner::ratings::RATING_RANGE.contains(&req.rating) {
        return Err(AppError::BadRequest(
            "la note doit être comprise entre 1 et 5".into(),
        ));
    }
    let comment = validate::review_comment(req.comment.as_deref().unwrap_or(""))?;
    sqlx::query(
        "INSERT INTO reviews (poi_id, user_id, rating, comment) VALUES ($1, $2, $3, $4)
         ON CONFLICT (poi_id, user_id)
         DO UPDATE SET rating = EXCLUDED.rating, comment = EXCLUDED.comment",
    )
    .bind(poi_id)
    .bind(current.user.id)
    .bind(i16::from(req.rating))
    .bind(&comment)
    .execute(&state.db)
    .await?;
    state.realtime.changed(
        trip_id,
        &[Topic::Pois, Topic::Reviews],
        &current.user,
        &client,
    );
    poi_for(&state, &current, trip_id, poi_id).await
}

/// Retire mon avis ; renvoie le POI avec les notes du groupe recalculées.
#[utoipa::path(delete, path = "/trips/{trip_id}/pois/{poi_id}/reviews/me", tag = "avis",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    responses((status = 200, body = Poi), (status = 404, body = ErrorBody)))]
async fn delete_my_review(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Poi>> {
    check_access(&state, &current, trip_id, poi_id).await?;
    sqlx::query("DELETE FROM reviews WHERE poi_id = $1 AND user_id = $2")
        .bind(poi_id)
        .bind(current.user.id)
        .execute(&state.db)
        .await?;
    state.realtime.changed(
        trip_id,
        &[Topic::Pois, Topic::Reviews],
        &current.user,
        &client,
    );
    poi_for(&state, &current, trip_id, poi_id).await
}
