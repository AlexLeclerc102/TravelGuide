//! Photos des lieux : lecture pour tous les membres, ajout et retrait par les éditeurs.
//!
//! Une photo est envoyée par un membre (multipart) ou importée depuis une adresse web
//! (assistant, ou membre qui colle un lien) : téléchargement protégé contre la SSRF, puis
//! même traitement (voir [`crate::photos`]). Les fichiers ne sont servis qu'aux membres.

use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
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
use crate::photos::{self, PhotoError, MAX_INPUT_BYTES, MAX_PER_POI};
use crate::pois;
use crate::realtime::{ClientId, Topic};
use crate::safe_fetch::FetchError;
use crate::state::AppState;
use crate::trips::{self, Role};
use crate::validate;

/// Photos ajoutées par utilisateur et par minute (traitement coûteux, téléchargements).
const PER_USER_PER_MINUTE: u32 = 20;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_photos, upload_photo))
        .routes(routes!(import_photo))
        .routes(routes!(delete_photo))
        .routes(routes!(photo_image))
        .routes(routes!(photo_thumb))
        // Corps d'un envoi : l'image et quelques champs texte.
        .layer(DefaultBodyLimit::max(MAX_INPUT_BYTES + 64 * 1024))
}

#[derive(Serialize, ToSchema)]
pub struct PoiPhoto {
    pub id: Uuid,
    pub poi_id: Uuid,
    pub width: i32,
    pub height: i32,
    /// Image (au plus 1600 px), réservée aux membres du voyage.
    pub image_url: String,
    /// Vignette (au plus 400 px).
    pub thumb_url: String,
    /// Image d'origine, si elle a été importée depuis le web.
    pub source_url: Option<String>,
    /// Page qui présente l'image (à créditer).
    pub source_page: Option<String>,
    pub author_credit: Option<String>,
    pub license: Option<String>,
    pub author_id: Option<Uuid>,
    pub author_name: Option<String>,
    pub via_assistant: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct PhotoRow {
    id: Uuid,
    poi_id: Uuid,
    width: i32,
    height: i32,
    source_url: Option<String>,
    source_page: Option<String>,
    author_credit: Option<String>,
    license: Option<String>,
    author_id: Option<Uuid>,
    author_name: Option<String>,
    via_assistant: bool,
    created_at: OffsetDateTime,
}

impl PhotoRow {
    fn into_photo(self, trip_id: Uuid) -> PoiPhoto {
        let base = format!(
            "/api/v1/trips/{trip_id}/pois/{}/photos/{}",
            self.poi_id, self.id
        );
        PoiPhoto {
            id: self.id,
            poi_id: self.poi_id,
            width: self.width,
            height: self.height,
            image_url: format!("{base}/image"),
            thumb_url: format!("{base}/thumb"),
            source_url: self.source_url,
            source_page: self.source_page,
            author_credit: self.author_credit,
            license: self.license,
            author_id: self.author_id,
            author_name: self.author_name,
            via_assistant: self.via_assistant,
            created_at: self.created_at,
        }
    }
}

const PHOTO_SELECT: &str = "
    SELECT ph.id, ph.poi_id, ph.width, ph.height, ph.source_url, ph.source_page,
           ph.author_credit, ph.license, ph.author_id, u.display_name AS author_name,
           ph.via_assistant, ph.created_at
    FROM poi_photos ph LEFT JOIN users u ON u.id = ph.author_id";

/// Crédit d'une photo, tel qu'affiché à côté d'elle.
#[derive(Default, Deserialize, ToSchema)]
pub struct PhotoCredit {
    /// Auteur à créditer.
    pub author_credit: Option<String>,
    /// Licence (ex. `CC BY-SA 4.0`).
    pub license: Option<String>,
    /// Page qui présente l'image (http(s)).
    pub source_page: Option<String>,
}

struct CleanCredit {
    author_credit: Option<String>,
    license: Option<String>,
    source_page: Option<String>,
}

fn one_line(raw: Option<&str>, max: usize, what: &str) -> AppResult<Option<String>> {
    let Some(v) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if v.chars().count() > max || v.chars().any(char::is_control) {
        return Err(AppError::BadRequest(format!(
            "{what} tient sur une ligne de {max} caractères au plus"
        )));
    }
    Ok(Some(v.to_owned()))
}

impl PhotoCredit {
    fn clean(&self) -> AppResult<CleanCredit> {
        Ok(CleanCredit {
            author_credit: one_line(self.author_credit.as_deref(), 200, "le crédit")?,
            license: one_line(self.license.as_deref(), 100, "la licence")?,
            source_page: validate::source_url(self.source_page.as_deref())?,
        })
    }
}

/// Photo à importer depuis le web.
#[derive(Deserialize, ToSchema)]
pub struct ImportPhoto {
    /// Adresse https de l'image.
    pub image_url: String,
    #[serde(flatten)]
    pub credit: PhotoCredit,
}

/// Envoi d'une photo (multipart) : documenté pour l'OpenAPI.
#[derive(ToSchema)]
#[allow(dead_code)]
struct UploadPhoto {
    /// Image JPEG, PNG, WebP ou GIF (10 Mo au plus).
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
    author_credit: Option<String>,
    license: Option<String>,
    source_page: Option<String>,
}

impl From<PhotoError> for AppError {
    fn from(e: PhotoError) -> Self {
        Self::BadRequest(e.to_string())
    }
}

fn fetch_error(e: FetchError) -> AppError {
    match e {
        FetchError::Refused(_) | FetchError::TooLarge(_) => {
            AppError::BadRequest(format!("image refusée : {e}"))
        }
        FetchError::Status(status) => AppError::BadRequest(format!(
            "l'image n'a pas pu être téléchargée (réponse {})",
            status.as_u16()
        )),
        FetchError::Unavailable(source) => {
            tracing::warn!(error = ?source, "téléchargement d'image en échec");
            AppError::ServiceUnavailable("l'image n'a pas pu être téléchargée".into())
        }
    }
}

/// Vérifications communes avant de recevoir ou télécharger une image.
async fn before_add(
    state: &AppState,
    current: &CurrentUser,
    trip_id: Uuid,
    poi_id: Uuid,
) -> AppResult<()> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    pois::ensure_in_trip(&state.db, trip_id, poi_id).await?;
    state.rate_limiter.check(
        &format!("photos:{}", current.user.id),
        PER_USER_PER_MINUTE,
        Duration::from_secs(60),
    )?;
    // Vérifiée de nouveau à l'enregistrement : ici, pour ne pas traiter l'image pour rien.
    check_room(&state.db, poi_id).await
}

async fn check_room(db: impl sqlx::PgExecutor<'_>, poi_id: Uuid) -> AppResult<()> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM poi_photos WHERE poi_id = $1")
        .bind(poi_id)
        .fetch_one(db)
        .await?;
    if count >= MAX_PER_POI {
        return Err(AppError::BadRequest(format!(
            "un lieu ne peut pas avoir plus de {MAX_PER_POI} photos"
        )));
    }
    Ok(())
}

/// Traite l'image, stocke ses fichiers puis enregistre la photo.
#[allow(clippy::too_many_arguments)]
async fn save(
    state: &AppState,
    current: &CurrentUser,
    client: &ClientId,
    via: ViaAssistant,
    trip_id: Uuid,
    poi_id: Uuid,
    bytes: Vec<u8>,
    credit: CleanCredit,
    source_url: Option<String>,
) -> AppResult<PoiPhoto> {
    let processed = tokio::task::spawn_blocking(move || photos::process(&bytes))
        .await
        .map_err(|e| AppError::Internal(e.into()))??;
    let id = Uuid::new_v4();
    let size = processed.full.len() as i32;
    let store = &state.photos.store;
    store.put(&photos::full_key(id), processed.full).await?;
    store.put(&photos::thumb_key(id), processed.thumb).await?;

    let insert = async {
        let mut tx = state.db.begin().await?;
        pois::lock_in_trip(&mut *tx, trip_id, poi_id).await?;
        check_room(&mut *tx, poi_id).await?;
        sqlx::query(
            "INSERT INTO poi_photos (id, poi_id, author_id, via_assistant, width, height, mime,
                                     bytes, source_url, source_page, author_credit, license)
             VALUES ($1, $2, $3, $4, $5, $6, 'image/jpeg', $7, $8, $9, $10, $11)",
        )
        .bind(id)
        .bind(poi_id)
        .bind(current.user.id)
        .bind(via.0)
        .bind(processed.width as i32)
        .bind(processed.height as i32)
        .bind(size)
        .bind(&source_url)
        .bind(&credit.source_page)
        .bind(&credit.author_credit)
        .bind(&credit.license)
        .execute(&mut *tx)
        .await?;
        let row: PhotoRow = sqlx::query_as(&format!("{PHOTO_SELECT} WHERE ph.id = $1"))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        AppResult::Ok(row)
    };
    let row = match insert.await {
        Ok(row) => row,
        Err(e) => {
            // Rien n'a été enregistré : les fichiers ne doivent pas rester.
            let _ = store.delete(&photos::full_key(id)).await;
            let _ = store.delete(&photos::thumb_key(id)).await;
            return Err(e);
        }
    };
    tracing::info!(%trip_id, %poi_id, photo_id = %id, "photo ajoutée");
    state
        .realtime
        .changed(trip_id, &[Topic::PoiContent], &current.user, client);
    Ok(row.into_photo(trip_id))
}

/// Photos d'un lieu, des plus anciennes aux plus récentes.
#[utoipa::path(get, path = "/trips/{trip_id}/pois/{poi_id}/photos", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    responses((status = 200, body = Vec<PoiPhoto>), (status = 404, body = ErrorBody)))]
async fn list_photos(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<PoiPhoto>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    pois::ensure_in_trip(&state.db, trip_id, poi_id).await?;
    let rows: Vec<PhotoRow> = sqlx::query_as(&format!(
        "{PHOTO_SELECT} WHERE ph.poi_id = $1 ORDER BY ph.created_at, ph.id"
    ))
    .bind(poi_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter().map(|r| r.into_photo(trip_id)).collect(),
    ))
}

/// Envoie une photo (éditeur) : champ `file`, et facultativement `author_credit`,
/// `license`, `source_page`. Ré-encodée en JPEG sans métadonnées (ni position GPS).
#[utoipa::path(post, path = "/trips/{trip_id}/pois/{poi_id}/photos", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    request_body(content = UploadPhoto, content_type = "multipart/form-data"),
    responses((status = 201, body = PoiPhoto), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
        (status = 429, body = ErrorBody)))]
async fn upload_photo(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    via: ViaAssistant,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
    mut multipart: Multipart,
) -> AppResult<(StatusCode, Json<PoiPhoto>)> {
    before_add(&state, &current, trip_id, poi_id).await?;
    let invalid = |e: axum::extract::multipart::MultipartError| {
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::BadRequest(format!(
                "image trop volumineuse ({} Mo au plus)",
                MAX_INPUT_BYTES / 1_000_000
            ))
        } else {
            AppError::BadRequest(format!("envoi invalide : {}", e.body_text()))
        }
    };
    let mut file = None;
    let mut credit = PhotoCredit::default();
    while let Some(field) = multipart.next_field().await.map_err(invalid)? {
        match field.name() {
            Some("file") => file = Some(field.bytes().await.map_err(invalid)?.to_vec()),
            Some("author_credit") => {
                credit.author_credit = Some(field.text().await.map_err(invalid)?)
            }
            Some("license") => credit.license = Some(field.text().await.map_err(invalid)?),
            Some("source_page") => credit.source_page = Some(field.text().await.map_err(invalid)?),
            _ => {}
        }
    }
    let bytes = file
        .filter(|f| !f.is_empty())
        .ok_or_else(|| AppError::BadRequest("aucune image envoyée (champ `file`)".into()))?;
    let credit = credit.clean()?;
    let photo = save(
        &state, &current, &client, via, trip_id, poi_id, bytes, credit, None,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(photo)))
}

/// Importe une photo depuis une adresse https (éditeur). Préférez les images de Wikimedia
/// Commons ou du site officiel du lieu, et indiquez leur crédit.
#[utoipa::path(post, path = "/trips/{trip_id}/pois/{poi_id}/photos/import", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)), request_body = ImportPhoto,
    responses((status = 201, body = PoiPhoto), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
        (status = 429, body = ErrorBody), (status = 503, body = ErrorBody)))]
async fn import_photo(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    via: ViaAssistant,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ImportPhoto>,
) -> AppResult<(StatusCode, Json<PoiPhoto>)> {
    before_add(&state, &current, trip_id, poi_id).await?;
    let credit = req.credit.clean()?;
    let image_url = validate::link_url(&req.image_url)?;
    let fetched = state
        .photos
        .web
        .get(&image_url, MAX_INPUT_BYTES)
        .await
        .map_err(fetch_error)?;
    let photo = save(
        &state,
        &current,
        &client,
        via,
        trip_id,
        poi_id,
        fetched.body,
        credit,
        Some(image_url),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(photo)))
}

/// Retire une photo (éditeur).
#[utoipa::path(delete, path = "/trips/{trip_id}/pois/{poi_id}/photos/{photo_id}", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path), ("photo_id" = Uuid, Path)),
    responses((status = 204), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn delete_photo(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id, photo_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    pois::ensure_in_trip(&state.db, trip_id, poi_id).await?;
    let deleted = sqlx::query("DELETE FROM poi_photos WHERE id = $1 AND poi_id = $2")
        .bind(photo_id)
        .bind(poi_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    for key in [photos::full_key(photo_id), photos::thumb_key(photo_id)] {
        // Un fichier restant sera supprimé par le nettoyage périodique.
        if let Err(e) = state.photos.store.delete(&key).await {
            tracing::warn!(error = ?e, %key, "suppression du fichier de photo en échec");
        }
    }
    state
        .realtime
        .changed(trip_id, &[Topic::PoiContent], &current.user, &client);
    Ok(StatusCode::NO_CONTENT)
}

/// Fichier d'une photo, pour les seuls membres du voyage.
async fn serve(
    state: &AppState,
    current: &CurrentUser,
    trip_id: Uuid,
    poi_id: Uuid,
    photo_id: Uuid,
    key: String,
) -> AppResult<Response> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM poi_photos ph JOIN pois p ON p.id = ph.poi_id
                        WHERE ph.id = $1 AND ph.poi_id = $2 AND p.trip_id = $3)",
    )
    .bind(photo_id)
    .bind(poi_id)
    .bind(trip_id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let bytes = state
        .photos
        .store
        .get(&key)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            // Fichier immuable (une photo modifiée est une nouvelle photo), propre à l'utilisateur.
            (header::CACHE_CONTROL, "private, max-age=604800, immutable"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}

/// Image d'une photo (au plus 1600 px, JPEG).
#[utoipa::path(get, path = "/trips/{trip_id}/pois/{poi_id}/photos/{photo_id}/image", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path), ("photo_id" = Uuid, Path)),
    responses((status = 200, content_type = "image/jpeg", body = Vec<u8>),
        (status = 404, body = ErrorBody)))]
async fn photo_image(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, poi_id, photo_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<Response> {
    serve(
        &state,
        &current,
        trip_id,
        poi_id,
        photo_id,
        photos::full_key(photo_id),
    )
    .await
}

/// Vignette d'une photo (au plus 400 px, JPEG).
#[utoipa::path(get, path = "/trips/{trip_id}/pois/{poi_id}/photos/{photo_id}/thumb", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path), ("photo_id" = Uuid, Path)),
    responses((status = 200, content_type = "image/jpeg", body = Vec<u8>),
        (status = 404, body = ErrorBody)))]
async fn photo_thumb(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, poi_id, photo_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<Response> {
    serve(
        &state,
        &current,
        trip_id,
        poi_id,
        photo_id,
        photos::thumb_key(photo_id),
    )
    .await
}
