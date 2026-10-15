//! Membres d'un voyage : liste, changement de rôle, transfert d'organisation, départ.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::state::AppState;
use crate::trips::{self, Role};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_members))
        .routes(routes!(update_member, remove_member))
}

#[derive(Serialize, ToSchema)]
pub struct Member {
    pub user_id: Uuid,
    pub display_name: String,
    /// Visible uniquement par l'organisateur (minimisation des données).
    pub email: Option<String>,
    pub role: Role,
    #[serde(with = "time::serde::rfc3339")]
    pub joined_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateMember {
    /// `organizer` transfère l'organisation : l'organisateur actuel devient éditeur.
    pub role: Role,
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    user_id: Uuid,
    display_name: String,
    email: String,
    role: String,
    joined_at: OffsetDateTime,
}

/// Membres du voyage.
#[utoipa::path(get, path = "/trips/{trip_id}/members", tag = "membres",
    params(("trip_id" = Uuid, Path)),
    responses((status = 200, body = Vec<Member>), (status = 404, body = ErrorBody)))]
async fn list_members(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<Json<Vec<Member>>> {
    let my_role = trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    let rows: Vec<MemberRow> = sqlx::query_as(
        "SELECT m.user_id, u.display_name, u.email::text AS email, m.role, m.joined_at
         FROM trip_members m JOIN users u ON u.id = m.user_id
         WHERE m.trip_id = $1
         ORDER BY CASE m.role WHEN 'organizer' THEN 0 WHEN 'editor' THEN 1 ELSE 2 END,
                  m.joined_at",
    )
    .bind(trip_id)
    .fetch_all(&state.db)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Member {
                user_id: r.user_id,
                display_name: r.display_name,
                email: (my_role == Role::Organizer).then_some(r.email),
                role: r.role.parse()?,
                joined_at: r.joined_at,
            })
        })
        .collect::<AppResult<_>>()
        .map(Json)
}

/// Change le rôle d'un membre, ou lui transfère l'organisation (organisateur).
#[utoipa::path(patch, path = "/trips/{trip_id}/members/{user_id}", tag = "membres",
    params(("trip_id" = Uuid, Path), ("user_id" = Uuid, Path)), request_body = UpdateMember,
    responses((status = 204), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn update_member(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateMember>,
) -> AppResult<StatusCode> {
    let me = current.user.id;
    let mut tx = state.db.begin().await?;
    trips::require_role(&mut *tx, trip_id, me, Role::Organizer).await?;
    if user_id == me {
        return Err(AppError::BadRequest(
            "vous ne pouvez pas changer votre propre rôle ; transférez l'organisation à un membre"
                .into(),
        ));
    }
    if trips::role_of(&mut *tx, trip_id, user_id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    if req.role == Role::Organizer {
        // Un seul organisateur : rétrograder d'abord, puis promouvoir.
        sqlx::query("UPDATE trip_members SET role = 'editor' WHERE trip_id = $1 AND user_id = $2")
            .bind(trip_id)
            .bind(me)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE trip_members SET role = $3 WHERE trip_id = $1 AND user_id = $2")
        .bind(trip_id)
        .bind(user_id)
        .bind(req.role.as_str())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Retire un membre (organisateur) ou quitte le voyage (soi-même).
#[utoipa::path(delete, path = "/trips/{trip_id}/members/{user_id}", tag = "membres",
    params(("trip_id" = Uuid, Path), ("user_id" = Uuid, Path)),
    responses((status = 204), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn remove_member(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, user_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    let me = current.user.id;
    let my_role = trips::require_role(&state.db, trip_id, me, Role::Viewer).await?;
    if user_id == me {
        if my_role == Role::Organizer {
            return Err(AppError::BadRequest(
                "l'organisateur ne peut pas quitter le voyage : transférez-le ou supprimez-le"
                    .into(),
            ));
        }
    } else if my_role != Role::Organizer {
        return Err(AppError::Forbidden);
    }
    let mut tx = state.db.begin().await?;
    let removed = sqlx::query("DELETE FROM trip_members WHERE trip_id = $1 AND user_id = $2")
        .bind(trip_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    if removed.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    // Ses avis ne comptent plus dans les notes du groupe.
    sqlx::query(
        "DELETE FROM reviews WHERE user_id = $1
         AND poi_id IN (SELECT id FROM pois WHERE trip_id = $2)",
    )
    .bind(user_id)
    .bind(trip_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
