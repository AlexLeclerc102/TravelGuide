//! Points d'intérêt d'un voyage (coordonnées stockées en PostGIS).

use serde::Serialize;
use sqlx::PgExecutor;
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::opening::{self, HoursStatus};
use crate::places::{Category, OsmRef, OsmType};

/// Nombre maximal de POI par voyage (borne le coût de l'algorithme d'itinéraire).
pub const MAX_PER_TRIP: i64 = 300;

#[derive(Debug, Serialize, ToSchema)]
pub struct Poi {
    pub id: Uuid,
    pub trip_id: Uuid,
    pub name: String,
    pub category: Category,
    /// Zone d'hébergement : hébergement encore à choisir (ville ou quartier approximatif).
    pub placeholder: bool,
    pub lat: f64,
    pub lon: f64,
    pub address: String,
    /// Durée de visite estimée, en minutes.
    pub duration_min: i32,
    /// Horaires au format OSM `opening_hours`, tels que saisis.
    pub opening_hours: Option<String>,
    pub opening_hours_status: HoursStatus,
    /// Dates du voyage où le lieu est fermé toute la journée (horaires valides uniquement).
    #[schema(value_type = Vec<String>)]
    pub closed_dates: Vec<Date>,
    pub country_code: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    /// Identifiant Wikidata (ex. `Q190622`).
    pub wikidata: Option<String>,
    /// Article Wikipédia au format OSM `langue:Titre`.
    pub wikipedia: Option<String>,
    pub notes: String,
    pub osm: Option<OsmRef>,
    /// Notes du groupe.
    pub rating: GroupRating,
    /// Avis de l'utilisateur courant, s'il en a donné un.
    pub my_review: Option<MyReview>,
    /// Version pour le verrouillage optimiste.
    pub version: i64,
    pub created_by: Option<Uuid>,
    pub created_by_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Synthèse des notes du groupe (définition partagée avec le planificateur).
#[derive(Debug, Serialize, ToSchema)]
pub struct GroupRating {
    pub count: usize,
    /// Moyenne de 1 à 5, absente sans avis.
    pub average: Option<f64>,
    pub min: Option<u8>,
    pub max: Option<u8>,
    /// Écart d'au moins 2 points entre la note la plus basse et la plus haute.
    pub disagreement: bool,
}

impl From<planner::ratings::RatingSummary> for GroupRating {
    fn from(s: planner::ratings::RatingSummary) -> Self {
        Self {
            count: s.count,
            average: s.average,
            min: s.min,
            max: s.max,
            disagreement: s.disagreement,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MyReview {
    pub rating: u8,
    pub comment: String,
}

#[derive(sqlx::FromRow)]
struct PoiRow {
    id: Uuid,
    trip_id: Uuid,
    name: String,
    category: String,
    placeholder: bool,
    lat: f64,
    lon: f64,
    address: String,
    duration_min: i32,
    opening_hours: Option<String>,
    country_code: Option<String>,
    website: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    wikidata: Option<String>,
    wikipedia: Option<String>,
    notes: String,
    osm_type: Option<String>,
    osm_id: Option<i64>,
    ratings: Vec<i16>,
    my_rating: Option<i16>,
    my_comment: Option<String>,
    version: i64,
    created_by: Option<Uuid>,
    created_by_name: Option<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl PoiRow {
    fn into_poi(self, trip_dates: (Date, Date)) -> AppResult<Poi> {
        let parsed = opening::parse(self.opening_hours.as_deref(), self.country_code.as_deref());
        let closed_dates = match &parsed {
            opening::Parsed::Valid(oh) => opening::closed_dates(oh, trip_dates.0, trip_dates.1),
            _ => vec![],
        };
        let osm = match (
            self.osm_type.as_deref().and_then(OsmType::parse),
            self.osm_id,
        ) {
            (Some(osm_type), Some(id)) => Some(OsmRef { osm_type, id }),
            _ => None,
        };
        Ok(Poi {
            id: self.id,
            trip_id: self.trip_id,
            name: self.name,
            category: self.category.parse()?,
            placeholder: self.placeholder,
            lat: self.lat,
            lon: self.lon,
            address: self.address,
            duration_min: self.duration_min,
            opening_hours_status: parsed.status(),
            opening_hours: self.opening_hours,
            closed_dates,
            country_code: self.country_code,
            website: self.website,
            phone: self.phone,
            email: self.email,
            wikidata: self.wikidata,
            wikipedia: self.wikipedia,
            notes: self.notes,
            osm,
            rating: planner::ratings::summarize(&to_u8(&self.ratings)).into(),
            my_review: self.my_rating.map(|r| MyReview {
                rating: r as u8,
                comment: self.my_comment.unwrap_or_default(),
            }),
            version: self.version,
            created_by: self.created_by,
            created_by_name: self.created_by_name,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Notes stockées en `smallint` (contrainte 1 à 5 en base).
fn to_u8(ratings: &[i16]) -> Vec<u8> {
    ratings.iter().map(|&r| r as u8).collect()
}

/// `$1` : utilisateur courant (pour `my_review`).
const POI_SELECT: &str = "
    SELECT p.id, p.trip_id, p.name, p.category, p.placeholder,
           ST_Y(p.location::geometry) AS lat, ST_X(p.location::geometry) AS lon,
           p.address, p.duration_min, p.opening_hours, p.country_code, p.website,
           p.phone, p.email, p.wikidata, p.wikipedia, p.notes,
           p.osm_type, p.osm_id,
           COALESCE((SELECT array_agg(r.rating) FROM reviews r WHERE r.poi_id = p.id), '{}')
               AS ratings,
           mr.rating AS my_rating, mr.comment AS my_comment,
           p.version, p.created_by, u.display_name AS created_by_name,
           p.created_at, p.updated_at
    FROM pois p
    LEFT JOIN users u ON u.id = p.created_by
    LEFT JOIN reviews mr ON mr.poi_id = p.id AND mr.user_id = $1";

/// Dates de début et de fin du voyage.
pub async fn trip_dates(db: impl PgExecutor<'_>, trip_id: Uuid) -> AppResult<(Date, Date)> {
    sqlx::query_as("SELECT start_date, end_date FROM trips WHERE id = $1")
        .bind(trip_id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

/// POI du voyage, vus par `viewer` (pour `my_review`).
pub async fn list(
    db: impl PgExecutor<'_>,
    trip_id: Uuid,
    viewer: Uuid,
    trip_dates: (Date, Date),
) -> AppResult<Vec<Poi>> {
    let rows: Vec<PoiRow> = sqlx::query_as(&format!(
        "{POI_SELECT} WHERE p.trip_id = $2 ORDER BY p.created_at, p.id"
    ))
    .bind(viewer)
    .bind(trip_id)
    .fetch_all(db)
    .await?;
    rows.into_iter().map(|r| r.into_poi(trip_dates)).collect()
}

/// POI du voyage ; `NotFound` s'il appartient à un autre voyage.
pub async fn get(
    db: impl PgExecutor<'_>,
    trip_id: Uuid,
    poi_id: Uuid,
    viewer: Uuid,
    trip_dates: (Date, Date),
) -> AppResult<Poi> {
    let row: Option<PoiRow> =
        sqlx::query_as(&format!("{POI_SELECT} WHERE p.trip_id = $2 AND p.id = $3"))
            .bind(viewer)
            .bind(trip_id)
            .bind(poi_id)
            .fetch_optional(db)
            .await?;
    row.ok_or(AppError::NotFound)?.into_poi(trip_dates)
}

/// Traduit la violation d'unicité « même lieu OSM dans le voyage » en 409.
pub fn map_duplicate(e: sqlx::Error) -> AppError {
    match &e {
        sqlx::Error::Database(db) if db.constraint() == Some("pois_trip_osm_unique") => {
            AppError::Conflict("ce lieu est déjà dans le voyage".into())
        }
        _ => e.into(),
    }
}
