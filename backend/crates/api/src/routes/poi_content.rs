//! Notes et liens des lieux : lecture pour tous les membres, écriture pour les éditeurs.
//!
//! Chaque élément garde son auteur et indique s'il a été ajouté par l'assistant
//! (`via_assistant`). Les photos sont dans [`super::photos`].

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::assistant::ViaAssistant;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::pois;
use crate::realtime::{ClientId, Topic};
use crate::state::AppState;
use crate::trips::{self, Role};
use crate::validate;

/// Nombre maximal de notes par lieu.
pub const MAX_NOTES_PER_POI: i64 = 100;
/// Nombre maximal de liens par lieu.
pub const MAX_LINKS_PER_POI: i64 = 50;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_notes, create_note))
        .routes(routes!(update_note, delete_note))
        .routes(routes!(list_links, create_link))
        .routes(routes!(delete_link))
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PoiNote {
    pub id: Uuid,
    pub poi_id: Uuid,
    pub body: String,
    /// Page d'où vient l'information.
    pub source_url: Option<String>,
    /// Absent si le compte de l'auteur a été supprimé.
    pub author_id: Option<Uuid>,
    pub author_name: Option<String>,
    pub via_assistant: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Deserialize, ToSchema)]
pub struct NoteInput {
    pub body: String,
    /// Page d'où vient l'information (http(s)) ; vide ou absent : aucune.
    pub source_url: Option<String>,
}

/// Nature d'un lien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// Site officiel du lieu.
    Official,
    /// Réservation (billets, hébergement).
    Booking,
    /// Article, blog, guide.
    Article,
    Video,
    Map,
    #[default]
    Other,
}

impl LinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Booking => "booking",
            Self::Article => "article",
            Self::Video => "video",
            Self::Map => "map",
            Self::Other => "other",
        }
    }
}

impl std::str::FromStr for LinkKind {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, AppError> {
        Ok(match s {
            "official" => Self::Official,
            "booking" => Self::Booking,
            "article" => Self::Article,
            "video" => Self::Video,
            "map" => Self::Map,
            "other" => Self::Other,
            _ => return Err(anyhow::anyhow!("type de lien inconnu en base : {s}").into()),
        })
    }
}

#[derive(Serialize, ToSchema)]
pub struct PoiLink {
    pub id: Uuid,
    pub poi_id: Uuid,
    pub url: String,
    pub title: String,
    pub kind: LinkKind,
    pub author_id: Option<Uuid>,
    pub author_name: Option<String>,
    pub via_assistant: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct LinkRow {
    id: Uuid,
    poi_id: Uuid,
    url: String,
    title: String,
    kind: String,
    author_id: Option<Uuid>,
    author_name: Option<String>,
    via_assistant: bool,
    created_at: OffsetDateTime,
}

impl TryFrom<LinkRow> for PoiLink {
    type Error = AppError;

    fn try_from(r: LinkRow) -> AppResult<Self> {
        Ok(Self {
            id: r.id,
            poi_id: r.poi_id,
            url: r.url,
            title: r.title,
            kind: r.kind.parse()?,
            author_id: r.author_id,
            author_name: r.author_name,
            via_assistant: r.via_assistant,
            created_at: r.created_at,
        })
    }
}

#[derive(Deserialize, ToSchema)]
pub struct LinkInput {
    /// Adresse https.
    pub url: String,
    /// Défaut : le nom de domaine.
    pub title: Option<String>,
    /// Défaut : `other`.
    pub kind: Option<LinkKind>,
}

const NOTE_SELECT: &str = "
    SELECT n.id, n.poi_id, n.body, n.source_url, n.author_id, u.display_name AS author_name,
           n.via_assistant, n.created_at, n.updated_at
    FROM poi_notes n LEFT JOIN users u ON u.id = n.author_id";

const LINK_SELECT: &str = "
    SELECT l.id, l.poi_id, l.url, l.title, l.kind, l.author_id, u.display_name AS author_name,
           l.via_assistant, l.created_at
    FROM poi_links l LEFT JOIN users u ON u.id = l.author_id";

async fn note(state: &AppState, poi_id: Uuid, note_id: Uuid) -> AppResult<PoiNote> {
    sqlx::query_as(&format!("{NOTE_SELECT} WHERE n.id = $1 AND n.poi_id = $2"))
        .bind(note_id)
        .bind(poi_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// Domaine d'une adresse https (titre par défaut d'un lien).
fn host(url: &str) -> &str {
    let rest = url.strip_prefix("https://").unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit('@').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host)
}

/// Notes d'un lieu, des plus anciennes aux plus récentes.
#[utoipa::path(get, path = "/trips/{trip_id}/pois/{poi_id}/notes", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    responses((status = 200, body = Vec<PoiNote>), (status = 404, body = ErrorBody)))]
async fn list_notes(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<PoiNote>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    pois::ensure_in_trip(&state.db, trip_id, poi_id).await?;
    let notes = sqlx::query_as(&format!(
        "{NOTE_SELECT} WHERE n.poi_id = $1 ORDER BY n.created_at, n.id"
    ))
    .bind(poi_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(notes))
}

/// Ajoute une note à un lieu (éditeur).
#[utoipa::path(post, path = "/trips/{trip_id}/pois/{poi_id}/notes", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)), request_body = NoteInput,
    responses((status = 201, body = PoiNote), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn create_note(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    via: ViaAssistant,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<NoteInput>,
) -> AppResult<(StatusCode, Json<PoiNote>)> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    let body = validate::note(&req.body)?;
    let source_url = validate::source_url(req.source_url.as_deref())?;

    let mut tx = state.db.begin().await?;
    pois::lock_in_trip(&mut *tx, trip_id, poi_id).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM poi_notes WHERE poi_id = $1")
        .bind(poi_id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_NOTES_PER_POI {
        return Err(AppError::BadRequest(format!(
            "un lieu ne peut pas avoir plus de {MAX_NOTES_PER_POI} notes"
        )));
    }
    let note_id: Uuid = sqlx::query_scalar(
        "INSERT INTO poi_notes (poi_id, author_id, via_assistant, body, source_url)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(poi_id)
    .bind(current.user.id)
    .bind(via.0)
    .bind(&body)
    .bind(&source_url)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    state
        .realtime
        .changed(trip_id, &[Topic::PoiContent], &current.user, &client);
    Ok((
        StatusCode::CREATED,
        Json(note(&state, poi_id, note_id).await?),
    ))
}

/// Modifie le texte ou la source d'une note (éditeur).
#[utoipa::path(put, path = "/trips/{trip_id}/pois/{poi_id}/notes/{note_id}", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path), ("note_id" = Uuid, Path)),
    request_body = NoteInput,
    responses((status = 200, body = PoiNote), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn update_note(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id, note_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(req): Json<NoteInput>,
) -> AppResult<Json<PoiNote>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    pois::ensure_in_trip(&state.db, trip_id, poi_id).await?;
    let body = validate::note(&req.body)?;
    let source_url = validate::source_url(req.source_url.as_deref())?;
    let updated = sqlx::query(
        "UPDATE poi_notes SET body = $3, source_url = $4 WHERE id = $1 AND poi_id = $2",
    )
    .bind(note_id)
    .bind(poi_id)
    .bind(&body)
    .bind(&source_url)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    state
        .realtime
        .changed(trip_id, &[Topic::PoiContent], &current.user, &client);
    Ok(Json(note(&state, poi_id, note_id).await?))
}

/// Supprime une note (éditeur).
#[utoipa::path(delete, path = "/trips/{trip_id}/pois/{poi_id}/notes/{note_id}", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path), ("note_id" = Uuid, Path)),
    responses((status = 204), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn delete_note(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id, note_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    pois::ensure_in_trip(&state.db, trip_id, poi_id).await?;
    let deleted = sqlx::query("DELETE FROM poi_notes WHERE id = $1 AND poi_id = $2")
        .bind(note_id)
        .bind(poi_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    state
        .realtime
        .changed(trip_id, &[Topic::PoiContent], &current.user, &client);
    Ok(StatusCode::NO_CONTENT)
}

/// Liens d'un lieu, des plus anciens aux plus récents.
#[utoipa::path(get, path = "/trips/{trip_id}/pois/{poi_id}/links", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    responses((status = 200, body = Vec<PoiLink>), (status = 404, body = ErrorBody)))]
async fn list_links(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<PoiLink>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    pois::ensure_in_trip(&state.db, trip_id, poi_id).await?;
    let rows: Vec<LinkRow> = sqlx::query_as(&format!(
        "{LINK_SELECT} WHERE l.poi_id = $1 ORDER BY l.created_at, l.id"
    ))
    .bind(poi_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(PoiLink::try_from)
            .collect::<AppResult<_>>()?,
    ))
}

/// Ajoute un lien à un lieu (éditeur) ; 409 si le lien y est déjà.
#[utoipa::path(post, path = "/trips/{trip_id}/pois/{poi_id}/links", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)), request_body = LinkInput,
    responses((status = 201, body = PoiLink), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody, description = "Lien déjà présent")))]
async fn create_link(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    via: ViaAssistant,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<LinkInput>,
) -> AppResult<(StatusCode, Json<PoiLink>)> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    let url = validate::link_url(&req.url)?;
    let title = match req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(t) => validate::link_title(t)?,
        None => validate::link_title(host(&url))?,
    };
    let kind = req.kind.unwrap_or_default();

    let mut tx = state.db.begin().await?;
    pois::lock_in_trip(&mut *tx, trip_id, poi_id).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM poi_links WHERE poi_id = $1")
        .bind(poi_id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_LINKS_PER_POI {
        return Err(AppError::BadRequest(format!(
            "un lieu ne peut pas avoir plus de {MAX_LINKS_PER_POI} liens"
        )));
    }
    let link_id: Uuid = sqlx::query_scalar(
        "INSERT INTO poi_links (poi_id, author_id, via_assistant, url, title, kind)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(poi_id)
    .bind(current.user.id)
    .bind(via.0)
    .bind(&url)
    .bind(&title)
    .bind(kind.as_str())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.constraint() == Some("poi_links_poi_url_unique") => {
            AppError::Conflict("ce lien est déjà associé au lieu".into())
        }
        _ => e.into(),
    })?;
    let row: LinkRow = sqlx::query_as(&format!("{LINK_SELECT} WHERE l.id = $1"))
        .bind(link_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    state
        .realtime
        .changed(trip_id, &[Topic::PoiContent], &current.user, &client);
    Ok((StatusCode::CREATED, Json(row.try_into()?)))
}

/// Retire un lien (éditeur).
#[utoipa::path(delete, path = "/trips/{trip_id}/pois/{poi_id}/links/{link_id}", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path), ("link_id" = Uuid, Path)),
    responses((status = 204), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn delete_link(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id, link_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    pois::ensure_in_trip(&state.db, trip_id, poi_id).await?;
    let deleted = sqlx::query("DELETE FROM poi_links WHERE id = $1 AND poi_id = $2")
        .bind(link_id)
        .bind(poi_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    state
        .realtime
        .changed(trip_id, &[Topic::PoiContent], &current.user, &client);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_title_is_the_host() {
        assert_eq!(host("https://www.arenal.net/fr/visite?x=1"), "arenal.net");
        assert_eq!(host("https://user@tours.cr#a"), "tours.cr");
        assert_eq!(host("https://example.org"), "example.org");
    }
}
