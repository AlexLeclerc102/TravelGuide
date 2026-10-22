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
    pub trips: Vec<MembershipExport>,
    /// Lieux ajoutés par l'utilisateur (ils restent dans les voyages, sans auteur, après suppression du compte).
    pub pois_created: Vec<PoiExport>,
    /// Mes avis (supprimés avec le compte).
    pub reviews: Vec<ReviewExport>,
    /// Notes ajoutées aux lieux (elles restent, sans auteur, après suppression du compte).
    pub poi_notes: Vec<PoiNoteExport>,
    /// Liens ajoutés aux lieux (ils restent, sans auteur, après suppression du compte).
    pub poi_links: Vec<PoiLinkExport>,
    /// Photos ajoutées aux lieux (elles restent, sans auteur, après suppression du compte).
    pub poi_photos: Vec<PoiPhotoExport>,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PoiPhotoExport {
    pub trip_id: Uuid,
    pub poi_id: Uuid,
    pub poi_name: String,
    pub photo_id: Uuid,
    pub source_url: Option<String>,
    pub author_credit: Option<String>,
    pub via_assistant: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PoiNoteExport {
    pub trip_id: Uuid,
    pub poi_id: Uuid,
    pub poi_name: String,
    pub body: String,
    pub source_url: Option<String>,
    pub via_assistant: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PoiLinkExport {
    pub trip_id: Uuid,
    pub poi_id: Uuid,
    pub poi_name: String,
    pub url: String,
    pub title: String,
    pub via_assistant: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct ReviewExport {
    pub trip_id: Uuid,
    pub poi_id: Uuid,
    pub poi_name: String,
    pub rating: i16,
    pub comment: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PoiExport {
    pub poi_id: Uuid,
    pub trip_id: Uuid,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct MembershipExport {
    pub trip_id: Uuid,
    pub trip_name: String,
    pub role: String,
    #[serde(with = "time::serde::rfc3339")]
    pub joined_at: OffsetDateTime,
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
    let trips = sqlx::query_as(
        "SELECT t.id AS trip_id, t.name AS trip_name, m.role, m.joined_at
         FROM trip_members m JOIN trips t ON t.id = m.trip_id
         WHERE m.user_id = $1 ORDER BY m.joined_at",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    let pois_created = sqlx::query_as(
        "SELECT id AS poi_id, trip_id, name, created_at
         FROM pois WHERE created_by = $1 ORDER BY created_at",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    let reviews = sqlx::query_as(
        "SELECT p.trip_id, r.poi_id, p.name AS poi_name, r.rating, r.comment, r.updated_at
         FROM reviews r JOIN pois p ON p.id = r.poi_id
         WHERE r.user_id = $1 ORDER BY r.updated_at",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    let poi_notes = sqlx::query_as(
        "SELECT p.trip_id, n.poi_id, p.name AS poi_name, n.body, n.source_url, n.via_assistant,
                n.created_at
         FROM poi_notes n JOIN pois p ON p.id = n.poi_id
         WHERE n.author_id = $1 ORDER BY n.created_at",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    let poi_links = sqlx::query_as(
        "SELECT p.trip_id, l.poi_id, p.name AS poi_name, l.url, l.title, l.via_assistant,
                l.created_at
         FROM poi_links l JOIN pois p ON p.id = l.poi_id
         WHERE l.author_id = $1 ORDER BY l.created_at",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    let poi_photos = sqlx::query_as(
        "SELECT p.trip_id, ph.poi_id, p.name AS poi_name, ph.id AS photo_id, ph.source_url,
                ph.author_credit, ph.via_assistant, ph.created_at
         FROM poi_photos ph JOIN pois p ON p.id = ph.poi_id
         WHERE ph.author_id = $1 ORDER BY ph.created_at",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    Ok(AccountExport {
        exported_at: OffsetDateTime::now_utc(),
        user,
        sessions,
        trips,
        pois_created,
        reviews,
        poi_notes,
        poi_links,
        poi_photos,
    })
}

/// Supprime définitivement un compte et ses données personnelles.
///
/// Pour chaque voyage qu'il organise, l'organisation est transférée à l'éditeur
/// le plus ancien ; sans éditeur, le voyage est supprimé. Sessions, liens magiques
/// et adhésions sont supprimés en cascade.
pub async fn delete(db: &PgPool, user_id: Uuid) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    let organized: Vec<Uuid> = sqlx::query_scalar(
        "SELECT trip_id FROM trip_members WHERE user_id = $1 AND role = 'organizer' FOR UPDATE",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    for trip_id in organized {
        let successor: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM trip_members
             WHERE trip_id = $1 AND role = 'editor'
             ORDER BY joined_at, user_id LIMIT 1",
        )
        .bind(trip_id)
        .fetch_optional(&mut *tx)
        .await?;
        match successor {
            Some(successor) => {
                sqlx::query("DELETE FROM trip_members WHERE trip_id = $1 AND user_id = $2")
                    .bind(trip_id)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(
                    "UPDATE trip_members SET role = 'organizer' WHERE trip_id = $1 AND user_id = $2",
                )
                .bind(trip_id)
                .bind(successor)
                .execute(&mut *tx)
                .await?;
                tracing::info!(%trip_id, %successor, "organisation transférée (suppression de compte)");
            }
            None => {
                sqlx::query("DELETE FROM trips WHERE id = $1")
                    .bind(trip_id)
                    .execute(&mut *tx)
                    .await?;
                tracing::info!(%trip_id, "voyage supprimé avec le compte de son organisateur");
            }
        }
    }
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}
