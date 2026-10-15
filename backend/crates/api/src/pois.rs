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
    pub notes: String,
    pub osm: Option<OsmRef>,
    /// Version pour le verrouillage optimiste.
    pub version: i64,
    pub created_by: Option<Uuid>,
    pub created_by_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct PoiRow {
    id: Uuid,
    trip_id: Uuid,
    name: String,
    category: String,
    lat: f64,
    lon: f64,
    address: String,
    duration_min: i32,
    opening_hours: Option<String>,
    country_code: Option<String>,
    website: Option<String>,
    notes: String,
    osm_type: Option<String>,
    osm_id: Option<i64>,
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
            lat: self.lat,
            lon: self.lon,
            address: self.address,
            duration_min: self.duration_min,
            opening_hours_status: parsed.status(),
            opening_hours: self.opening_hours,
            closed_dates,
            country_code: self.country_code,
            website: self.website,
            notes: self.notes,
            osm,
            version: self.version,
            created_by: self.created_by,
            created_by_name: self.created_by_name,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const POI_SELECT: &str = "
    SELECT p.id, p.trip_id, p.name, p.category,
           ST_Y(p.location::geometry) AS lat, ST_X(p.location::geometry) AS lon,
           p.address, p.duration_min, p.opening_hours, p.country_code, p.website, p.notes,
           p.osm_type, p.osm_id, p.version, p.created_by, u.display_name AS created_by_name,
           p.created_at, p.updated_at
    FROM pois p LEFT JOIN users u ON u.id = p.created_by";

/// Dates de début et de fin du voyage.
pub async fn trip_dates(db: impl PgExecutor<'_>, trip_id: Uuid) -> AppResult<(Date, Date)> {
    sqlx::query_as("SELECT start_date, end_date FROM trips WHERE id = $1")
        .bind(trip_id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn list(
    db: impl PgExecutor<'_>,
    trip_id: Uuid,
    trip_dates: (Date, Date),
) -> AppResult<Vec<Poi>> {
    let rows: Vec<PoiRow> = sqlx::query_as(&format!(
        "{POI_SELECT} WHERE p.trip_id = $1 ORDER BY p.created_at, p.id"
    ))
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
    trip_dates: (Date, Date),
) -> AppResult<Poi> {
    let row: Option<PoiRow> =
        sqlx::query_as(&format!("{POI_SELECT} WHERE p.trip_id = $1 AND p.id = $2"))
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
