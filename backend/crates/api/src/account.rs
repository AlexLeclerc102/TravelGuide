//! Opérations sur le compte (RGPD : export et suppression).

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::users::User;

#[derive(Serialize, ToSchema)]
pub struct AccountExport {
    #[serde(with = "time::serde::rfc3339")]
    pub exported_at: OffsetDateTime,
    pub user: User,
    pub sessions: Vec<SessionExport>,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct SessionExport {
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    pub user_agent: Option<String>,
}

pub async fn export(db: &PgPool, user: User) -> sqlx::Result<AccountExport> {
    let sessions = sqlx::query_as(
        "SELECT created_at, last_seen_at, expires_at, user_agent
         FROM sessions WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    Ok(AccountExport {
        exported_at: OffsetDateTime::now_utc(),
        user,
        sessions,
    })
}

/// Supprime définitivement un compte et ses données personnelles.
///
/// Les sessions et liens magiques sont supprimés en cascade. Les règles propres
/// aux voyages (transfert à l'éditeur le plus ancien, etc.) s'ajoutent ici.
pub async fn delete(db: &PgPool, user_id: Uuid) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}
