//! Inscription, connexion (mot de passe ou lien magique), déconnexion.

use std::time::Duration;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::Json;
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::{password, session, token, CurrentUser};
use crate::client_ip::ClientIp;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::mail::Email;
use crate::state::AppState;
use crate::users::{self, User, UserRow};
use crate::validate;

const MAGIC_LINK_TTL_MINUTES: i32 = 15;
const WINDOW_15_MIN: Duration = Duration::from_secs(15 * 60);
const WINDOW_1_H: Duration = Duration::from_secs(60 * 60);

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(register))
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(request_magic_link))
        .routes(routes!(verify_magic_link))
}

#[derive(Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct MagicLinkRequest {
    pub email: String,
    /// Page interne où revenir après connexion (ex. `/invite/<jeton>`).
    pub next: Option<String>,
}

/// Chemin interne sûr pour une redirection (pas de domaine externe, caractères limités).
fn safe_next(next: Option<&str>) -> Option<&str> {
    next.filter(|n| {
        n.len() <= 200
            && n.starts_with('/')
            && !n.starts_with("//")
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_'))
    })
}

#[derive(Deserialize, ToSchema)]
pub struct MagicLinkVerify {
    pub token: String,
}

#[derive(Serialize, ToSchema)]
pub struct Accepted {
    pub message: &'static str,
}

fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
}

/// Ouvre une session et place le cookie.
async fn start_session(
    state: &AppState,
    jar: CookieJar,
    headers: &HeaderMap,
    user: UserRow,
) -> AppResult<(CookieJar, Json<User>)> {
    let token = session::create(&state.db, user.id, user_agent(headers)).await?;
    Ok((
        jar.add(session::cookie(&state.config, token)),
        Json(user.into()),
    ))
}

/// Crée un compte avec mot de passe et ouvre une session.
#[utoipa::path(post, path = "/auth/register", tag = "authentification",
    request_body = RegisterRequest,
    responses(
        (status = 201, body = User, description = "Compte créé, cookie de session posé"),
        (status = 400, body = ErrorBody), (status = 409, body = ErrorBody),
        (status = 429, body = ErrorBody)))]
async fn register(
    State(state): State<AppState>,
    ip: ClientIp,
    jar: CookieJar,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> AppResult<(StatusCode, CookieJar, Json<User>)> {
    state
        .rate_limiter
        .check(&format!("register:{}", ip.key()), 20, WINDOW_1_H)?;
    let email = validate::email(&req.email)?;
    let display_name = validate::display_name(&req.display_name)?;
    validate::password(&req.password)?;

    let hash = password::hash(req.password).await?;
    let plan = if state.config.grants_organizer(&email) {
        "organizer"
    } else {
        "free"
    };
    let user = users::create(&state.db, &email, &display_name, Some(&hash), plan)
        .await?
        .ok_or_else(|| AppError::Conflict("un compte existe déjà pour cette adresse".into()))?;
    tracing::info!(user_id = %user.id, "compte créé");
    let (jar, body) = start_session(&state, jar, &headers, user).await?;
    Ok((StatusCode::CREATED, jar, body))
}

/// Connexion par email et mot de passe.
#[utoipa::path(post, path = "/auth/login", tag = "authentification",
    request_body = LoginRequest,
    responses(
        (status = 200, body = User, description = "Connecté, cookie de session posé"),
        (status = 401, body = ErrorBody), (status = 429, body = ErrorBody)))]
async fn login(
    State(state): State<AppState>,
    ip: ClientIp,
    jar: CookieJar,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> AppResult<(CookieJar, Json<User>)> {
    let email = req.email.trim().to_lowercase();
    state
        .rate_limiter
        .check(&format!("login-ip:{}", ip.key()), 50, WINDOW_15_MIN)?;
    state
        .rate_limiter
        .check(&format!("login:{email}"), 10, WINDOW_15_MIN)?;

    let user = users::find_by_email(&state.db, &email).await?;
    let hash = user.as_ref().and_then(|u| u.password_hash.clone());
    // Toujours vérifier (empreinte factice si besoin) : même durée de réponse.
    if !password::verify(req.password, hash).await? {
        return Err(AppError::Unauthorized);
    }
    let mut user = user.ok_or(AppError::Unauthorized)?;
    if user.plan != "organizer" && state.config.grants_organizer(&user.email) {
        user = users::set_plan(&state.db, user.id, "organizer").await?;
    }
    start_session(&state, jar, &headers, user).await
}

/// Ferme la session courante.
#[utoipa::path(post, path = "/auth/logout", tag = "authentification",
    responses((status = 204, description = "Déconnecté")))]
async fn logout(
    State(state): State<AppState>,
    current: CurrentUser,
    jar: CookieJar,
) -> AppResult<(StatusCode, CookieJar)> {
    session::delete(&state.db, &current.session_token).await?;
    Ok((
        StatusCode::NO_CONTENT,
        jar.add(session::removal_cookie(&state.config)),
    ))
}

/// Envoie un lien de connexion par email.
///
/// La réponse est identique que le compte existe ou non (pas d'énumération des comptes).
#[utoipa::path(post, path = "/auth/magic-link", tag = "authentification",
    request_body = MagicLinkRequest,
    responses((status = 202, body = Accepted), (status = 400, body = ErrorBody),
        (status = 429, body = ErrorBody)))]
async fn request_magic_link(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(req): Json<MagicLinkRequest>,
) -> AppResult<(StatusCode, Json<Accepted>)> {
    let email = validate::email(&req.email)?;
    state
        .rate_limiter
        .check(&format!("magic-ip:{}", ip.key()), 30, WINDOW_1_H)?;
    state
        .rate_limiter
        .check(&format!("magic:{email}"), 5, WINDOW_15_MIN)?;

    if let Some(user) = users::find_by_email(&state.db, &email).await? {
        let (token, hash) = token::generate();
        sqlx::query(
            "INSERT INTO magic_links (token_hash, user_id, expires_at)
             VALUES ($1, $2, now() + make_interval(mins => $3))",
        )
        .bind(&hash)
        .bind(user.id)
        .bind(MAGIC_LINK_TTL_MINUTES)
        .execute(&state.db)
        .await?;

        let mut link = format!("{}/auth/magic?token={token}", state.config.public_origin);
        if let Some(next) = safe_next(req.next.as_deref()) {
            link.push_str("&next=");
            link.push_str(next);
        }
        let email = Email {
            to: user.email,
            subject: "Votre lien de connexion TravelGuide".into(),
            text: format!(
                "Bonjour {name},\n\nCliquez sur ce lien pour vous connecter :\n{link}\n\n\
                 Il expire dans {MAGIC_LINK_TTL_MINUTES} minutes et ne fonctionne qu'une fois.\n\
                 Si vous n'êtes pas à l'origine de cette demande, ignorez ce message.\n",
                name = user.display_name
            ),
        };
        // Envoi en arrière-plan : la durée de réponse ne dépend pas de l'existence du compte.
        let mailer = state.mailer.clone();
        tokio::spawn(async move {
            if let Err(e) = mailer.send(email).await {
                tracing::error!(error = ?e, "échec d'envoi du lien magique");
            }
        });
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(Accepted {
            message: "si un compte existe pour cette adresse, un lien de connexion a été envoyé",
        }),
    ))
}

/// Échange un jeton de lien magique (usage unique, 15 min) contre une session.
#[utoipa::path(post, path = "/auth/magic-link/verify", tag = "authentification",
    request_body = MagicLinkVerify,
    responses((status = 200, body = User), (status = 400, body = ErrorBody)))]
async fn verify_magic_link(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(req): Json<MagicLinkVerify>,
) -> AppResult<(CookieJar, Json<User>)> {
    let invalid = || AppError::BadRequest("lien invalide ou expiré".into());
    if req.token.len() > 128 {
        return Err(invalid());
    }
    let mut tx = state.db.begin().await?;
    let user_id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE magic_links SET used_at = now()
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
         RETURNING user_id",
    )
    .bind(token::hash(&req.token))
    .fetch_optional(&mut *tx)
    .await?;
    let user_id = user_id.ok_or_else(invalid)?;
    // Première preuve de possession de l'email : on ferme les sessions ouvertes avant
    // (un compte invité a pu être créé avec cet email par quelqu'un d'autre).
    let was_verified: bool =
        sqlx::query_scalar("SELECT email_verified_at IS NOT NULL FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
    if !was_verified {
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }
    let user: UserRow = sqlx::query_as(&format!(
        "UPDATE users SET email_verified_at = coalesce(email_verified_at, now())
         WHERE id = $1 RETURNING {}",
        UserRow::COLUMNS
    ))
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    start_session(&state, jar, &headers, user).await
}

#[cfg(test)]
mod tests {
    use super::safe_next;

    #[test]
    fn next_must_be_internal_path() {
        assert_eq!(safe_next(Some("/invite/abc-_1")), Some("/invite/abc-_1"));
        for bad in [
            "//evil.example",
            "https://evil.example",
            "/a?b=c",
            "/a b",
            "invite",
        ] {
            assert_eq!(safe_next(Some(bad)), None, "{bad}");
        }
    }
}
