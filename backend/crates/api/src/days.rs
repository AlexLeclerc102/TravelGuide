//! Jours du voyage : moyens de transport et hébergement propres à chaque jour.

use serde::Serialize;
use sqlx::PgExecutor;
use time::Date;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::trips::{parse_modes, TransportMode};

/// Un jour du voyage, avec ses réglages effectifs.
#[derive(Debug, Serialize, ToSchema)]
pub struct TripDay {
    #[schema(value_type = String, format = Date, example = "2026-12-01")]
    pub date: Date,
    /// Modes effectifs : ceux du jour, sinon ceux du voyage.
    pub modes: Vec<TransportMode>,
    /// `true` : le jour reprend les modes par défaut du voyage.
    pub modes_inherited: bool,
    /// Hébergement du jour (départ et arrivée de la journée), s'il est défini.
    pub base_poi_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
struct DayRow {
    date: Date,
    modes: Option<Vec<String>>,
    default_modes: Vec<String>,
    base_poi_id: Option<Uuid>,
}

impl From<DayRow> for TripDay {
    fn from(r: DayRow) -> Self {
        let modes_inherited = r.modes.is_none();
        Self {
            date: r.date,
            modes: parse_modes(r.modes.as_ref().unwrap_or(&r.default_modes)),
            modes_inherited,
            base_poi_id: r.base_poi_id,
        }
    }
}

/// Tous les jours du voyage, dans l'ordre (les réglages hors des dates sont ignorés).
pub async fn list(db: impl PgExecutor<'_>, trip_id: Uuid) -> AppResult<Vec<TripDay>> {
    let rows: Vec<DayRow> = sqlx::query_as(
        "SELECT d::date AS date, td.modes, t.default_modes, td.base_poi_id
         FROM trips t
         CROSS JOIN generate_series(t.start_date, t.end_date, interval '1 day') AS d
         LEFT JOIN trip_days td ON td.trip_id = t.id AND td.date = d::date
         WHERE t.id = $1
         ORDER BY d",
    )
    .bind(trip_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(TripDay::from).collect())
}

/// Un jour du voyage ; `BadRequest` si la date n'en fait pas partie.
pub async fn get(db: impl PgExecutor<'_>, trip_id: Uuid, date: Date) -> AppResult<TripDay> {
    list(db, trip_id)
        .await?
        .into_iter()
        .find(|d| d.date == date)
        .ok_or_else(not_in_trip)
}

pub fn not_in_trip() -> AppError {
    AppError::BadRequest("cette date ne fait pas partie du voyage".into())
}
