//! Points d'intérêt : lecture pour tous les membres, écriture pour les éditeurs.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::opening::{self, DaySchedule, HoursStatus};
use crate::places::{Category, OsmRef};
use crate::pois::{self, Poi};
use crate::realtime::{ClientId, Topic};
use crate::state::AppState;
use crate::trips::{self, Role};
use crate::validate;
use crate::wiki::WikiSummary;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_pois, create_poi))
        .routes(routes!(get_poi, update_poi, delete_poi))
        .routes(routes!(preview_opening_hours))
        .routes(routes!(poi_wiki))
}

#[derive(Deserialize, ToSchema)]
pub struct CreatePoi {
    pub name: String,
    /// Défaut : `other`.
    pub category: Option<Category>,
    pub lat: f64,
    pub lon: f64,
    pub address: Option<String>,
    /// Défaut : durée type de la catégorie.
    pub duration_min: Option<i32>,
    pub opening_hours: Option<String>,
    pub country_code: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    /// Identifiant Wikidata (ex. `Q190622`).
    pub wikidata: Option<String>,
    /// Article Wikipédia au format OSM `langue:Titre`.
    pub wikipedia: Option<String>,
    pub notes: Option<String>,
    /// Objet OSM d'origine (un même lieu n'est ajouté qu'une fois par voyage).
    pub osm: Option<OsmRef>,
}

/// Modification partielle ; une chaîne vide efface un champ facultatif (horaires, site web,
/// téléphone, email, références Wikidata et Wikipédia).
#[derive(Deserialize, ToSchema)]
pub struct UpdatePoi {
    pub version: i64,
    pub name: Option<String>,
    pub category: Option<Category>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub address: Option<String>,
    pub duration_min: Option<i32>,
    pub opening_hours: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub wikidata: Option<String>,
    pub wikipedia: Option<String>,
    pub notes: Option<String>,
}

/// Présentation Wikipédia d'un lieu.
#[derive(Serialize, ToSchema)]
pub struct PoiWiki {
    /// `null` si le lieu n'a pas de référence Wikipédia / Wikidata, ou si l'article n'existe pas.
    pub summary: Option<WikiSummary>,
}

/// Présentations demandées par utilisateur et par minute (le cache n'est pas décompté à part).
const WIKI_PER_USER_PER_MINUTE: u32 = 60;

#[derive(Deserialize, ToSchema)]
pub struct OpeningPreviewRequest {
    pub opening_hours: String,
    pub country_code: Option<String>,
}

/// Horaires interprétés pour chaque date du voyage.
#[derive(Serialize, ToSchema)]
pub struct OpeningPreview {
    pub status: HoursStatus,
    /// Erreur de syntaxe (si `status` vaut `invalid`).
    pub error: Option<String>,
    pub days: Vec<DaySchedule>,
}

/// Présentation du lieu tirée de Wikipédia (résumé, photo Commons créditée, guide
/// Wikivoyage), d'après ses références `wikidata` / `wikipedia`. Textes et photos sous
/// licences libres, à créditer.
#[utoipa::path(get, path = "/trips/{trip_id}/pois/{poi_id}/wiki", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    responses((status = 200, body = PoiWiki), (status = 404, body = ErrorBody),
        (status = 429, body = ErrorBody), (status = 503, body = ErrorBody)))]
async fn poi_wiki(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<PoiWiki>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    let dates = pois::trip_dates(&state.db, trip_id).await?;
    let poi = pois::get(&state.db, trip_id, poi_id, current.user.id, dates).await?;
    if poi.wikidata.is_none() && poi.wikipedia.is_none() {
        return Ok(Json(PoiWiki { summary: None }));
    }
    state.rate_limiter.check(
        &format!("wiki:{}", current.user.id),
        WIKI_PER_USER_PER_MINUTE,
        std::time::Duration::from_secs(60),
    )?;
    let summary = state
        .wiki
        .summary(poi.wikidata.as_deref(), poi.wikipedia.as_deref())
        .await?;
    Ok(Json(PoiWiki { summary }))
}

/// POI du voyage.
#[utoipa::path(get, path = "/trips/{trip_id}/pois", tag = "lieux",
    params(("trip_id" = Uuid, Path)),
    responses((status = 200, body = Vec<Poi>), (status = 404, body = ErrorBody)))]
async fn list_pois(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
) -> AppResult<Json<Vec<Poi>>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    let dates = pois::trip_dates(&state.db, trip_id).await?;
    Ok(Json(
        pois::list(&state.db, trip_id, current.user.id, dates).await?,
    ))
}

/// Ajoute un POI (éditeur).
#[utoipa::path(post, path = "/trips/{trip_id}/pois", tag = "lieux",
    params(("trip_id" = Uuid, Path)), request_body = CreatePoi,
    responses((status = 201, body = Poi), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody, description = "Lieu OSM déjà présent dans le voyage")))]
async fn create_poi(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<CreatePoi>,
) -> AppResult<(StatusCode, Json<Poi>)> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    let name = validate::poi_name(&req.name)?;
    validate::coordinates(req.lat, req.lon)?;
    let category = req.category.unwrap_or(Category::Other);
    let duration = validate::duration_min(
        req.duration_min
            .unwrap_or_else(|| category.default_duration_min()),
    )?;
    let address = validate::address(req.address.as_deref().unwrap_or(""))?;
    let notes = validate::notes(req.notes.as_deref().unwrap_or(""))?;
    let opening_hours = opening::normalize(req.opening_hours.as_deref())?;
    let country_code = validate::country_code(req.country_code.as_deref())?;
    let website = validate::website(req.website.as_deref())?;
    let phone = validate::phone(req.phone.as_deref())?;
    let email = validate::contact_email(req.email.as_deref())?;
    let wikidata = validate::wikidata(req.wikidata.as_deref())?;
    let wikipedia = validate::wikipedia(req.wikipedia.as_deref())?;
    if req.osm.is_some_and(|o| o.id <= 0) {
        return Err(AppError::BadRequest("identifiant OSM invalide".into()));
    }

    let mut tx = state.db.begin().await?;
    // Verrou sur le voyage : sérialise les ajouts concurrents pour respecter la limite.
    sqlx::query("SELECT 1 FROM trips WHERE id = $1 FOR UPDATE")
        .bind(trip_id)
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pois WHERE trip_id = $1")
        .bind(trip_id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= pois::MAX_PER_TRIP {
        return Err(AppError::BadRequest(format!(
            "un voyage ne peut pas contenir plus de {} lieux",
            pois::MAX_PER_TRIP
        )));
    }
    let poi_id: Uuid = sqlx::query_scalar(
        "INSERT INTO pois (trip_id, name, category, location, address, duration_min,
                           opening_hours, country_code, website, notes, osm_type, osm_id, created_by,
                           phone, email, wikidata, wikipedia)
         VALUES ($1, $2, $3, ST_SetSRID(ST_MakePoint($4, $5), 4326)::geography, $6, $7,
                 $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
         RETURNING id",
    )
    .bind(trip_id)
    .bind(&name)
    .bind(category.as_str())
    .bind(req.lon)
    .bind(req.lat)
    .bind(&address)
    .bind(duration)
    .bind(&opening_hours)
    .bind(&country_code)
    .bind(&website)
    .bind(&notes)
    .bind(req.osm.map(|o| o.osm_type.as_str()))
    .bind(req.osm.map(|o| o.id))
    .bind(current.user.id)
    .bind(&phone)
    .bind(&email)
    .bind(&wikidata)
    .bind(&wikipedia)
    .fetch_one(&mut *tx)
    .await
    .map_err(pois::map_duplicate)?;
    let dates = pois::trip_dates(&mut *tx, trip_id).await?;
    let poi = pois::get(&mut *tx, trip_id, poi_id, current.user.id, dates).await?;
    tx.commit().await?;
    tracing::info!(%trip_id, %poi_id, "POI ajouté");
    state.realtime.changed(
        trip_id,
        &[Topic::Pois, Topic::Itinerary],
        &current.user,
        &client,
    );
    Ok((StatusCode::CREATED, Json(poi)))
}

/// Détail d'un POI.
#[utoipa::path(get, path = "/trips/{trip_id}/pois/{poi_id}", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    responses((status = 200, body = Poi), (status = 404, body = ErrorBody)))]
async fn get_poi(
    State(state): State<AppState>,
    current: CurrentUser,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Poi>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    let dates = pois::trip_dates(&state.db, trip_id).await?;
    Ok(Json(
        pois::get(&state.db, trip_id, poi_id, current.user.id, dates).await?,
    ))
}

/// Modifie un POI (éditeur) ; `version` doit correspondre à la version actuelle.
#[utoipa::path(patch, path = "/trips/{trip_id}/pois/{poi_id}", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)), request_body = UpdatePoi,
    responses((status = 200, body = Poi), (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody), (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody, description = "Version obsolète : rechargez le lieu")))]
async fn update_poi(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdatePoi>,
) -> AppResult<Json<Poi>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    let dates = pois::trip_dates(&state.db, trip_id).await?;
    let cur = pois::get(&state.db, trip_id, poi_id, current.user.id, dates).await?;

    let name = match req.name {
        Some(n) => validate::poi_name(&n)?,
        None => cur.name,
    };
    let category = req.category.unwrap_or(cur.category);
    let (lat, lon) = (req.lat.unwrap_or(cur.lat), req.lon.unwrap_or(cur.lon));
    validate::coordinates(lat, lon)?;
    let address = match req.address {
        Some(a) => validate::address(&a)?,
        None => cur.address,
    };
    let duration = validate::duration_min(req.duration_min.unwrap_or(cur.duration_min))?;
    let opening_hours = match req.opening_hours {
        Some(h) => opening::normalize(Some(&h))?,
        None => cur.opening_hours,
    };
    let website = match req.website {
        Some(w) => validate::website(Some(&w))?,
        None => cur.website,
    };
    let phone = match req.phone {
        Some(v) => validate::phone(Some(&v))?,
        None => cur.phone,
    };
    let email = match req.email {
        Some(v) => validate::contact_email(Some(&v))?,
        None => cur.email,
    };
    let wikidata = match req.wikidata {
        Some(v) => validate::wikidata(Some(&v))?,
        None => cur.wikidata,
    };
    let wikipedia = match req.wikipedia {
        Some(v) => validate::wikipedia(Some(&v))?,
        None => cur.wikipedia,
    };
    let notes = match req.notes {
        Some(n) => validate::notes(&n)?,
        None => cur.notes,
    };

    let updated = sqlx::query(
        "UPDATE pois SET name = $4, category = $5,
             location = ST_SetSRID(ST_MakePoint($6, $7), 4326)::geography,
             address = $8, duration_min = $9, opening_hours = $10, website = $11, notes = $12,
             phone = $13, email = $14, wikidata = $15, wikipedia = $16, version = version + 1
         WHERE id = $1 AND trip_id = $2 AND version = $3",
    )
    .bind(poi_id)
    .bind(trip_id)
    .bind(req.version)
    .bind(&name)
    .bind(category.as_str())
    .bind(lon)
    .bind(lat)
    .bind(&address)
    .bind(duration)
    .bind(&opening_hours)
    .bind(&website)
    .bind(&notes)
    .bind(&phone)
    .bind(&email)
    .bind(&wikidata)
    .bind(&wikipedia)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "ce lieu a été modifié entre-temps, rechargez-le".into(),
        ));
    }
    state.realtime.changed(
        trip_id,
        &[Topic::Pois, Topic::Itinerary],
        &current.user,
        &client,
    );
    Ok(Json(
        pois::get(&state.db, trip_id, poi_id, current.user.id, dates).await?,
    ))
}

/// Supprime un POI (éditeur).
#[utoipa::path(delete, path = "/trips/{trip_id}/pois/{poi_id}", tag = "lieux",
    params(("trip_id" = Uuid, Path), ("poi_id" = Uuid, Path)),
    responses((status = 204), (status = 403, body = ErrorBody), (status = 404, body = ErrorBody)))]
async fn delete_poi(
    State(state): State<AppState>,
    current: CurrentUser,
    client: ClientId,
    Path((trip_id, poi_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Editor).await?;
    let deleted = sqlx::query("DELETE FROM pois WHERE id = $1 AND trip_id = $2")
        .bind(poi_id)
        .bind(trip_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tracing::info!(%trip_id, %poi_id, "POI supprimé");
    state.realtime.changed(
        trip_id,
        &[Topic::Pois, Topic::Days, Topic::Itinerary],
        &current.user,
        &client,
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Interprète des horaires OSM sur les dates du voyage, sans rien enregistrer
/// (vérification avant saisie, ou affichage du détail d'un lieu).
#[utoipa::path(post, path = "/trips/{trip_id}/opening-hours/preview", tag = "lieux",
    params(("trip_id" = Uuid, Path)), request_body = OpeningPreviewRequest,
    responses((status = 200, body = OpeningPreview), (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody)))]
async fn preview_opening_hours(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
    Json(req): Json<OpeningPreviewRequest>,
) -> AppResult<Json<OpeningPreview>> {
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    let raw = opening::normalize(Some(&req.opening_hours))?;
    let country = validate::country_code(req.country_code.as_deref())?;
    let (start, end) = pois::trip_dates(&state.db, trip_id).await?;
    let parsed = opening::parse(raw.as_deref(), country.as_deref());
    let status = parsed.status();
    let (error, days) = match parsed {
        opening::Parsed::Valid(oh) => (None, opening::schedule(&oh, start, end)),
        opening::Parsed::Invalid(e) => (Some(e), vec![]),
        opening::Parsed::None => (None, vec![]),
    };
    Ok(Json(OpeningPreview {
        status,
        error,
        days,
    }))
}
