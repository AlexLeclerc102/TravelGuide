//! Itinéraire d'un voyage : génération (planificateur + temps de trajet réels),
//! ajustements manuels, enregistrement et relecture avec détection des changements
//! survenus depuis le calcul.
//!
//! Toute écriture se fait dans une transaction qui verrouille le voyage : générations
//! et ajustements concurrents s'enchaînent sans se mélanger.

use std::collections::{HashMap, HashSet};

use planner::geo::LatLon;
use planner::itinerary::{
    self as plan, Candidate, DayProblem, DaySlot, StopIssue, TravelMatrix, Visit,
};
use planner::travel::{self, Leg, LegSource, Mode};
use serde::Serialize;
use sqlx::{PgConnection, PgPool};
use time::{Date, OffsetDateTime, Time};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::days::{self, TripDay};
use crate::error::{AppError, AppResult};
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
    /// Retiré à la main (reste hors itinéraire quand on régénère).
    Removed,
    /// Ajouté après la génération de l'itinéraire.
    AddedAfter,
}

impl UnscheduledReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::ClosedAllDays => "closed_all_days",
            Self::OpeningHours => "opening_hours",
            Self::NoTime => "no_time",
            Self::Removed => "removed",
            Self::AddedAfter => "added_after",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "closed_all_days" => Self::ClosedAllDays,
            "opening_hours" => Self::OpeningHours,
            "no_time" => Self::NoTime,
            "removed" => Self::Removed,
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

/// Contrainte qu'une étape placée ou verrouillée à la main ne respecte pas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItineraryIssue {
    /// Fermé à l'heure prévue, ou pas assez longtemps pour la visite.
    OutsideOpeningHours,
    /// La visite se termine après la fin de la journée.
    AfterDayEnd,
}

impl ItineraryIssue {
    fn as_str(self) -> &'static str {
        match self {
            Self::OutsideOpeningHours => "outside_opening_hours",
            Self::AfterDayEnd => "after_day_end",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "outside_opening_hours" => Self::OutsideOpeningHours,
            "after_day_end" => Self::AfterDayEnd,
            _ => return None,
        })
    }
}

impl From<StopIssue> for ItineraryIssue {
    fn from(i: StopIssue) -> Self {
        match i {
            StopIssue::OutsideOpeningHours => Self::OutsideOpeningHours,
            StopIssue::AfterDayEnd => Self::AfterDayEnd,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UnscheduledPoi {
    pub poi_id: Uuid,
    pub reason: UnscheduledReason,
}

/// Étape de la journée. Les heures sont au format `HH:MM` (heure locale du voyage ;
/// au-delà de `24:00` si une journée chargée à la main déborde après minuit).
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
    /// Verrouillée : garde son jour et son ordre quand on régénère.
    pub locked: bool,
    pub issue: Option<ItineraryIssue>,
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
    /// La journée se termine après `day_end`.
    pub overtime: bool,
    /// Lieux, réglages ou dates modifiés depuis le calcul de ce jour : à recalculer.
    pub stale: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Itinerary {
    /// À renvoyer avec chaque ajustement (verrouillage optimiste).
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

fn location(p: &Poi) -> LatLon {
    LatLon::new(p.lat, p.lon)
}

fn modes_of(day: &TripDay) -> Vec<Mode> {
    day.modes.iter().map(|&m| Mode::from(m)).collect()
}

// --- Contexte de calcul ------------------------------------------------------------------

/// État du voyage lu au début d'un calcul, dans la transaction.
struct Context {
    trip: Trip,
    days: Vec<TripDay>,
    bases: Vec<(Option<Uuid>, Option<Uuid>)>,
    /// Hébergements servant de départ ou d'arrivée à au moins un jour.
    base_ids: HashSet<Uuid>,
    pois: HashMap<Uuid, Poi>,
    /// Instant de lecture : un lieu modifié après sera signalé comme changé.
    read_at: OffsetDateTime,
}

impl Context {
    async fn load(conn: &mut PgConnection, trip_id: Uuid, user_id: Uuid) -> AppResult<Self> {
        // Générations et ajustements concurrents d'un même voyage s'enchaînent. Verrou
        // consultatif plutôt que `FOR UPDATE` : les appels au moteur de routage ont lieu dans
        // la transaction et ne doivent pas bloquer les autres écritures du voyage (une
        // modification concurrente rend simplement l'itinéraire périmé).
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('itinerary:' || $1::text, 0))")
            .bind(trip_id)
            .execute(&mut *conn)
            .await?;
        let read_at: OffsetDateTime = sqlx::query_scalar("SELECT now()")
            .fetch_one(&mut *conn)
            .await?;
        let trip = trips::get_for_user(&mut *conn, trip_id, user_id).await?;
        let days = days::list(&mut *conn, trip_id).await?;
        let pois = pois::list(
            &mut *conn,
            trip_id,
            user_id,
            (trip.start_date, trip.end_date),
        )
        .await?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
        let bases = day_bases(&days);
        Ok(Self {
            base_ids: bases.iter().flat_map(|&(s, e)| [s, e]).flatten().collect(),
            bases,
            trip,
            days,
            pois,
            read_at,
        })
    }

    /// Lieu que l'on peut visiter (ni hébergement, ni supprimé).
    fn visitable(&self, id: Uuid) -> Option<&Poi> {
        self.pois
            .get(&id)
            .filter(|p| p.category != Category::Lodging && !self.base_ids.contains(&id))
    }

    fn day_index(&self, date: Date) -> Option<usize> {
        self.days.iter().position(|d| d.date == date)
    }

    fn place(&self, id: Option<Uuid>) -> Option<LatLon> {
        id.and_then(|id| self.pois.get(&id)).map(location)
    }

    /// Plages d'ouverture d'un lieu pour chaque jour (`None` : horaires inconnus).
    fn windows(&self, p: &Poi) -> Option<Vec<Vec<plan::Window>>> {
        match opening::parse(p.opening_hours.as_deref(), p.country_code.as_deref()) {
            opening::Parsed::Valid(oh) => Some(
                self.days
                    .iter()
                    .map(|d| opening::day_windows(&oh, d.date))
                    .collect(),
            ),
            _ => None,
        }
    }

    fn duration_s(p: &Poi) -> u32 {
        p.duration_min.max(0) as u32 * 60
    }
}

/// Une journée prête à être ordonnée ou minutée : visites, nœuds et trajets.
struct DayInputs {
    visits: Vec<Visit>,
    matrix: TravelMatrix,
    start: Option<usize>,
    end: Option<usize>,
    day_start_s: u32,
    day_end_s: u32,
}

impl DayInputs {
    /// `members` : lieux de la journée ; les nœuds `0..members.len()` sont ces lieux,
    /// suivis du départ et de l'arrivée (un seul nœud s'ils sont identiques).
    async fn build(ctx: &Context, routing: &RoutingService, d: usize, members: &[&Poi]) -> Self {
        let (start, end) = ctx.bases[d];
        let visits = members
            .iter()
            .map(|p| Visit {
                duration_s: Context::duration_s(p),
                score: plan::score(p.rating.average),
                windows: ctx.windows(p).map(|mut w| w.swap_remove(d)),
            })
            .collect();
        let mut nodes: Vec<LatLon> = members.iter().map(|p| location(p)).collect();
        let mut push = |place: Option<LatLon>| {
            nodes.push(place?);
            Some(nodes.len() - 1)
        };
        let start_node = push(ctx.place(start));
        let end_node = if end == start {
            start_node
        } else {
            push(ctx.place(end))
        };
        let matrix = travel_matrix(routing, &modes_of(&ctx.days[d]), &nodes).await;
        Self {
            visits,
            matrix,
            start: start_node,
            end: end_node,
            day_start_s: seconds(ctx.trip.day_start),
            day_end_s: seconds(ctx.trip.day_end),
        }
    }

    fn problem<'a>(&'a self, fixed: &'a [usize]) -> DayProblem<'a> {
        DayProblem {
            day_start_s: self.day_start_s,
            day_end_s: self.day_end_s,
            visits: &self.visits,
            start: self.start,
            end: self.end,
            travel: &self.matrix,
            fixed,
        }
    }
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
    // Modes routés en parallèle ; les transports en commun ensuite, dérivés de la matrice
    // voiture (alors en cache si la voiture fait partie des modes du jour).
    let run = |mode: Mode| async move {
        if modes.contains(&mode) {
            Some(routing.matrix(mode, nodes).await)
        } else {
            None
        }
    };
    let (walk, bike, car) = tokio::join!(run(Mode::Walk), run(Mode::Bike), run(Mode::Car));
    let transit = run(Mode::Transit).await;
    let mut grids = [
        (Mode::Walk, walk),
        (Mode::Bike, bike),
        (Mode::Car, car),
        (Mode::Transit, transit),
    ];
    let per_mode: Vec<(Mode, Vec<Vec<Leg>>)> = modes
        .iter()
        .filter_map(|&mode| {
            let (_, grid) = grids.iter_mut().find(|(m, _)| *m == mode)?;
            Some((mode, grid.take()?))
        })
        .collect();
    TravelMatrix::new(nodes.len(), |i, j| {
        if i == j {
            return (fallback_mode, zero);
        }
        let options: Vec<(Mode, Leg)> = per_mode.iter().map(|(m, grid)| (*m, grid[i][j])).collect();
        travel::choose_mode(&options).unwrap_or((fallback_mode, zero))
    })
}

// --- Écriture ----------------------------------------------------------------------------

struct PlannedStop {
    poi_id: Uuid,
    stop: plan::Stop,
    locked: bool,
}

/// Journée calculée, prête à être enregistrée.
struct PlannedDay {
    date: Date,
    modes: Vec<TransportMode>,
    start: Option<Uuid>,
    end: Option<Uuid>,
    stops: Vec<PlannedStop>,
    back: Option<(Mode, Leg)>,
}

impl PlannedDay {
    fn new(
        ctx: &Context,
        d: usize,
        plan: plan::DayPlan,
        ids: &[Uuid],
        locked: &HashSet<Uuid>,
    ) -> Self {
        let (start, end) = ctx.bases[d];
        Self {
            date: ctx.days[d].date,
            modes: ctx.days[d].modes.clone(),
            start,
            end,
            stops: plan
                .stops
                .into_iter()
                .map(|stop| PlannedStop {
                    poi_id: ids[stop.visit],
                    locked: locked.contains(&ids[stop.visit]),
                    stop,
                })
                .collect(),
            back: plan.back,
        }
    }
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

/// Remplace une journée (et ses étapes) par celle calculée, réglages actuels compris.
async fn write_day(conn: &mut PgConnection, ctx: &Context, day: &PlannedDay) -> AppResult<()> {
    let trip_id = ctx.trip.id;
    sqlx::query("DELETE FROM itinerary_days WHERE trip_id = $1 AND date = $2")
        .bind(trip_id)
        .bind(day.date)
        .execute(&mut *conn)
        .await?;
    let modes: Vec<&str> = day.modes.iter().map(|m| m.as_str()).collect();
    let (back_mode, back_duration, back_distance, back_source) = travel_columns(day.back);
    sqlx::query(
        "INSERT INTO itinerary_days (trip_id, date, modes, start_poi_id, end_poi_id,
             day_start, day_end, computed_at, stop_count,
             back_mode, back_duration_s, back_distance_m, back_source)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(trip_id)
    .bind(day.date)
    .bind(&modes)
    .bind(day.start)
    .bind(day.end)
    .bind(ctx.trip.day_start)
    .bind(ctx.trip.day_end)
    .bind(ctx.read_at)
    .bind(day.stops.len() as i16)
    .bind(back_mode)
    .bind(back_duration)
    .bind(back_distance)
    .bind(back_source)
    .execute(&mut *conn)
    .await?;
    for (position, s) in day.stops.iter().enumerate() {
        let (mode, duration, distance, source) = travel_columns(s.stop.travel);
        sqlx::query(
            "INSERT INTO itinerary_stops (trip_id, date, position, poi_id, arrival_s, start_s, end_s,
                 travel_mode, travel_duration_s, travel_distance_m, travel_source, locked, issue)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(trip_id)
        .bind(day.date)
        .bind(position as i16)
        .bind(s.poi_id)
        .bind(s.stop.arrival_s as i32)
        .bind(s.stop.start_s as i32)
        .bind(s.stop.end_s as i32)
        .bind(mode)
        .bind(duration)
        .bind(distance)
        .bind(source)
        .bind(s.locked)
        .bind(s.stop.issue.map(|i| ItineraryIssue::from(i).as_str()))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn set_unscheduled(
    conn: &mut PgConnection,
    trip_id: Uuid,
    poi_id: Uuid,
    reason: Option<UnscheduledReason>,
) -> AppResult<()> {
    match reason {
        Some(reason) => {
            sqlx::query(
                "INSERT INTO itinerary_unscheduled (trip_id, poi_id, reason) VALUES ($1, $2, $3)
                 ON CONFLICT (trip_id, poi_id) DO UPDATE SET reason = EXCLUDED.reason",
            )
            .bind(trip_id)
            .bind(poi_id)
            .bind(reason.as_str())
            .execute(&mut *conn)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM itinerary_unscheduled WHERE trip_id = $1 AND poi_id = $2")
                .bind(trip_id)
                .bind(poi_id)
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

// --- Génération --------------------------------------------------------------------------

/// Génère l'itinéraire complet du voyage et remplace le précédent. Les étapes
/// verrouillées gardent leur jour et leur ordre ; les lieux retirés à la main restent
/// hors itinéraire.
pub async fn generate(
    db: &PgPool,
    routing: &RoutingService,
    trip_id: Uuid,
    user_id: Uuid,
) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let ctx = Context::load(&mut tx, trip_id, user_id).await?;

    // Choix manuels de l'itinéraire précédent.
    let locked_rows: Vec<(Date, Uuid)> = sqlx::query_as(
        "SELECT date, poi_id FROM itinerary_stops WHERE trip_id = $1 AND locked
         ORDER BY date, position",
    )
    .bind(trip_id)
    .fetch_all(&mut *tx)
    .await?;
    let removed: HashSet<Uuid> = sqlx::query_scalar(
        "SELECT poi_id FROM itinerary_unscheduled WHERE trip_id = $1 AND reason = 'removed'",
    )
    .bind(trip_id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let pinned: HashMap<Uuid, usize> = locked_rows
        .iter()
        .filter(|(_, id)| ctx.visitable(*id).is_some())
        .filter_map(|(date, id)| Some((*id, ctx.day_index(*date)?)))
        .collect();
    let locked: HashSet<Uuid> = pinned.keys().copied().collect();

    // Ordre stable : ordre d'ajout des lieux (celui de `pois::list`).
    let mut visitable: Vec<&Poi> = ctx
        .pois
        .values()
        .filter(|p| ctx.visitable(p.id).is_some() && !removed.contains(&p.id))
        .collect();
    visitable.sort_by_key(|p| (p.created_at, p.id));

    let budget = seconds(ctx.trip.day_end).saturating_sub(seconds(ctx.trip.day_start));
    let candidates: Vec<Candidate> = visitable
        .iter()
        .map(|p| Candidate {
            location: location(p),
            duration_s: Context::duration_s(p),
            score: plan::score(p.rating.average),
            closed: match ctx.windows(p) {
                Some(per_day) => per_day.iter().map(Vec::is_empty).collect(),
                None => vec![false; ctx.days.len()],
            },
            pinned: pinned.get(&p.id).copied(),
        })
        .collect();
    let slots: Vec<DaySlot> = ctx
        .days
        .iter()
        .zip(&ctx.bases)
        .map(|(day, &(start, end))| {
            let mode = plan::fastest_mode(&modes_of(day));
            // Changement d'hébergement : le trajet entre les deux est pris sur la journée.
            let transfer = match (ctx.place(start), ctx.place(end)) {
                (Some(a), Some(b)) => travel::straight_line(mode, a, b).duration_s,
                _ => 0,
            };
            DaySlot {
                anchor: ctx.place(end).or(ctx.place(start)),
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
        .chain(removed.iter().map(|&id| (id, UnscheduledReason::Removed)))
        .collect();
    let mut planned = Vec::with_capacity(ctx.days.len());
    for d in 0..ctx.days.len() {
        let members: Vec<&Poi> = assignment.days[d].iter().map(|&i| visitable[i]).collect();
        let ids: Vec<Uuid> = members.iter().map(|p| p.id).collect();
        // Étapes verrouillées de ce jour, dans leur ordre précédent.
        let fixed: Vec<usize> = locked_rows
            .iter()
            .filter(|(date, _)| *date == ctx.days[d].date)
            .filter_map(|(_, id)| ids.iter().position(|x| x == id))
            .collect();
        let inputs = DayInputs::build(&ctx, routing, d, &members).await;
        let day_plan = plan::schedule_day(&inputs.problem(&fixed));
        unscheduled.extend(
            day_plan
                .unscheduled
                .iter()
                .map(|&(v, r)| (ids[v], r.into())),
        );
        planned.push(PlannedDay::new(&ctx, d, day_plan, &ids, &locked));
    }

    let version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM itineraries WHERE trip_id = $1), 0) + 1",
    )
    .bind(trip_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM itineraries WHERE trip_id = $1")
        .bind(trip_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO itineraries (trip_id, version, generated_at, generated_by) VALUES ($1, $2, $3, $4)",
    )
    .bind(trip_id)
    .bind(version)
    .bind(ctx.read_at)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    for day in &planned {
        write_day(&mut tx, &ctx, day).await?;
    }
    for &(poi_id, reason) in &unscheduled {
        set_unscheduled(&mut tx, trip_id, poi_id, Some(reason)).await?;
    }
    tx.commit().await?;
    tracing::info!(%trip_id, version, "itinéraire généré");
    Ok(())
}

// --- Ajustements manuels -------------------------------------------------------------------

/// Modification manuelle de l'itinéraire.
#[derive(Debug, Clone, Copy)]
pub enum Adjustment {
    /// Place un lieu à la position donnée d'un jour (réordonner, changer de jour, ajouter un
    /// lieu hors itinéraire) ; `date: None` le retire de l'itinéraire.
    Move {
        poi_id: Uuid,
        date: Option<Date>,
        position: usize,
    },
    /// Verrouille ou déverrouille une étape (sans recalcul).
    Lock { poi_id: Uuid, locked: bool },
    /// Recalcule une journée dans son ordre actuel, avec les lieux et réglages actuels.
    Recompute { date: Date },
}

/// Applique un ajustement si `version` est celle de l'itinéraire, puis recalcule les
/// horaires des seuls jours touchés (dans l'ordre choisi, contraintes signalées).
pub async fn adjust(
    db: &PgPool,
    routing: &RoutingService,
    trip_id: Uuid,
    user_id: Uuid,
    version: i64,
    adjustment: Adjustment,
) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let ctx = Context::load(&mut tx, trip_id, user_id).await?;
    let current: Option<i64> =
        sqlx::query_scalar("SELECT version FROM itineraries WHERE trip_id = $1")
            .bind(trip_id)
            .fetch_optional(&mut *tx)
            .await?;
    match current {
        None => return Err(AppError::BadRequest("générez d'abord l'itinéraire".into())),
        Some(v) if v != version => {
            return Err(AppError::Conflict(
                "l'itinéraire a été modifié entre-temps, rechargez-le".into(),
            ))
        }
        Some(_) => {}
    }

    // Étapes actuelles, jour par jour : (lieu, verrouillé).
    let rows: Vec<(Date, Uuid, bool)> = sqlx::query_as(
        "SELECT date, poi_id, locked FROM itinerary_stops WHERE trip_id = $1 ORDER BY date, position",
    )
    .bind(trip_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut stops: HashMap<Date, Vec<(Uuid, bool)>> = HashMap::new();
    for (date, poi_id, locked) in rows {
        stops.entry(date).or_default().push((poi_id, locked));
    }
    let find = |stops: &HashMap<Date, Vec<(Uuid, bool)>>, poi_id: Uuid| {
        stops.iter().find_map(|(date, list)| {
            list.iter()
                .position(|(id, _)| *id == poi_id)
                .map(|i| (*date, i))
        })
    };

    let mut touched: Vec<Date> = vec![];
    match adjustment {
        Adjustment::Move {
            poi_id,
            date,
            position,
        } => {
            if ctx.visitable(poi_id).is_none() {
                return Err(if ctx.pois.contains_key(&poi_id) {
                    AppError::BadRequest(
                        "un hébergement n'est pas une étape de l'itinéraire".into(),
                    )
                } else {
                    AppError::NotFound
                });
            }
            if date.is_some_and(|d| ctx.day_index(d).is_none()) {
                return Err(days::not_in_trip());
            }
            let mut locked = false;
            if let Some((from, i)) = find(&stops, poi_id) {
                let list = stops.get_mut(&from).expect("jour trouvé");
                locked = list.remove(i).1;
                if ctx.day_index(from).is_some() {
                    touched.push(from);
                } else {
                    // Jour sorti des dates du voyage : il n'est plus recalculé (il reste
                    // signalé), on y retire seulement l'étape.
                    sqlx::query("DELETE FROM itinerary_stops WHERE trip_id = $1 AND poi_id = $2")
                        .bind(trip_id)
                        .bind(poi_id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            match date {
                Some(to) => {
                    let list = stops.entry(to).or_default();
                    list.insert(position.min(list.len()), (poi_id, locked));
                    if !touched.contains(&to) {
                        touched.push(to);
                    }
                    set_unscheduled(&mut tx, trip_id, poi_id, None).await?;
                }
                None => {
                    set_unscheduled(&mut tx, trip_id, poi_id, Some(UnscheduledReason::Removed))
                        .await?
                }
            }
        }
        Adjustment::Lock { poi_id, locked } => {
            let (date, _) = find(&stops, poi_id).ok_or(AppError::NotFound)?;
            sqlx::query(
                "UPDATE itinerary_stops SET locked = $3 WHERE trip_id = $1 AND poi_id = $2",
            )
            .bind(trip_id)
            .bind(poi_id)
            .bind(locked)
            .execute(&mut *tx)
            .await?;
            tracing::debug!(%trip_id, %poi_id, %date, locked, "étape (dé)verrouillée");
        }
        Adjustment::Recompute { date } => {
            ctx.day_index(date).ok_or_else(days::not_in_trip)?;
            touched.push(date);
        }
    }

    for date in touched {
        // Seuls des jours du voyage sont touchés (dates vérifiées ci-dessus).
        let Some(d) = ctx.day_index(date) else {
            continue;
        };
        // Les lieux devenus hébergements (ou supprimés) quittent la journée.
        let list: Vec<(Uuid, bool)> = stops
            .remove(&date)
            .unwrap_or_default()
            .into_iter()
            .filter(|(id, _)| ctx.visitable(*id).is_some())
            .collect();
        let members: Vec<&Poi> = list
            .iter()
            .filter_map(|(id, _)| ctx.visitable(*id))
            .collect();
        let ids: Vec<Uuid> = list.iter().map(|(id, _)| *id).collect();
        let locked: HashSet<Uuid> = list.iter().filter(|(_, l)| *l).map(|(id, _)| *id).collect();
        let inputs = DayInputs::build(&ctx, routing, d, &members).await;
        let order: Vec<usize> = (0..members.len()).collect();
        let day_plan = plan::time_day(&inputs.problem(&[]), &order);
        write_day(
            &mut tx,
            &ctx,
            &PlannedDay::new(&ctx, d, day_plan, &ids, &locked),
        )
        .await?;
    }
    sqlx::query("UPDATE itineraries SET version = version + 1 WHERE trip_id = $1")
        .bind(trip_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!(%trip_id, ?adjustment, "itinéraire ajusté");
    Ok(())
}

// --- Lecture -----------------------------------------------------------------------------

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
    issue: Option<String>,
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
                travel_duration_s, travel_distance_m, travel_source, locked, issue
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
                overtime: false,
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
                    issue: s.issue.as_deref().and_then(ItineraryIssue::parse),
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
        let finish = last_end + back.as_ref().map_or(0, |b| b.duration_s);
        out_days.push(ItineraryDay {
            date: current.date,
            modes: parse_modes(&row.modes),
            start_poi_id: row.start_poi_id,
            end_poi_id: row.end_poi_id,
            day_start: hhmm::format(row.day_start),
            day_end: hhmm::format(row.day_end),
            finish: clock(finish),
            overtime: finish > seconds(row.day_end),
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
        assert_eq!(clock(90_000), "25:00");
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
