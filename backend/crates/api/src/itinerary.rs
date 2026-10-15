//! Itinéraire d'un voyage : génération (planificateur + temps de trajet réels),
//! enregistrement et relecture avec détection des changements survenus depuis.

use std::collections::{HashMap, HashSet};

use planner::geo::LatLon;
use planner::itinerary::{self as plan, Candidate, DayProblem, DaySlot, TravelMatrix, Visit};
use planner::travel::{self, Leg, LegSource, Mode};
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, OffsetDateTime, Time};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::days::{self, TripDay};
use crate::error::AppResult;
use crate::opening;
use crate::places::Category;
use crate::pois::{self, Poi};
use crate::routing::RoutingService;
use crate::travel::{TravelOption, TravelSource};
use crate::trips::{self, hhmm, parse_modes, TransportMode, Trip};

/// Raison pour laquelle un lieu ne figure pas dans l'itinéraire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnscheduledReason {
    /// Fermé toute la journée à chaque date du voyage.
    ClosedAllDays,
    /// Ses horaires ne laissent pas le temps de la visite dans la journée prévue.
    OpeningHours,
    /// Plus de temps disponible : les lieux les mieux notés passent d'abord.
    NoTime,
    /// Ajouté après la génération de l'itinéraire.
    AddedAfter,
}

impl UnscheduledReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::ClosedAllDays => "closed_all_days",
            Self::OpeningHours => "opening_hours",
            Self::NoTime => "no_time",
            Self::AddedAfter => "added_after",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "closed_all_days" => Self::ClosedAllDays,
            "opening_hours" => Self::OpeningHours,
            "no_time" => Self::NoTime,
            "added_after" => Self::AddedAfter,
            _ => return None,
        })
    }
}

impl From<plan::Unscheduled> for UnscheduledReason {
    fn from(u: plan::Unscheduled) -> Self {
        match u {
            plan::Unscheduled::ClosedAllDays => Self::ClosedAllDays,
            plan::Unscheduled::OpeningHours => Self::OpeningHours,
            plan::Unscheduled::NoTime => Self::NoTime,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UnscheduledPoi {
    pub poi_id: Uuid,
    pub reason: UnscheduledReason,
}

/// Étape de la journée. Les heures sont au format `HH:MM` (heure locale du voyage).
#[derive(Debug, Serialize, ToSchema)]
pub struct ItineraryStop {
    pub poi_id: Uuid,
    /// Arrivée sur place.
    #[schema(example = "09:40")]
    pub arrival: String,
    /// Début de la visite (après une éventuelle attente de l'ouverture).
    #[schema(example = "10:00")]
    pub start: String,
    #[schema(example = "11:30")]
    pub end: String,
    /// Attente avant l'ouverture, en secondes.
    pub wait_s: u32,
    /// Trajet depuis l'étape précédente ou l'hébergement (`null` pour une première
    /// visite sans point de départ).
    pub travel: Option<TravelOption>,
    pub locked: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ItineraryDay {
    #[schema(value_type = String, format = Date)]
    pub date: Date,
    /// Modes utilisés pour ce jour.
    pub modes: Vec<TransportMode>,
    /// Départ : hébergement de la veille, sinon celui du jour.
    pub start_poi_id: Option<Uuid>,
    /// Arrivée : hébergement du jour.
    pub end_poi_id: Option<Uuid>,
    #[schema(example = "09:00")]
    pub day_start: String,
    #[schema(example = "19:00")]
    pub day_end: String,
    pub stops: Vec<ItineraryStop>,
    /// Trajet vers l'hébergement du soir.
    pub back: Option<TravelOption>,
    pub total_travel_s: u32,
    pub total_visit_s: u32,
    /// Fin de la journée (arrivée à l'hébergement ou fin de la dernière visite).
    #[schema(example = "17:45")]
    pub finish: String,
    /// Lieux, réglages ou dates modifiés depuis le calcul de ce jour : à régénérer.
    pub stale: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Itinerary {
    pub version: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
    pub generated_by_name: Option<String>,
    pub days: Vec<ItineraryDay>,
    /// Lieux hors itinéraire (hébergements exclus).
    pub unscheduled: Vec<UnscheduledPoi>,
    /// Au moins un jour est périmé.
    pub stale: bool,
    /// Au moins un trajet est une estimation (vol d'oiseau ou transports en commun).
    pub has_estimates: bool,
}

/// Heure `HH:MM` arrondie à la minute (`24:00` pour minuit en fin de journée).
fn clock(seconds: u32) -> String {
    let min = (seconds + 30) / 60;
    format!("{:02}:{:02}", min / 60, min % 60)
}

fn seconds(t: Time) -> u32 {
    u32::from(t.hour()) * 3600 + u32::from(t.minute()) * 60 + u32::from(t.second())
}

/// Départ et arrivée de chaque jour : on part de l'hébergement de la veille (sinon de
/// celui du jour) et l'on termine à l'hébergement du jour.
fn day_bases(days: &[TripDay]) -> Vec<(Option<Uuid>, Option<Uuid>)> {
    days.iter()
        .enumerate()
        .map(|(d, day)| {
            let previous = d.checked_sub(1).and_then(|p| days[p].base_poi_id);
            (previous.or(day.base_poi_id), day.base_poi_id)
        })
        .collect()
}

/// Les hébergements ne sont pas des visites.
fn is_visitable(p: &Poi, bases: &HashSet<Uuid>) -> bool {
    p.category != Category::Lodging && !bases.contains(&p.id)
}

fn location(p: &Poi) -> LatLon {
    LatLon::new(p.lat, p.lon)
}

// --- Génération ------------------------------------------------------------------------

struct PlannedDay {
    date: Date,
    modes: Vec<TransportMode>,
    start: Option<Uuid>,
    end: Option<Uuid>,
    stops: Vec<(Uuid, plan::Stop)>,
    back: Option<(Mode, Leg)>,
}

/// Génère l'itinéraire complet du voyage et remplace le précédent.
pub async fn generate(
    db: &PgPool,
    routing: &RoutingService,
    trip_id: Uuid,
    user_id: Uuid,
) -> AppResult<()> {
    // Instant de lecture : un lieu modifié après sera signalé comme changé.
    let read_at: OffsetDateTime = sqlx::query_scalar("SELECT now()").fetch_one(db).await?;
    let trip = trips::get_for_user(db, trip_id, user_id).await?;
    let days = days::list(db, trip_id).await?;
    let all_pois = pois::list(db, trip_id, user_id, (trip.start_date, trip.end_date)).await?;
    let by_id: HashMap<Uuid, &Poi> = all_pois.iter().map(|p| (p.id, p)).collect();

    let bases = day_bases(&days);
    let base_ids: HashSet<Uuid> = bases.iter().flat_map(|&(s, e)| [s, e]).flatten().collect();
    let visitable: Vec<&Poi> = all_pois
        .iter()
        .filter(|p| is_visitable(p, &base_ids))
        .collect();

    // Plages d'ouverture de chaque lieu, jour par jour (`None` : horaires inconnus).
    let windows: Vec<Option<Vec<Vec<plan::Window>>>> = visitable
        .iter()
        .map(
            |p| match opening::parse(p.opening_hours.as_deref(), p.country_code.as_deref()) {
                opening::Parsed::Valid(oh) => Some(
                    days.iter()
                        .map(|d| opening::day_windows(&oh, d.date))
                        .collect(),
                ),
                _ => None,
            },
        )
        .collect();

    let budget = seconds(trip.day_end).saturating_sub(seconds(trip.day_start));
    let candidates: Vec<Candidate> = visitable
        .iter()
        .zip(&windows)
        .map(|(p, w)| Candidate {
            location: location(p),
            duration_s: p.duration_min.max(0) as u32 * 60,
            score: plan::score(p.rating.average),
            closed: match w {
                Some(per_day) => per_day.iter().map(Vec::is_empty).collect(),
                None => vec![false; days.len()],
            },
        })
        .collect();
    let slots: Vec<DaySlot> = days
        .iter()
        .zip(&bases)
        .map(|(day, &(start, end))| {
            let place = |id: Option<Uuid>| id.and_then(|id| by_id.get(&id)).map(|p| location(p));
            let mode = plan::fastest_mode(&modes_of(day));
            // Changement d'hébergement : le trajet entre les deux est pris sur la journée.
            let transfer = match (place(start), place(end)) {
                (Some(a), Some(b)) => travel::straight_line(mode, a, b).duration_s,
                _ => 0,
            };
            DaySlot {
                anchor: place(end).or(place(start)),
                budget_s: budget.saturating_sub(transfer),
                mode,
            }
        })
        .collect();
    let assignment = plan::assign_days(&candidates, &slots);

    let mut unscheduled: Vec<(Uuid, UnscheduledReason)> = assignment
        .unassigned
        .iter()
        .map(|&(i, r)| (visitable[i].id, r.into()))
        .collect();
    let mut planned = Vec::with_capacity(days.len());
    for (d, day) in days.iter().enumerate() {
        let (start, end) = bases[d];
        let members = &assignment.days[d];
        let visits: Vec<Visit> = members
            .iter()
            .map(|&i| Visit {
                duration_s: candidates[i].duration_s,
                score: candidates[i].score,
                windows: windows[i].as_ref().map(|w| w[d].clone()),
            })
            .collect();
        // Nœuds : les visites, puis le départ et l'arrivée (un seul nœud s'ils sont identiques).
        let mut nodes: Vec<LatLon> = members.iter().map(|&i| candidates[i].location).collect();
        let node_of = |id: Option<Uuid>, nodes: &mut Vec<LatLon>| {
            let p = by_id.get(&id?)?;
            nodes.push(location(p));
            Some(nodes.len() - 1)
        };
        let start_node = node_of(start, &mut nodes);
        let end_node = if end == start {
            start_node
        } else {
            node_of(end, &mut nodes)
        };
        let modes = modes_of(day);
        let matrix = travel_matrix(routing, &modes, &nodes).await;
        let day_plan = plan::schedule_day(&DayProblem {
            day_start_s: seconds(trip.day_start),
            day_end_s: seconds(trip.day_end),
            visits: &visits,
            start: start_node,
            end: end_node,
            travel: &matrix,
        });
        unscheduled.extend(
            day_plan
                .unscheduled
                .iter()
                .map(|&(v, r)| (visitable[members[v]].id, r.into())),
        );
        planned.push(PlannedDay {
            date: day.date,
            modes: day.modes.clone(),
            start,
            end,
            stops: day_plan
                .stops
                .iter()
                .map(|s| (visitable[members[s.visit]].id, s.clone()))
                .collect(),
            back: day_plan.back,
        });
    }
    save(db, &trip, user_id, read_at, &planned, &unscheduled).await
}

fn modes_of(day: &TripDay) -> Vec<Mode> {
    day.modes.iter().map(|&m| Mode::from(m)).collect()
}

/// Matrice des trajets d'une journée : pour chaque paire, le mode choisi par la règle
/// commune ([`travel::choose_mode`]) parmi ceux du jour.
async fn travel_matrix(routing: &RoutingService, modes: &[Mode], nodes: &[LatLon]) -> TravelMatrix {
    let zero = Leg {
        duration_s: 0,
        distance_m: 0,
        source: LegSource::Router,
    };
    let fallback_mode = modes.first().copied().unwrap_or(Mode::Car);
    if nodes.len() < 2 {
        return TravelMatrix::new(nodes.len(), |_, _| (fallback_mode, zero));
    }
    let mut per_mode = Vec::with_capacity(modes.len());
    for &mode in modes {
        per_mode.push((mode, routing.matrix(mode, nodes).await));
    }
    TravelMatrix::new(nodes.len(), |i, j| {
        if i == j {
            return (fallback_mode, zero);
        }
        let options: Vec<(Mode, Leg)> = per_mode.iter().map(|(m, grid)| (*m, grid[i][j])).collect();
        travel::choose_mode(&options).unwrap_or((fallback_mode, zero))
    })
}

fn travel_columns(
    t: Option<(Mode, Leg)>,
) -> (
    Option<&'static str>,
    Option<i32>,
    Option<i32>,
    Option<&'static str>,
) {
    match t {
        Some((mode, leg)) => (
            Some(TransportMode::from(mode).as_str()),
            Some(leg.duration_s as i32),
            Some(leg.distance_m as i32),
            Some(TravelSource::from(leg.source).as_str()),
        ),
        None => (None, None, None, None),
    }
}

async fn save(
    db: &PgPool,
    trip: &Trip,
    user_id: Uuid,
    read_at: OffsetDateTime,
    days: &[PlannedDay],
    unscheduled: &[(Uuid, UnscheduledReason)],
) -> AppResult<()> {
    let mut tx = db.begin().await?;
    // Sérialise les générations concurrentes d'un même voyage.
    sqlx::query("SELECT 1 FROM trips WHERE id = $1 FOR UPDATE")
        .bind(trip.id)
        .execute(&mut *tx)
        .await?;
    let version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM itineraries WHERE trip_id = $1), 0) + 1",
    )
    .bind(trip.id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM itineraries WHERE trip_id = $1")
        .bind(trip.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO itineraries (trip_id, version, generated_at, generated_by) VALUES ($1, $2, $3, $4)",
    )
    .bind(trip.id)
    .bind(version)
    .bind(read_at)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    for day in days {
        let modes: Vec<&str> = day.modes.iter().map(|m| m.as_str()).collect();
        let (back_mode, back_duration, back_distance, back_source) = travel_columns(day.back);
        sqlx::query(
            "INSERT INTO itinerary_days (trip_id, date, modes, start_poi_id, end_poi_id,
                 day_start, day_end, computed_at, stop_count,
                 back_mode, back_duration_s, back_distance_m, back_source)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(trip.id)
        .bind(day.date)
        .bind(&modes)
        .bind(day.start)
        .bind(day.end)
        .bind(trip.day_start)
        .bind(trip.day_end)
        .bind(read_at)
        .bind(day.stops.len() as i16)
        .bind(back_mode)
        .bind(back_duration)
        .bind(back_distance)
        .bind(back_source)
        .execute(&mut *tx)
        .await?;
        for (position, (poi_id, stop)) in day.stops.iter().enumerate() {
            let (mode, duration, distance, source) = travel_columns(stop.travel);
            sqlx::query(
                "INSERT INTO itinerary_stops (trip_id, date, position, poi_id, arrival_s, start_s, end_s,
                     travel_mode, travel_duration_s, travel_distance_m, travel_source)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(trip.id)
            .bind(day.date)
            .bind(position as i16)
            .bind(poi_id)
            .bind(stop.arrival_s as i32)
            .bind(stop.start_s as i32)
            .bind(stop.end_s as i32)
            .bind(mode)
            .bind(duration)
            .bind(distance)
            .bind(source)
            .execute(&mut *tx)
            .await?;
        }
    }
    for (poi_id, reason) in unscheduled {
        sqlx::query(
            "INSERT INTO itinerary_unscheduled (trip_id, poi_id, reason) VALUES ($1, $2, $3)",
        )
        .bind(trip.id)
        .bind(poi_id)
        .bind(reason.as_str())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    tracing::info!(trip_id = %trip.id, version, "itinéraire généré");
    Ok(())
}

// --- Lecture ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct HeadRow {
    version: i64,
    generated_at: OffsetDateTime,
    generated_by_name: Option<String>,
}

#[derive(sqlx::FromRow)]
struct DayRow {
    date: Date,
    modes: Vec<String>,
    start_poi_id: Option<Uuid>,
    end_poi_id: Option<Uuid>,
    day_start: Time,
    day_end: Time,
    computed_at: OffsetDateTime,
    stop_count: i16,
    back_mode: Option<String>,
    back_duration_s: Option<i32>,
    back_distance_m: Option<i32>,
    back_source: Option<String>,
}

#[derive(sqlx::FromRow)]
struct StopRow {
    date: Date,
    poi_id: Uuid,
    arrival_s: i32,
    start_s: i32,
    end_s: i32,
    travel_mode: Option<String>,
    travel_duration_s: Option<i32>,
    travel_distance_m: Option<i32>,
    travel_source: Option<String>,
    locked: bool,
}

fn travel_from_columns(
    mode: Option<&str>,
    duration: Option<i32>,
    distance: Option<i32>,
    source: Option<&str>,
) -> Option<TravelOption> {
    Some(TravelOption {
        mode: TransportMode::parse(mode?)?,
        duration_s: duration?.max(0) as u32,
        distance_m: distance?.max(0) as u32,
        source: TravelSource::parse(source?)?,
    })
}

/// Itinéraire du voyage, ou `None` s'il n'a jamais été généré.
pub async fn load(db: &PgPool, trip: &Trip) -> AppResult<Option<Itinerary>> {
    let head: Option<HeadRow> = sqlx::query_as(
        "SELECT i.version, i.generated_at, u.display_name AS generated_by_name
         FROM itineraries i LEFT JOIN users u ON u.id = i.generated_by
         WHERE i.trip_id = $1",
    )
    .bind(trip.id)
    .fetch_optional(db)
    .await?;
    let Some(head) = head else {
        return Ok(None);
    };
    let day_rows: Vec<DayRow> = sqlx::query_as(
        "SELECT date, modes, start_poi_id, end_poi_id, day_start, day_end, computed_at, stop_count,
                back_mode, back_duration_s, back_distance_m, back_source
         FROM itinerary_days WHERE trip_id = $1 ORDER BY date",
    )
    .bind(trip.id)
    .fetch_all(db)
    .await?;
    let stop_rows: Vec<StopRow> = sqlx::query_as(
        "SELECT date, poi_id, arrival_s, start_s, end_s, travel_mode,
                travel_duration_s, travel_distance_m, travel_source, locked
         FROM itinerary_stops WHERE trip_id = $1 ORDER BY date, position",
    )
    .bind(trip.id)
    .fetch_all(db)
    .await?;
    let unscheduled_rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT poi_id, reason FROM itinerary_unscheduled WHERE trip_id = $1")
            .bind(trip.id)
            .fetch_all(db)
            .await?;
    // État actuel du voyage, pour détecter ce qui a changé depuis le calcul.
    let current_days = days::list(db, trip.id).await?;
    let poi_rows: Vec<(Uuid, String, OffsetDateTime)> =
        sqlx::query_as("SELECT id, category, updated_at FROM pois WHERE trip_id = $1")
            .bind(trip.id)
            .fetch_all(db)
            .await?;
    let updated_at: HashMap<Uuid, OffsetDateTime> =
        poi_rows.iter().map(|(id, _, at)| (*id, *at)).collect();

    let bases = day_bases(&current_days);
    let mut stops_by_date: HashMap<Date, Vec<StopRow>> = HashMap::new();
    for s in stop_rows {
        stops_by_date.entry(s.date).or_default().push(s);
    }
    let stored: HashMap<Date, DayRow> = day_rows.into_iter().map(|d| (d.date, d)).collect();
    let mut stale = stored
        .keys()
        .any(|d| !current_days.iter().any(|c| c.date == *d));
    let mut has_estimates = false;
    let mut planned: HashSet<Uuid> = HashSet::new();

    let mut out_days = Vec::with_capacity(current_days.len());
    for (current, &(start, end)) in current_days.iter().zip(&bases) {
        let Some(row) = stored.get(&current.date) else {
            // Jour ajouté depuis (dates du voyage modifiées).
            stale = true;
            out_days.push(ItineraryDay {
                date: current.date,
                modes: current.modes.clone(),
                start_poi_id: start,
                end_poi_id: end,
                day_start: hhmm::format(trip.day_start),
                day_end: hhmm::format(trip.day_end),
                stops: vec![],
                back: None,
                total_travel_s: 0,
                total_visit_s: 0,
                finish: hhmm::format(trip.day_start),
                stale: true,
            });
            continue;
        };
        let rows = stops_by_date.remove(&current.date).unwrap_or_default();
        let changed_since = |id: &Uuid| updated_at.get(id).is_some_and(|at| *at > row.computed_at);
        let day_stale = parse_modes(&row.modes) != current.modes
            || (row.start_poi_id, row.end_poi_id) != (start, end)
            || (row.day_start, row.day_end) != (trip.day_start, trip.day_end)
            // Étape supprimée avec son lieu.
            || rows.len() != row.stop_count as usize
            || rows.iter().any(|s| changed_since(&s.poi_id))
            || [row.start_poi_id, row.end_poi_id].iter().flatten().any(changed_since);
        stale |= day_stale;

        let stops: Vec<ItineraryStop> = rows
            .iter()
            .map(|s| {
                planned.insert(s.poi_id);
                ItineraryStop {
                    poi_id: s.poi_id,
                    arrival: clock(s.arrival_s as u32),
                    start: clock(s.start_s as u32),
                    end: clock(s.end_s as u32),
                    wait_s: (s.start_s - s.arrival_s).max(0) as u32,
                    travel: travel_from_columns(
                        s.travel_mode.as_deref(),
                        s.travel_duration_s,
                        s.travel_distance_m,
                        s.travel_source.as_deref(),
                    ),
                    locked: s.locked,
                }
            })
            .collect();
        let back = travel_from_columns(
            row.back_mode.as_deref(),
            row.back_duration_s,
            row.back_distance_m,
            row.back_source.as_deref(),
        );
        let travels = stops
            .iter()
            .filter_map(|s| s.travel.as_ref())
            .chain(back.as_ref());
        has_estimates |= travels.clone().any(|t| t.source.is_estimate());
        let total_travel_s = travels.map(|t| t.duration_s).sum();
        let total_visit_s = rows
            .iter()
            .map(|s| (s.end_s - s.start_s).max(0) as u32)
            .sum();
        let last_end = rows
            .last()
            .map_or(seconds(row.day_start), |s| s.end_s as u32);
        out_days.push(ItineraryDay {
            date: current.date,
            modes: parse_modes(&row.modes),
            start_poi_id: row.start_poi_id,
            end_poi_id: row.end_poi_id,
            day_start: hhmm::format(row.day_start),
            day_end: hhmm::format(row.day_end),
            finish: clock(last_end + back.as_ref().map_or(0, |b| b.duration_s)),
            stops,
            back,
            total_travel_s,
            total_visit_s,
            stale: day_stale,
        });
    }

    let base_ids: HashSet<Uuid> = bases.iter().flat_map(|&(s, e)| [s, e]).flatten().collect();
    let known: HashMap<Uuid, UnscheduledReason> = unscheduled_rows
        .iter()
        .filter_map(|(id, r)| Some((*id, UnscheduledReason::parse(r)?)))
        .collect();
    let mut unscheduled: Vec<UnscheduledPoi> = poi_rows
        .iter()
        .filter(|(id, category, _)| {
            category != Category::Lodging.as_str()
                && !base_ids.contains(id)
                && !planned.contains(id)
        })
        .map(|(id, _, _)| UnscheduledPoi {
            poi_id: *id,
            reason: known
                .get(id)
                .copied()
                .unwrap_or(UnscheduledReason::AddedAfter),
        })
        .collect();
    unscheduled.sort_by_key(|u| u.poi_id);

    Ok(Some(Itinerary {
        version: head.version,
        generated_at: head.generated_at,
        generated_by_name: head.generated_by_name,
        days: out_days,
        unscheduled,
        stale,
        has_estimates,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_rounds_to_the_minute() {
        assert_eq!(clock(9 * 3600), "09:00");
        assert_eq!(clock(9 * 3600 + 29), "09:00");
        assert_eq!(clock(9 * 3600 + 30), "09:01");
        assert_eq!(clock(86_400), "24:00");
    }

    fn day(base: Option<u128>) -> TripDay {
        TripDay {
            date: time::macros::date!(2026 - 12 - 01),
            modes: vec![TransportMode::Car],
            modes_inherited: true,
            base_poi_id: base.map(Uuid::from_u128),
        }
    }

    #[test]
    fn days_start_from_last_night_lodging() {
        let id = Uuid::from_u128;
        let bases = day_bases(&[
            day(Some(1)),
            day(Some(1)),
            day(Some(2)),
            day(None),
            day(Some(3)),
        ]);
        assert_eq!(
            bases,
            vec![
                (Some(id(1)), Some(id(1))),
                (Some(id(1)), Some(id(1))),
                // Changement d'hôtel : départ du premier, arrivée au second.
                (Some(id(1)), Some(id(2))),
                (Some(id(2)), None),
                (Some(id(3)), Some(id(3))),
            ]
        );
    }
}
