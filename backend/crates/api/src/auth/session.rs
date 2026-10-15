//! Sessions stockées en base, référencées par un cookie HttpOnly.

use axum_extra::extract::cookie::{Cookie, SameSite};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::token;
use crate::config::Config;
use crate::users::UserRow;

pub const COOKIE_NAME: &str = "tg_session";
pub const SESSION_TTL: Duration = Duration::days(30);
/// Fréquence maximale de prolongation d'une session (évite une écriture par requête).
const TOUCH_INTERVAL: Duration = Duration::hours(1);

/// Crée une session et renvoie le jeton à placer dans le cookie.
pub async fn create(db: &PgPool, user_id: Uuid, user_agent: Option<&str>) -> sqlx::Result<String> {
    let (token, hash) = token::generate();
    let user_agent = user_agent.map(|ua| ua.chars().take(255).collect::<String>());
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, user_agent, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(days => $4))",
    )
    .bind(&hash)
    .bind(user_id)
    .bind(user_agent)
    .bind(SESSION_TTL.whole_days() as i32)
    .execute(db)
    .await?;
    Ok(token)
}

#[derive(sqlx::FromRow)]
struct SessionLookup {
    #[sqlx(flatten)]
    user: UserRow,
    last_seen_at: OffsetDateTime,
}

async fn lookup(db: &PgPool, hash: &[u8]) -> sqlx::Result<Option<SessionLookup>> {
    sqlx::query_as(&format!(
        "SELECT {cols}, s.last_seen_at
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = $1 AND s.expires_at > now()",
        cols = UserRow::COLUMNS_U
    ))
    .bind(hash)
    .fetch_optional(db)
    .await
}

/// Retrouve l'utilisateur d'un jeton de session valide, et prolonge la session.
pub async fn authenticate(db: &PgPool, token: &str) -> sqlx::Result<Option<UserRow>> {
    let hash = token::hash(token);
    let Some(found) = lookup(db, &hash).await? else {
        return Ok(None);
    };
    if OffsetDateTime::now_utc() - found.last_seen_at > TOUCH_INTERVAL {
        sqlx::query(
            "UPDATE sessions SET last_seen_at = now(),
                 expires_at = now() + make_interval(days => $2)
             WHERE token_hash = $1",
        )
        .bind(&hash)
        .bind(SESSION_TTL.whole_days() as i32)
        .execute(db)
        .await?;
    }
    Ok(Some(found.user))
}

/// Comme [`authenticate`], sans prolonger la session : vérifications périodiques qui ne
/// sont pas une activité de l'utilisateur (ex. connexion temps réel restée ouverte).
pub async fn peek(db: &PgPool, token: &str) -> sqlx::Result<Option<UserRow>> {
    Ok(lookup(db, &token::hash(token))
        .await?
        .map(|found| found.user))
}

pub async fn delete(db: &PgPool, token: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token::hash(token))
        .execute(db)
        .await?;
    Ok(())
}

/// Supprime toutes les sessions d'un utilisateur sauf, éventuellement, la session courante.
pub async fn delete_others(db: &PgPool, user_id: Uuid, keep_token: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND token_hash <> $2")
        .bind(user_id)
        .bind(token::hash(keep_token))
        .execute(db)
        .await?;
    Ok(())
}

pub fn cookie(config: &Config, token: String) -> Cookie<'static> {
    Cookie::build((COOKIE_NAME, token))
        .http_only(true)
        .secure(config.cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(SESSION_TTL)
        .build()
}

pub fn removal_cookie(config: &Config) -> Cookie<'static> {
    let mut c = cookie(config, String::new());
    c.make_removal();
    c
}
