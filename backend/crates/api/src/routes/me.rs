//! Compte de l'utilisateur connecté : profil, mot de passe, export et suppression (RGPD).

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::account::{self, AccountExport};
use crate::auth::{password, session, CurrentUser};
use crate::error::{AppError, AppResult, ErrorBody};
use crate::state::AppState;
use crate::users::{User, UserRow};
use crate::validate;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_me, update_me, delete_me))
        .routes(routes!(change_password))
        .routes(routes!(export_me))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateMe {
    pub display_name: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ChangePassword {
    /// Obligatoire si le compte a déjà un mot de passe.
    pub current_password: Option<String>,
    pub new_password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct DeleteAccount {
    /// Doit correspondre à l'email du compte (confirmation explicite).
    pub confirm_email: String,
    /// Obligatoire si le compte a un mot de passe.
    pub password: Option<String>,
}

/// Profil de l'utilisateur connecté.
#[utoipa::path(get, path = "/me", tag = "compte",
    responses((status = 200, body = User), (status = 401, body = ErrorBody)))]
async fn get_me(current: CurrentUser) -> Json<User> {
    Json(current.user.into())
}

/// Modifie le profil.
#[utoipa::path(patch, path = "/me", tag = "compte", request_body = UpdateMe,
    responses((status = 200, body = User), (status = 400, body = ErrorBody)))]
async fn update_me(
    State(state): State<AppState>,
    current: CurrentUser,
    Json(req): Json<UpdateMe>,
) -> AppResult<Json<User>> {
    let mut user = current.user;
    if let Some(name) = req.display_name {
        let name = validate::display_name(&name)?;
        user = sqlx::query_as(&format!(
            "UPDATE users SET display_name = $2 WHERE id = $1 RETURNING {}",
            UserRow::COLUMNS
        ))
        .bind(user.id)
        .bind(name)
        .fetch_one(&state.db)
        .await?;
    }
    Ok(Json(user.into()))
}

/// Définit ou change le mot de passe. Les autres sessions sont fermées.
#[utoipa::path(put, path = "/me/password", tag = "compte", request_body = ChangePassword,
    responses((status = 204), (status = 400, body = ErrorBody), (status = 401, body = ErrorBody)))]
async fn change_password(
    State(state): State<AppState>,
    current: CurrentUser,
    Json(req): Json<ChangePassword>,
) -> AppResult<StatusCode> {
    if let Some(existing) = current.user.password_hash.clone() {
        let given = req.current_password.unwrap_or_default();
        if !password::verify(given, Some(existing)).await? {
            return Err(AppError::BadRequest("mot de passe actuel incorrect".into()));
        }
    }
    validate::password(&req.new_password)?;
    let hash = password::hash(req.new_password).await?;
    sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(current.user.id)
        .bind(hash)
        .execute(&state.db)
        .await?;
    session::delete_others(&state.db, current.user.id, &current.session_token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Export de toutes les données personnelles du compte (RGPD, art. 15 et 20).
#[utoipa::path(get, path = "/me/export", tag = "compte",
    responses((status = 200, body = AccountExport)))]
async fn export_me(
    State(state): State<AppState>,
    current: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let export = account::export(&state.db, current.user.into()).await?;
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"travelguide-export.json\"",
        )],
        Json(export),
    ))
}

/// Supprime définitivement le compte (RGPD, art. 17).
#[utoipa::path(delete, path = "/me", tag = "compte", request_body = DeleteAccount,
    responses((status = 204), (status = 400, body = ErrorBody)))]
async fn delete_me(
    State(state): State<AppState>,
    current: CurrentUser,
    jar: CookieJar,
    Json(req): Json<DeleteAccount>,
) -> AppResult<(StatusCode, CookieJar)> {
    let user = current.user;
    if req.confirm_email.trim().to_lowercase() != user.email.to_lowercase() {
        return Err(AppError::BadRequest(
            "l'email de confirmation ne correspond pas".into(),
        ));
    }
    if let Some(hash) = user.password_hash.clone() {
        if !password::verify(req.password.unwrap_or_default(), Some(hash)).await? {
            return Err(AppError::BadRequest("mot de passe incorrect".into()));
        }
    }
    account::delete(&state.db, user.id).await?;
    tracing::info!(user_id = %user.id, "compte supprimé");
    Ok((
        StatusCode::NO_CONTENT,
        jar.add(session::removal_cookie(&state.config)),
    ))
}
