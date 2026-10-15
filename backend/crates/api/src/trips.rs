//! Voyages, rôles des membres et contrôle d'accès.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sqlx::{PgExecutor, PgPool};
use time::{Date, OffsetDateTime, Time};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// Rôle d'un membre dans un voyage, du moins au plus privilégié.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Consulte le voyage.
    Viewer,
    /// Ajoute et modifie le contenu (POI, avis, itinéraire).
    Editor,
    /// Gère le voyage, ses membres et ses invitations (un seul par voyage).
    Organizer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Editor => "editor",
            Role::Organizer => "organizer",
        }
    }
}

impl FromStr for Role {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, AppError> {
        match s {
            "viewer" => Ok(Role::Viewer),
            "editor" => Ok(Role::Editor),
            "organizer" => Ok(Role::Organizer),
            other => Err(AppError::Internal(anyhow::anyhow!(
                "rôle inconnu : {other}"
            ))),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Moyen de transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    Walk,
    Bike,
    Car,
    /// Transports en commun (temps estimés au Costa Rica, voir la documentation).
    Transit,
}

impl TransportMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TransportMode::Walk => "walk",
            TransportMode::Bike => "bike",
            TransportMode::Car => "car",
            TransportMode::Transit => "transit",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "walk" => TransportMode::Walk,
            "bike" => TransportMode::Bike,
            "car" => TransportMode::Car,
            "transit" => TransportMode::Transit,
            _ => return None,
        })
    }
}

impl From<TransportMode> for planner::travel::Mode {
    fn from(m: TransportMode) -> Self {
        match m {
            TransportMode::Walk => Self::Walk,
            TransportMode::Bike => Self::Bike,
            TransportMode::Car => Self::Car,
            TransportMode::Transit => Self::Transit,
        }
    }
}

impl From<planner::travel::Mode> for TransportMode {
    fn from(m: planner::travel::Mode) -> Self {
        use planner::travel::Mode;
        match m {
            Mode::Walk => Self::Walk,
            Mode::Bike => Self::Bike,
            Mode::Car => Self::Car,
            Mode::Transit => Self::Transit,
        }
    }
}

/// Modes stockés en base (valeurs inconnues ignorées).
pub fn parse_modes(stored: &[String]) -> Vec<TransportMode> {
    stored
        .iter()
        .filter_map(|m| TransportMode::parse(m))
        .collect()
}

/// Liste de modes non vide et sans doublon, dans l'ordre donné.
pub fn normalize_modes(modes: &[TransportMode]) -> AppResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for m in modes {
        if !out.iter().any(|s| s == m.as_str()) {
            out.push(m.as_str().to_owned());
        }
    }
    if out.is_empty() {
        return Err(AppError::BadRequest(
            "choisissez au moins un moyen de transport".into(),
        ));
    }
    Ok(out)
}

/// Sérialisation des heures au format `HH:MM`.
pub mod hhmm {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::Time;

    pub fn format(t: Time) -> String {
        format!("{:02}:{:02}", t.hour(), t.minute())
    }

    pub fn parse(s: &str) -> Option<Time> {
        let (h, m) = s.split_once(':')?;
        if h.len() != 2 || m.len() != 2 {
            return None;
        }
        Time::from_hms(h.parse().ok()?, m.parse().ok()?, 0).ok()
    }

    pub fn serialize<S: Serializer>(t: &Time, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format(*t))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Time, D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).ok_or_else(|| serde::de::Error::custom("heure attendue au format HH:MM"))
    }

    pub mod option {
        use super::*;

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Time>, D::Error> {
            Option::<String>::deserialize(d)?
                .map(|s| {
                    parse(&s)
                        .ok_or_else(|| serde::de::Error::custom("heure attendue au format HH:MM"))
                })
                .transpose()
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct TripRow {
    id: Uuid,
    name: String,
    description: String,
    start_date: Date,
    end_date: Date,
    timezone: String,
    day_start: Time,
    day_end: Time,
    default_modes: Vec<String>,
    version: i64,
    created_at: OffsetDateTime,
    my_role: String,
    member_count: i64,
}

/// Voyage vu par un membre.
#[derive(Debug, Serialize, ToSchema)]
pub struct Trip {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    #[schema(value_type = String, format = Date, example = "2026-12-01")]
    pub start_date: Date,
    #[schema(value_type = String, format = Date, example = "2026-12-10")]
    pub end_date: Date,
    /// Fuseau IANA (ex. `America/Costa_Rica`).
    pub timezone: String,
    #[serde(with = "hhmm")]
    #[schema(value_type = String, example = "09:00")]
    pub day_start: Time,
    #[serde(with = "hhmm")]
    #[schema(value_type = String, example = "19:00")]
    pub day_end: Time,
    pub default_modes: Vec<TransportMode>,
    /// Version pour le verrouillage optimiste (à renvoyer lors d'une modification).
    pub version: i64,
    pub my_role: Role,
    pub member_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl TryFrom<TripRow> for Trip {
    type Error = AppError;
    fn try_from(r: TripRow) -> AppResult<Self> {
        Ok(Self {
            id: r.id,
            name: r.name,
            description: r.description,
            start_date: r.start_date,
            end_date: r.end_date,
            timezone: r.timezone,
            day_start: r.day_start,
            day_end: r.day_end,
            default_modes: parse_modes(&r.default_modes),
            version: r.version,
            my_role: r.my_role.parse()?,
            member_count: r.member_count,
            created_at: r.created_at,
        })
    }
}

const TRIP_SELECT: &str = "
    SELECT t.id, t.name, t.description, t.start_date, t.end_date, t.timezone,
           t.day_start, t.day_end, t.default_modes, t.version, t.created_at,
           m.role AS my_role,
           (SELECT count(*) FROM trip_members mm WHERE mm.trip_id = t.id) AS member_count
    FROM trips t
    JOIN trip_members m ON m.trip_id = t.id AND m.user_id = $1";

/// Voyages dont l'utilisateur est membre, les plus proches d'abord.
pub async fn list_for_user(db: &PgPool, user_id: Uuid) -> AppResult<Vec<Trip>> {
    let rows: Vec<TripRow> =
        sqlx::query_as(&format!("{TRIP_SELECT} ORDER BY t.start_date, t.name"))
            .bind(user_id)
            .fetch_all(db)
            .await?;
    rows.into_iter().map(Trip::try_from).collect()
}

/// Voyage vu par `user_id` ; `NotFound` s'il n'en est pas membre.
pub async fn get_for_user(
    db: impl PgExecutor<'_>,
    trip_id: Uuid,
    user_id: Uuid,
) -> AppResult<Trip> {
    let row: Option<TripRow> = sqlx::query_as(&format!("{TRIP_SELECT} WHERE t.id = $2"))
        .bind(user_id)
        .bind(trip_id)
        .fetch_optional(db)
        .await?;
    row.ok_or(AppError::NotFound)?.try_into()
}

/// Rôle de l'utilisateur dans le voyage, ou `None` s'il n'est pas membre.
pub async fn role_of(
    db: impl PgExecutor<'_>,
    trip_id: Uuid,
    user_id: Uuid,
) -> AppResult<Option<Role>> {
    let role: Option<String> =
        sqlx::query_scalar("SELECT role FROM trip_members WHERE trip_id = $1 AND user_id = $2")
            .bind(trip_id)
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    role.map(|r| r.parse()).transpose()
}

/// Contrôle d'accès commun à toutes les routes d'un voyage.
///
/// Un non-membre reçoit 404 (l'existence du voyage n'est pas révélée) ;
/// un membre au rôle insuffisant reçoit 403.
pub async fn require_role(
    db: impl PgExecutor<'_>,
    trip_id: Uuid,
    user_id: Uuid,
    min: Role,
) -> AppResult<Role> {
    match role_of(db, trip_id, user_id).await? {
        None => Err(AppError::NotFound),
        Some(role) if role < min => Err(AppError::Forbidden),
        Some(role) => Ok(role),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_ordered() {
        assert!(Role::Viewer < Role::Editor);
        assert!(Role::Editor < Role::Organizer);
    }

    #[test]
    fn hhmm_roundtrip() {
        let t = hhmm::parse("08:30").unwrap();
        assert_eq!(hhmm::format(t), "08:30");
        for bad in ["8:30", "24:00", "12:60", "midi", "12-30"] {
            assert!(hhmm::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn modes_are_deduplicated_and_required() {
        use TransportMode::*;
        assert_eq!(
            normalize_modes(&[Car, Walk, Car]).unwrap(),
            vec!["car", "walk"]
        );
        assert!(normalize_modes(&[]).is_err());
    }
}
