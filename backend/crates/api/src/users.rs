//! Accès aux comptes utilisateurs.

use serde::Serialize;
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub password_hash: Option<String>,
    pub plan: String,
    pub created_at: OffsetDateTime,
}

impl UserRow {
    /// Colonnes à sélectionner (l'email `citext` est converti en `text`).
    pub const COLUMNS: &'static str =
        "id, email::text AS email, display_name, password_hash, plan, created_at";
    /// Mêmes colonnes, préfixées par l'alias `u`.
    pub const COLUMNS_U: &'static str =
        "u.id, u.email::text AS email, u.display_name, u.password_hash, u.plan, u.created_at";
}

/// Représentation publique d'un compte (jamais l'empreinte du mot de passe).
#[derive(Debug, Serialize, ToSchema)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    /// `free` ou `organizer` (abonnement simulé pour le MVP).
    pub plan: String,
    /// `false` pour un compte créé par invitation ou lien magique.
    pub has_password: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl From<UserRow> for User {
    fn from(u: UserRow) -> Self {
        Self {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            plan: u.plan,
            has_password: u.password_hash.is_some(),
            created_at: u.created_at,
        }
    }
}

pub async fn find_by_email(db: impl PgExecutor<'_>, email: &str) -> sqlx::Result<Option<UserRow>> {
    sqlx::query_as(&format!(
        "SELECT {} FROM users WHERE email = $1::citext",
        UserRow::COLUMNS
    ))
    .bind(email)
    .fetch_optional(db)
    .await
}

/// Crée un compte. Renvoie `None` si l'email est déjà utilisé.
pub async fn create(
    db: &PgPool,
    email: &str,
    display_name: &str,
    password_hash: Option<&str>,
) -> sqlx::Result<Option<UserRow>> {
    sqlx::query_as(&format!(
        "INSERT INTO users (email, display_name, password_hash)
         VALUES ($1::citext, $2, $3)
         ON CONFLICT (email) DO NOTHING
         RETURNING {}",
        UserRow::COLUMNS
    ))
    .bind(email)
    .bind(display_name)
    .bind(password_hash)
    .fetch_optional(db)
    .await
}
