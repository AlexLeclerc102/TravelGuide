//! Invitations par lien : création et révocation (organisateur), aperçu et acceptation (public).
//!
//! Un invité sans compte donne seulement son prénom et son email : un compte sans mot de
//! passe est créé et connecté immédiatement. Si l'email appartient déjà à un compte, il faut
//! d'abord se connecter (sinon n'importe qui pourrait prendre la session d'un autre).

use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::Json;
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::{session, token, CurrentUser};
use crate::client_ip::ClientIp;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::realtime::{ClientId, Topic};
use crate::state::AppState;
use crate::trips::{self, Role};
use crate::users::{self, User};
use crate::validate;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_invitations, create_invitation))
        .routes(routes!(revoke_invitation))
        .routes(routes!(preview_invitation))
        .routes(routes!(accept_invitation))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateInvitation {
    /// `editor` ou `viewer`.
    pub role: Role,
    /// Validité en jours (1 à 30, défaut 7).
    pub expires_in_days: Option<i32>,
    /// Nombre maximal d'utilisations (1 à 100) ; illimité si absent.
    pub max_uses: Option<i32>,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Invitation {
    pub id: Uuid,
    pub role: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub max_uses: Option<i32>,
    pub uses: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
}

/// Invitation créée : le lien n'est affiché qu'une fois (seule son empreinte est stockée).
#[derive(Serialize, ToSchema)]
pub struct CreatedInvitation {
    #[serde(flatten)]
    pub invitation: Invitation,
    pub url: String,
}

#[derive(Serialize, ToSchema)]
pub struct InvitationPreview {
    pub trip_name: String,
    #[schema(value_type = String, format = Date)]
    pub start_date: Date,
    #[schema(value_type = String, format = Date)]
    pub end_date: Date,
    pub invited_by: Option<String>,
    pub role: Role,
}

#[derive(Deserialize, ToSchema)]
pub struct AcceptInvitation {
    /// Requis sans session.
    pub display_name: Option<String>,
    /// Requis sans session.
    pub email: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Accepted {
    pub trip_id: Uuid,
    pub role: Role,
    pub user: User,
}

/// Invitations du voyage (organisateur).
#[utoipa::path(get, path = "/trips/{trip_id}/invitations", tag = "invitations",
    params(("trip_id" = Uuid, Path)),
    responses((status = 200, body = Vec<Invitation>), (status = 403, body = ErrorBody)))]
async fn list_invitations(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<Json<Vec<Invitation>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Organizer).await?;
    let list = sqlx::query_as(
        "SELECT id, role, created_at, expires_at, max_uses, uses, revoked_at
         FROM invitations WHERE trip_id = $1 ORDER BY created_at DESC",
    )
    .bind(trip_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(list))
}

/// Crée un lien d'invitation (organisateur).
#[utoipa::path(post, path = "/trips/{trip_id}/invitations", tag = "invitations",
    params(("trip_id" = Uuid, Path)), request_body = CreateInvitation,
    responses((status = 201, body = CreatedInvitation), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody)))]
async fn create_invitation(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<CreateInvitation>,
) -> AppResult<(StatusCode, Json<CreatedInvitation>)> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Organizer).await?;
    if req.role == Role::Organizer {
        return Err(AppError::BadRequest(
            "une invitation donne le rôle éditeur ou lecteur".into(),
        ));
    }
    let days = req.expires_in_days.unwrap_or(7);
    if !(1..=30).contains(&days) {
        return Err(AppError::BadRequest(
            "la validité doit être comprise entre 1 et 30 jours".into(),
        ));
    }
    if req.max_uses.is_some_and(|m| !(1..=100).contains(&m)) {
        return Err(AppError::BadRequest(
            "le nombre d'utilisations doit être compris entre 1 et 100".into(),
        ));
    }
    let (token, hash) = token::generate();
    let invitation: Invitation = sqlx::query_as(
        "INSERT INTO invitations (trip_id, token_hash, role, created_by, expires_at, max_uses)
         VALUES ($1, $2, $3, $4, now() + make_interval(days => $5), $6)
         RETURNING id, role, created_at, expires_at, max_uses, uses, revoked_at",
    )
    .bind(trip_id)
    .bind(&hash)
    .bind(req.role.as_str())
    .bind(current.user.id)
    .bind(days)
    .bind(req.max_uses)
    .fetch_one(&state.db)
    .await?;
    let url = format!("{}/invite/{token}", state.config.public_origin);
    state
        .realtime
        .changed(trip_id, &[Topic::Invitations], &current.user, &client);
    Ok((
        StatusCode::CREATED,
        Json(CreatedInvitation { invitation, url }),
    ))
}

/// Révoque une invitation (organisateur).
#[utoipa::path(delete, path = "/trips/{trip_id}/invitations/{invitation_id}", tag = "invitations",
    params(("trip_id" = Uuid, Path), ("invitation_id" = Uuid, Path)),
    responses((status = 204), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn revoke_invitation(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, invitation_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Organizer).await?;
    let res = sqlx::query(
        "UPDATE invitations SET revoked_at = coalesce(revoked_at, now())
         WHERE id = $1 AND trip_id = $2",
    )
    .bind(invitation_id)
    .bind(trip_id)
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    state
        .realtime
        .changed(trip_id, &[Topic::Invitations], &current.user, &client);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct ValidInvitation {
    id: Uuid,
    trip_id: Uuid,
    role: String,
}

/// Invitation utilisable (ni révoquée, ni expirée, ni épuisée). Verrouille la ligne si `for_update`.
async fn find_valid(
    db: impl sqlx::PgExecutor<'_>,
    token: &str,
    for_update: bool,
) -> AppResult<ValidInvitation> {
    if token.len() > 128 {
        return Err(AppError::NotFound);
    }
    let lock = if for_update { "FOR UPDATE" } else { "" };
    let inv: Option<ValidInvitation> = sqlx::query_as(&format!(
        "SELECT id, trip_id, role FROM invitations
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()
           AND (max_uses IS NULL OR uses < max_uses)
         {lock}"
    ))
    .bind(token::hash(token))
    .fetch_optional(db)
    .await?;
    inv.ok_or_else(|| AppError::BadRequest("invitation invalide ou expirée".into()))
}

/// Aperçu public d'une invitation (nom et dates du voyage, rôle proposé).
#[utoipa::path(get, path = "/invitations/{token}", tag = "invitations",
    params(("token" = String, Path)),
    responses((status = 200, body = InvitationPreview), (status = 400, body = ErrorBody)))]
async fn preview_invitation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> AppResult<Json<InvitationPreview>> {
    let inv = find_valid(&state.db, &token, false).await?;
    let (trip_name, start_date, end_date, invited_by): (String, Date, Date, Option<String>) =
        sqlx::query_as(
            "SELECT t.name, t.start_date, t.end_date, u.display_name
             FROM invitations i JOIN trips t ON t.id = i.trip_id
             LEFT JOIN users u ON u.id = i.created_by
             WHERE i.id = $1",
        )
        .bind(inv.id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(InvitationPreview {
        trip_name,
        start_date,
        end_date,
        invited_by,
        role: inv.role.parse()?,
    }))
}

/// Rejoint le voyage. Sans session : crée un compte sans mot de passe et le connecte.
#[utoipa::path(post, path = "/invitations/{token}/accept", tag = "invitations",
    params(("token" = String, Path)), request_body = AcceptInvitation,
    responses((status = 200, body = Accepted),
        (status = 400, body = ErrorBody),
        (status = 409, body = ErrorBody,
            description = "`account_exists` : connectez-vous d'abord avec cet email")))]
async fn accept_invitation(
    State(state): State<AppState>,
    current: Option<CurrentUser>,
    ip: ClientIp,
    jar: CookieJar,
    headers: HeaderMap,
    Path(token): Path<String>,
    Json(req): Json<AcceptInvitation>,
) -> AppResult<(CookieJar, Json<Accepted>)> {
    let mut tx = state.db.begin().await?;
    let inv = find_valid(&mut *tx, &token, true).await?;
    let invited_role: Role = inv.role.parse()?;

    let (user, new_session) = match current {
        Some(c) => (c.user, None),
        None => {
            state.rate_limiter.check(
                &format!("invite-signup:{}", ip.key()),
                20,
                Duration::from_secs(3600),
            )?;
            let email = validate::email(req.email.as_deref().unwrap_or(""))?;
            let name = validate::display_name(req.display_name.as_deref().unwrap_or(""))?;
            if users::find_by_email(&mut *tx, &email).await?.is_some() {
                return Err(AppError::AccountExists);
            }
            let user = users::create(&mut *tx, &email, &name, None, "free")
                .await?
                .ok_or(AppError::AccountExists)?;
            tracing::info!(user_id = %user.id, "compte invité créé");
            (user, Some(()))
        }
    };

    let role = match trips::role_of(&mut *tx, inv.trip_id, user.id).await? {
        // Déjà membre : on n'abaisse jamais un rôle, et l'invitation n'est pas consommée.
        Some(existing) if existing >= invited_role => existing,
        Some(_) => {
            sqlx::query("UPDATE trip_members SET role = $3 WHERE trip_id = $1 AND user_id = $2")
                .bind(inv.trip_id)
                .bind(user.id)
                .bind(invited_role.as_str())
                .execute(&mut *tx)
                .await?;
            invited_role
        }
        None => {
            sqlx::query("INSERT INTO trip_members (trip_id, user_id, role) VALUES ($1, $2, $3)")
                .bind(inv.trip_id)
                .bind(user.id)
                .bind(invited_role.as_str())
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE invitations SET uses = uses + 1 WHERE id = $1")
                .bind(inv.id)
                .execute(&mut *tx)
                .await?;
            invited_role
        }
    };
    tx.commit().await?;
    state.realtime.changed(
        inv.trip_id,
        &[Topic::Trip, Topic::Members, Topic::Invitations],
        &user,
        // La personne qui rejoint n'a pas encore le voyage ouvert.
        &ClientId::default(),
    );

    let jar = if new_session.is_some() {
        let ua = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok());
        let token = session::create(&state.db, user.id, ua).await?;
        jar.add(session::cookie(&state.config, token))
    } else {
        jar
    };
    Ok((
        jar,
        Json(Accepted {
            trip_id: inv.trip_id,
            role,
            user: user.into(),
        }),
    ))
}
