//! Génération d'itinéraire, en deux temps.
//!
//! 1. [`assign_days`] répartit les lieux entre les jours par regroupement géographique
//!    (vol d'oiseau, aucun appel au moteur de routage) : centres fixés par les hébergements,
//!    les autres choisis par la méthode du point le plus éloigné puis affinés (k-moyennes).
//!    Les lieux sont placés par ordre de note du groupe, dans la limite d'un budget de temps
//!    estimé par jour et en évitant les jours où ils sont fermés.
//! 2. [`schedule_day`] ordonne une journée avec de vrais temps de trajet : insertion des lieux
//!    les mieux notés d'abord, à la position qui termine la journée le plus tôt en respectant
//!    les horaires d'ouverture, puis amélioration 2-opt du temps de trajet total.
//!
//! Tout est déterministe : mêmes entrées, même itinéraire.

use serde::Serialize;

use crate::geo::{haversine_m, LatLon};
use crate::travel::{self, Leg, Mode};

/// Note donnée à un lieu que personne n'a encore noté (milieu de l'échelle 1–5).
pub const NEUTRAL_SCORE: f64 = 3.0;

/// Note utilisée par le planificateur : la moyenne du groupe, sinon une note neutre.
pub fn score(average: Option<f64>) -> f64 {
    average.unwrap_or(NEUTRAL_SCORE)
}

/// Plage d'ouverture d'une journée, en secondes depuis minuit (`fin` exclue).
pub type Window = (u32, u32);

/// Raison pour laquelle un lieu ne figure pas dans l'itinéraire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unscheduled {
    /// Fermé toute la journée à chaque date du voyage.
    ClosedAllDays,
    /// Ses horaires ne laissent pas le temps de la visite dans la plage de la journée.
    OpeningHours,
    /// Plus de temps disponible (les lieux mieux notés passent d'abord).
    NoTime,
}

// --- Étape 1 : répartition par jour ---------------------------------------------------

/// Lieu à placer.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub location: LatLon,
    pub duration_s: u32,
    pub score: f64,
    /// Pour chaque jour : le lieu est-il fermé toute la journée ?
    pub closed: Vec<bool>,
    /// Jour imposé (étape verrouillée) : placé là quoi qu'il arrive.
    pub pinned: Option<usize>,
}

/// Jour du voyage, vu par la répartition.
#[derive(Debug, Clone)]
pub struct DaySlot {
    /// Hébergement du jour : centre fixe du regroupement.
    pub anchor: Option<LatLon>,
    /// Temps disponible pour les visites et les trajets.
    pub budget_s: u32,
    /// Mode servant à estimer les trajets (le plus rapide autorisé ce jour-là).
    pub mode: Mode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    /// Indices des lieux de chaque jour.
    pub days: Vec<Vec<usize>>,
    pub unassigned: Vec<(usize, Unscheduled)>,
}

/// Mode le plus rapide parmi ceux autorisés (estimation des trajets à l'étape 1).
pub fn fastest_mode(modes: &[Mode]) -> Mode {
    [Mode::Car, Mode::Transit, Mode::Bike, Mode::Walk]
        .into_iter()
        .find(|m| modes.contains(m))
        .unwrap_or(Mode::Walk)
}

/// Nombre maximal d'itérations des k-moyennes.
const MAX_ITERATIONS: usize = 20;
/// En deçà de cet écart (mètres), deux jours sont considérés aussi proches d'un lieu.
const SAME_PLACE_M: f64 = 1_000.0;
/// Les trajets estimés à vol d'oiseau sont majorés (détours, attente, stationnement).
const TRAVEL_MARGIN: f64 = 1.2;

/// Répartit les lieux entre les jours (voir le module).
pub fn assign_days(cands: &[Candidate], days: &[DaySlot]) -> Assignment {
    if days.is_empty() {
        return Assignment {
            days: vec![],
            unassigned: (0..cands.len()).map(|i| (i, Unscheduled::NoTime)).collect(),
        };
    }
    // Priorité : meilleure note, puis visite la plus courte, puis ordre d'ajout.
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|&a, &b| {
        cands[b]
            .score
            .total_cmp(&cands[a].score)
            .then(cands[a].duration_s.cmp(&cands[b].duration_s))
            .then(a.cmp(&b))
    });

    let mut centers = initial_centers(cands, days);
    let mut result = fill(cands, days, &centers, &order);
    for _ in 0..MAX_ITERATIONS {
        let next_centers: Vec<LatLon> = days
            .iter()
            .enumerate()
            .map(|(d, day)| {
                day.anchor
                    .or_else(|| centroid(result.days[d].iter().map(|&i| cands[i].location)))
                    .unwrap_or(centers[d])
            })
            .collect();
        if next_centers == centers {
            break;
        }
        centers = next_centers;
        let next = fill(cands, days, &centers, &order);
        if next == result {
            break;
        }
        result = next;
    }
    reorder_free_days(cands, days, &mut result);
    result
}

fn centroid(points: impl Iterator<Item = LatLon>) -> Option<LatLon> {
    let (mut lat, mut lon, mut n) = (0.0, 0.0, 0usize);
    for p in points {
        lat += p.lat;
        lon += p.lon;
        n += 1;
    }
    (n > 0).then(|| LatLon::new(lat / n as f64, lon / n as f64))
}

/// Centres de départ : les hébergements, puis, pour les jours libres, le lieu le plus
/// éloigné des centres déjà choisis (le mieux noté s'il n'y a encore aucun centre).
fn initial_centers(cands: &[Candidate], days: &[DaySlot]) -> Vec<LatLon> {
    let mut chosen: Vec<LatLon> = days.iter().filter_map(|d| d.anchor).collect();
    let fallback = centroid(cands.iter().map(|c| c.location)).unwrap_or(LatLon::new(0.0, 0.0));
    days.iter()
        .map(|day| {
            if let Some(a) = day.anchor {
                return a;
            }
            let next = if chosen.is_empty() {
                cands
                    .iter()
                    .enumerate()
                    .max_by(|(ia, a), (ib, b)| a.score.total_cmp(&b.score).then(ib.cmp(ia)))
                    .map(|(_, c)| c.location)
            } else {
                cands
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let d = chosen
                            .iter()
                            .map(|&p| haversine_m(p, c.location))
                            .fold(f64::INFINITY, f64::min);
                        (i, d, c.location)
                    })
                    .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                    .map(|(_, _, loc)| loc)
            };
            let center = next.unwrap_or(fallback);
            chosen.push(center);
            center
        })
        .collect()
}

/// Coût estimé d'un lieu pour un jour : visite + trajet depuis le centre du jour.
fn estimated_cost(c: &Candidate, center: LatLon, mode: Mode) -> u32 {
    let travel = travel::straight_line(mode, center, c.location).duration_s;
    c.duration_s + (f64::from(travel) * TRAVEL_MARGIN).round() as u32
}

/// Place chaque lieu, par priorité, dans le jour ouvert le plus proche qui a encore le temps.
fn fill(cands: &[Candidate], days: &[DaySlot], centers: &[LatLon], order: &[usize]) -> Assignment {
    let mut remaining: Vec<u32> = days.iter().map(|d| d.budget_s).collect();
    let mut out = Assignment {
        days: vec![vec![]; days.len()],
        unassigned: vec![],
    };
    // Les lieux verrouillés d'abord, sur leur jour, même au-delà du budget.
    for &i in order {
        if let Some(d) = cands[i].pinned.filter(|&d| d < days.len()) {
            remaining[d] =
                remaining[d].saturating_sub(estimated_cost(&cands[i], centers[d], days[d].mode));
            out.days[d].push(i);
        }
    }
    for &i in order {
        let c = &cands[i];
        if c.pinned.is_some_and(|d| d < days.len()) {
            continue;
        }
        if (0..days.len()).all(|d| c.closed.get(d).copied().unwrap_or(false)) {
            out.unassigned.push((i, Unscheduled::ClosedAllDays));
            continue;
        }
        // Le jour le plus proche ; à distance quasi égale (même hébergement plusieurs nuits),
        // celui qui a le plus de temps libre, pour équilibrer les journées.
        let best = (0..days.len())
            .filter(|&d| !c.closed.get(d).copied().unwrap_or(false))
            .map(|d| (d, estimated_cost(c, centers[d], days[d].mode)))
            .filter(|&(d, cost)| cost <= remaining[d])
            .min_by(|a, b| {
                let (da, db) = (
                    haversine_m(centers[a.0], c.location),
                    haversine_m(centers[b.0], c.location),
                );
                if (da - db).abs() < SAME_PLACE_M {
                    remaining[b.0].cmp(&remaining[a.0]).then(a.0.cmp(&b.0))
                } else {
                    da.total_cmp(&db)
                }
            });
        match best {
            Some((d, cost)) => {
                remaining[d] -= cost;
                out.days[d].push(i);
            }
            None => out.unassigned.push((i, Unscheduled::NoTime)),
        }
    }
    for day in &mut out.days {
        day.sort_unstable();
    }
    out.unassigned.sort_unstable_by_key(|&(i, _)| i);
    out
}

/// Ordonne les groupes des jours sans hébergement pour éviter les allers-retours : chaque
/// jour libre reprend, parmi les groupes libres restants, le plus proche de la veille
/// (à condition que tous ses lieux soient ouverts ce jour-là).
fn reorder_free_days(cands: &[Candidate], days: &[DaySlot], result: &mut Assignment) {
    let free: Vec<usize> = (0..days.len())
        .filter(|&d| days[d].anchor.is_none())
        .collect();
    if free.len() < 2 {
        return;
    }
    let groups: Vec<Vec<usize>> = free.iter().map(|&d| result.days[d].clone()).collect();
    let center = |g: &Vec<usize>| centroid(g.iter().map(|&i| cands[i].location));
    let fits = |g: &Vec<usize>, d: usize| {
        g.iter().all(|&i| {
            !cands[i].closed.get(d).copied().unwrap_or(false)
                && cands[i].pinned.is_none_or(|p| p == d)
        }) && g.iter().map(|&i| cands[i].duration_s).sum::<u32>() <= days[d].budget_s
    };

    let mut taken = vec![false; groups.len()];
    let mut prev: Option<LatLon> = None;
    let mut new_days = result.days.clone();
    for d in 0..days.len() {
        if let Some(a) = days[d].anchor {
            prev = Some(a);
            continue;
        }
        let pick = (0..groups.len())
            .filter(|&g| !taken[g] && fits(&groups[g], d))
            .min_by(|&a, &b| {
                let dist = |g: usize| match (prev, center(&groups[g])) {
                    (Some(p), Some(c)) => haversine_m(p, c),
                    // Sans repère, ou groupe vide : on garde l'ordre d'origine.
                    _ => g as f64 * 1e-9,
                };
                dist(a).total_cmp(&dist(b)).then(a.cmp(&b))
            });
        let Some(g) = pick else {
            // Aucun groupe compatible : on renonce à réordonner.
            return;
        };
        taken[g] = true;
        if let Some(c) = center(&groups[g]) {
            prev = Some(c);
        }
        new_days[d] = groups[g].clone();
    }
    result.days = new_days;
}

// --- Étape 2 : ordre et horaires d'une journée ----------------------------------------

/// Lieu à visiter dans la journée.
#[derive(Debug, Clone)]
pub struct Visit {
    pub duration_s: u32,
    pub score: f64,
    /// Plages d'ouverture du jour ; `None` : horaires inconnus, considéré ouvert.
    pub windows: Option<Vec<Window>>,
}

/// Trajets entre les nœuds d'une journée, mode déjà choisi pour chaque paire.
#[derive(Debug, Clone)]
pub struct TravelMatrix {
    size: usize,
    cells: Vec<(Mode, Leg)>,
}

impl TravelMatrix {
    /// `cell(i, j)` : trajet du nœud `i` au nœud `j`.
    pub fn new(size: usize, mut cell: impl FnMut(usize, usize) -> (Mode, Leg)) -> Self {
        let mut cells = Vec::with_capacity(size * size);
        for i in 0..size {
            for j in 0..size {
                cells.push(cell(i, j));
            }
        }
        Self { size, cells }
    }

    pub fn get(&self, from: usize, to: usize) -> (Mode, Leg) {
        self.cells[from * self.size + to]
    }
}

/// Une journée à planifier. Les nœuds `0..visits.len()` sont les visites ; `start` et `end`
/// (hébergements) sont d'autres nœuds de la matrice.
#[derive(Debug, Clone)]
pub struct DayProblem<'a> {
    pub day_start_s: u32,
    pub day_end_s: u32,
    pub visits: &'a [Visit],
    pub start: Option<usize>,
    pub end: Option<usize>,
    pub travel: &'a TravelMatrix,
    /// Visites verrouillées, dans l'ordre à conserver : toujours gardées, quitte à ne pas
    /// respecter une contrainte (signalée).
    pub fixed: &'a [usize],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub visit: usize,
    /// Trajet depuis l'étape précédente (ou l'hébergement) ; `None` pour une première visite
    /// sans point de départ.
    pub travel: Option<(Mode, Leg)>,
    pub arrival_s: u32,
    /// Début de la visite (après une éventuelle attente de l'ouverture).
    pub start_s: u32,
    pub end_s: u32,
    /// Contrainte non respectée (étape placée ou verrouillée à la main).
    pub issue: Option<StopIssue>,
}

/// Contrainte qu'une étape imposée à la main ne respecte pas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopIssue {
    /// Fermé à l'heure prévue, ou pas assez longtemps pour la visite.
    OutsideOpeningHours,
    /// La visite se termine après la fin de la journée.
    AfterDayEnd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DayPlan {
    pub stops: Vec<Stop>,
    /// Retour à l'hébergement du soir.
    pub back: Option<(Mode, Leg)>,
    /// Heure de fin de la journée (arrivée à l'hébergement, ou fin de la dernière visite).
    pub finish_s: u32,
    pub total_travel_s: u32,
    /// La journée se termine après l'heure de fin prévue.
    pub overtime: bool,
    pub unscheduled: Vec<(usize, Unscheduled)>,
}

/// Début de visite au plus tôt après `arrival`, dans une plage d'ouverture et la journée.
fn earliest_start(arrival: u32, v: &Visit, day_end: u32) -> Option<u32> {
    match &v.windows {
        None => (arrival + v.duration_s <= day_end).then_some(arrival),
        Some(windows) => windows.iter().find_map(|&(open, close)| {
            let start = arrival.max(open);
            (start + v.duration_s <= close.min(day_end)).then_some(start)
        }),
    }
}

/// Simule une journée dans l'ordre donné. Strict : `None` si une contrainte n'est pas
/// respectée. Souple : toujours un résultat, les contraintes non respectées sont signalées
/// (visite à l'arrivée si le lieu est fermé, journée qui déborde).
fn simulate(p: &DayProblem<'_>, order: &[usize], lenient: bool) -> Option<DayPlan> {
    let mut t = p.day_start_s;
    let mut prev = p.start;
    let mut stops = Vec::with_capacity(order.len());
    let mut total_travel = 0;
    for &v in order {
        let visit = &p.visits[v];
        let travel = prev.map(|from| p.travel.get(from, v));
        let arrival = t + travel.map_or(0, |(_, l)| l.duration_s);
        total_travel += travel.map_or(0, |(_, l)| l.duration_s);
        let (start, issue) = match earliest_start(arrival, visit, p.day_end_s) {
            Some(start) => (start, None),
            None if !lenient => return None,
            None => match earliest_start(arrival, visit, u32::MAX) {
                Some(start) => (start, Some(StopIssue::AfterDayEnd)),
                None => (arrival, Some(StopIssue::OutsideOpeningHours)),
            },
        };
        let end = start + visit.duration_s;
        stops.push(Stop {
            visit: v,
            travel,
            arrival_s: arrival,
            start_s: start,
            end_s: end,
            issue,
        });
        t = end;
        prev = Some(v);
    }
    // Changement d'hébergement sans visite : le trajet entre les deux reste à faire.
    let back = match (prev, p.end) {
        (Some(from), Some(to)) if from != to => Some(p.travel.get(from, to)),
        _ => None,
    };
    let finish = t + back.map_or(0, |(_, l)| l.duration_s);
    total_travel += back.map_or(0, |(_, l)| l.duration_s);
    let overtime = finish > p.day_end_s;
    (lenient || !overtime).then_some(DayPlan {
        stops,
        back,
        finish_s: finish,
        total_travel_s: total_travel,
        overtime,
        unscheduled: vec![],
    })
}

/// Minute une journée dans l'ordre imposé (ajustement manuel), sans rien retirer :
/// les contraintes non respectées sont signalées sur les étapes et la journée.
pub fn time_day(p: &DayProblem<'_>, order: &[usize]) -> DayPlan {
    simulate(p, order, true).expect("la simulation souple aboutit toujours")
}

/// Nombre maximal de passes d'amélioration 2-opt.
const MAX_2OPT_PASSES: usize = 50;

/// Ordonne et minute une journée (voir le module). Les visites verrouillées
/// (`p.fixed`) sont gardées dans leur ordre ; les autres s'insèrent autour.
pub fn schedule_day(p: &DayProblem<'_>) -> DayPlan {
    let is_fixed = |v: usize| p.fixed.contains(&v);
    let mut order_by_priority: Vec<usize> = (0..p.visits.len()).filter(|&v| !is_fixed(v)).collect();
    order_by_priority.sort_by(|&a, &b| {
        p.visits[b]
            .score
            .total_cmp(&p.visits[a].score)
            .then(a.cmp(&b))
    });

    // Étapes verrouillées déjà impossibles à tenir (lieu fermé à l'heure prévue, journée
    // qui déborde) : elles restent signalées, et les autres visites s'insèrent pourvu
    // qu'elles n'ajoutent aucun problème. Sinon, toute la journée doit être tenable.
    let baseline = match simulate(p, p.fixed, false) {
        Some(_) => None,
        None => simulate(p, p.fixed, true),
    };
    let fits = |order: &[usize]| -> Option<DayPlan> {
        let Some(base) = &baseline else {
            return simulate(p, order, false);
        };
        let plan = simulate(p, order, true)?;
        let flagged_before =
            |v: usize| base.stops.iter().any(|s| s.visit == v && s.issue.is_some());
        let no_new_issue = plan
            .stops
            .iter()
            .all(|s| s.issue.is_none() || flagged_before(s.visit));
        let no_more_overtime = !plan.overtime || (base.overtime && plan.finish_s <= base.finish_s);
        (no_new_issue && no_more_overtime).then_some(plan)
    };

    let mut order: Vec<usize> = p.fixed.to_vec();
    let mut unscheduled = vec![];
    for v in order_by_priority {
        let best = (0..=order.len())
            .filter_map(|pos| {
                let mut candidate = order.clone();
                candidate.insert(pos, v);
                fits(&candidate).map(|plan| (plan.finish_s, plan.total_travel_s, pos, candidate))
            })
            .min_by_key(|&(finish, travel, pos, _)| (finish, travel, pos));
        match best {
            Some((_, _, _, candidate)) => order = candidate,
            None => {
                // Le lieu seul tient-il dans la journée ? Sinon, ce sont ses horaires.
                let alone = simulate(p, &[v], false).is_some();
                let reason = if !alone && p.visits[v].windows.is_some() {
                    Unscheduled::OpeningHours
                } else {
                    Unscheduled::NoTime
                };
                unscheduled.push((v, reason));
            }
        }
    }

    // Par construction, l'ordre retenu convient ; repli souple par sécurité.
    let Some(mut plan) = fits(&order) else {
        let mut plan = time_day(p, &order);
        plan.unscheduled = unscheduled;
        return plan;
    };
    for _ in 0..MAX_2OPT_PASSES {
        let mut improved = false;
        for i in 0..order.len() {
            for j in i + 1..order.len() {
                // Inverser deux étapes verrouillées changerait leur ordre.
                if order[i..=j].iter().filter(|&&v| is_fixed(v)).count() > 1 {
                    continue;
                }
                let mut candidate = order.clone();
                candidate[i..=j].reverse();
                if let Some(next) = fits(&candidate) {
                    if next.total_travel_s < plan.total_travel_s {
                        order = candidate;
                        plan = next;
                        improved = true;
                    }
                }
            }
        }
        if !improved {
            break;
        }
    }
    unscheduled.sort_unstable_by_key(|&(v, _)| v);
    plan.unscheduled = unscheduled;
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::travel::LegSource;

    const H: u32 = 3600;

    // --- Répartition ---

    fn cand(lat: f64, lon: f64, duration_min: u32, score: f64, days: usize) -> Candidate {
        Candidate {
            location: LatLon::new(lat, lon),
            duration_s: duration_min * 60,
            score,
            closed: vec![false; days],
            pinned: None,
        }
    }

    fn slot(anchor: Option<LatLon>) -> DaySlot {
        DaySlot {
            anchor,
            budget_s: 10 * H,
            mode: Mode::Car,
        }
    }

    const SAN_JOSE: LatLon = LatLon::new(9.93, -84.08);
    const FORTUNA: LatLon = LatLon::new(10.47, -84.64);

    /// Trois lieux autour de San José, trois autour de La Fortuna.
    fn two_regions(days: usize) -> Vec<Candidate> {
        vec![
            cand(9.93, -84.07, 60, 4.0, days),
            cand(10.46, -84.70, 120, 5.0, days),
            cand(9.94, -84.09, 60, 3.0, days),
            cand(10.49, -84.62, 90, 4.0, days),
            cand(9.92, -84.05, 60, 3.0, days),
            cand(10.43, -84.67, 60, 3.0, days),
        ]
    }

    fn day_of(a: &Assignment, i: usize) -> usize {
        a.days
            .iter()
            .position(|d| d.contains(&i))
            .expect("lieu placé")
    }

    #[test]
    fn clusters_regions_into_separate_days() {
        let a = assign_days(&two_regions(2), &[slot(None), slot(None)]);
        assert!(a.unassigned.is_empty());
        let sj = day_of(&a, 0);
        assert_eq!(day_of(&a, 2), sj);
        assert_eq!(day_of(&a, 4), sj);
        let fortuna = day_of(&a, 1);
        assert_ne!(fortuna, sj);
        assert_eq!(day_of(&a, 3), fortuna);
        assert_eq!(day_of(&a, 5), fortuna);
    }

    #[test]
    fn lodging_anchors_days() {
        let a = assign_days(
            &two_regions(2),
            &[slot(Some(FORTUNA)), slot(Some(SAN_JOSE))],
        );
        assert_eq!(a.days[0], vec![1, 3, 5]);
        assert_eq!(a.days[1], vec![0, 2, 4]);
    }

    #[test]
    fn nights_in_the_same_lodging_share_the_visits() {
        // Trois nuits au même hôtel, six lieux autour : deux par jour.
        let cands: Vec<Candidate> = (0..6)
            .map(|i| cand(10.46 + f64::from(i) * 0.005, -84.70, 120, 3.0, 3))
            .collect();
        let a = assign_days(
            &cands,
            &[
                slot(Some(FORTUNA)),
                slot(Some(FORTUNA)),
                slot(Some(FORTUNA)),
            ],
        );
        assert!(a.days.iter().all(|d| d.len() == 2), "{a:?}");
    }

    #[test]
    fn pinned_places_stay_on_their_day() {
        let mut cands = two_regions(2);
        // Lieu de San José verrouillé le jour de La Fortuna, même s'il est loin.
        cands[0].pinned = Some(0);
        cands[2].pinned = Some(0);
        let a = assign_days(&cands, &[slot(Some(FORTUNA)), slot(Some(SAN_JOSE))]);
        assert!(a.days[0].contains(&0) && a.days[0].contains(&2), "{a:?}");
        assert_eq!(day_of(&a, 4), 1);
        // Même sans hébergement (jours réordonnés), un lieu verrouillé ne change pas de jour.
        let a = assign_days(&cands, &[slot(None), slot(None)]);
        assert_eq!(day_of(&a, 0), 0);
        assert_eq!(day_of(&a, 2), 0);
    }

    #[test]
    fn closed_days_are_avoided() {
        let mut cands = two_regions(2);
        // Le lieu 0 (San José) est fermé le jour où San José est l'hébergement.
        cands[0].closed = vec![false, true];
        let a = assign_days(&cands, &[slot(Some(FORTUNA)), slot(Some(SAN_JOSE))]);
        assert_eq!(day_of(&a, 0), 0);
        cands[0].closed = vec![true, true];
        let a = assign_days(&cands, &[slot(Some(FORTUNA)), slot(Some(SAN_JOSE))]);
        assert_eq!(a.unassigned, vec![(0, Unscheduled::ClosedAllDays)]);
    }

    #[test]
    fn best_rated_places_get_the_time_first() {
        let cands = vec![
            cand(9.93, -84.07, 240, 2.0, 1),
            cand(9.931, -84.071, 240, 5.0, 1),
            cand(9.932, -84.072, 240, 4.0, 1),
        ];
        let day = DaySlot {
            anchor: Some(SAN_JOSE),
            budget_s: 9 * H,
            mode: Mode::Car,
        };
        let a = assign_days(&cands, &[day]);
        assert_eq!(a.days[0], vec![1, 2]);
        assert_eq!(a.unassigned, vec![(0, Unscheduled::NoTime)]);
    }

    #[test]
    fn free_days_follow_geography() {
        // Jour 1 à San José, jours 2 et 3 libres, jour 4 à La Fortuna : le jour 2 doit
        // reprendre le groupe proche de San José, le jour 3 celui proche de La Fortuna.
        let cands = vec![
            cand(10.30, -84.50, 60, 3.0, 4), // près de La Fortuna
            cand(10.31, -84.51, 60, 3.0, 4),
            cand(9.98, -84.15, 60, 3.0, 4), // près de San José
            cand(9.99, -84.16, 60, 3.0, 4),
        ];
        let days = [
            slot(Some(SAN_JOSE)),
            slot(None),
            slot(None),
            slot(Some(FORTUNA)),
        ];
        let a = assign_days(&cands, &days);
        let near_sj = day_of(&a, 2);
        let near_fortuna = day_of(&a, 0);
        assert!(near_sj < near_fortuna, "{a:?}");
    }

    #[test]
    fn assignment_is_deterministic() {
        let cands = two_regions(3);
        let days = [slot(None), slot(None), slot(None)];
        assert_eq!(assign_days(&cands, &days), assign_days(&cands, &days));
    }

    #[test]
    fn fastest_mode_prefers_motorized() {
        assert_eq!(fastest_mode(&[Mode::Walk, Mode::Transit]), Mode::Transit);
        assert_eq!(fastest_mode(&[Mode::Walk, Mode::Bike]), Mode::Bike);
        assert_eq!(fastest_mode(&[]), Mode::Walk);
    }

    // --- Journée ---

    /// Points sur une ligne : le trajet entre `i` et `j` dure `|x_i - x_j|` × 10 min.
    fn line_matrix(xs: &[i32]) -> TravelMatrix {
        TravelMatrix::new(xs.len(), |i, j| {
            let d = (xs[i] - xs[j]).unsigned_abs() * 600;
            (
                Mode::Car,
                Leg {
                    duration_s: d,
                    distance_m: d * 10,
                    source: LegSource::Router,
                },
            )
        })
    }

    fn visit(duration_min: u32, score: f64) -> Visit {
        Visit {
            duration_s: duration_min * 60,
            score,
            windows: None,
        }
    }

    fn order(plan: &DayPlan) -> Vec<usize> {
        plan.stops.iter().map(|s| s.visit).collect()
    }

    #[test]
    fn orders_visits_along_the_way_and_back() {
        // Hébergement en 0 (nœud 4) ; visites en 3, 1, 2, 4.
        let visits = vec![visit(30, 3.0); 4];
        let m = line_matrix(&[3, 1, 2, 4, 0]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 19 * H,
            visits: &visits,
            start: Some(4),
            end: Some(4),
            travel: &m,
            fixed: &[],
        };
        let plan = schedule_day(&p);
        // Aller simple puis retour : 4 × 10 min aller + 4 × 10 min retour.
        assert_eq!(plan.total_travel_s, 80 * 60);
        // Plusieurs ordres sont optimaux ; le premier arrêt est à la bonne heure.
        let first = &plan.stops[0];
        let first_x = [3, 1, 2, 4][first.visit];
        assert_eq!(first.arrival_s, 9 * H + 600 * first_x);
        assert!(plan.back.is_some());
        assert_eq!(plan.finish_s, 9 * H + 80 * 60 + 4 * 30 * 60);
    }

    #[test]
    fn waits_for_opening_and_respects_closing() {
        let mut museum = visit(60, 3.0);
        museum.windows = Some(vec![(10 * H, 12 * H), (14 * H, 17 * H)]);
        let visits = vec![museum, visit(60, 3.0)];
        let m = line_matrix(&[0, 0]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 19 * H,
            visits: &visits,
            start: None,
            end: None,
            travel: &m,
            fixed: &[],
        };
        let plan = schedule_day(&p);
        // La visite sans horaires occupe 9 h–10 h, puis le musée ouvre à 10 h : aucune attente.
        assert_eq!(order(&plan), vec![1, 0]);
        assert_eq!(plan.stops[1].start_s, 10 * H);

        // Musée ouvert seulement 30 min : impossible.
        let mut short = visit(60, 5.0);
        short.windows = Some(vec![(10 * H, 10 * H + 1800)]);
        let visits = vec![short, visit(60, 3.0)];
        let p = DayProblem {
            visits: &visits,
            ..p
        };
        let plan = schedule_day(&p);
        assert_eq!(plan.unscheduled, vec![(0, Unscheduled::OpeningHours)]);
        assert_eq!(order(&plan), vec![1]);
    }

    #[test]
    fn overflow_drops_the_lowest_rated() {
        let visits = vec![visit(180, 2.0), visit(180, 5.0), visit(180, 4.0)];
        let m = line_matrix(&[0, 0, 0]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 17 * H,
            visits: &visits,
            start: None,
            end: None,
            travel: &m,
            fixed: &[],
        };
        let plan = schedule_day(&p);
        assert_eq!(plan.unscheduled, vec![(0, Unscheduled::NoTime)]);
        assert_eq!(plan.stops.len(), 2);
        assert!(plan.finish_s <= 17 * H);
    }

    #[test]
    fn return_to_lodging_counts_in_the_budget() {
        // Visite de 7 h à 2 h 30 de l'hébergement : 12 h au total pour 10 h disponibles.
        let visits = vec![visit(420, 5.0)];
        let m = line_matrix(&[15, 0]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 19 * H,
            visits: &visits,
            start: Some(1),
            end: Some(1),
            travel: &m,
            fixed: &[],
        };
        let plan = schedule_day(&p);
        assert!(plan.stops.is_empty());
        assert_eq!(plan.unscheduled, vec![(0, Unscheduled::NoTime)]);
        assert_eq!(plan.finish_s, 9 * H);
    }

    #[test]
    fn different_start_and_end_lodgings() {
        // Départ de l'hôtel A (x = 0), arrivée à l'hôtel B (x = 6) : visites dans l'ordre.
        let visits = vec![visit(30, 3.0), visit(30, 3.0), visit(30, 3.0)];
        let m = line_matrix(&[4, 2, 5, 0, 6]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 19 * H,
            visits: &visits,
            start: Some(3),
            end: Some(4),
            travel: &m,
            fixed: &[],
        };
        let plan = schedule_day(&p);
        assert_eq!(order(&plan), vec![1, 0, 2]);
        assert_eq!(plan.total_travel_s, 60 * 60);
    }

    #[test]
    fn locked_visits_keep_their_relative_order() {
        // Sans contrainte, l'ordre optimal serait 1, 2, 0, 3 (voir plus haut) ; 3 puis 1
        // sont verrouillés dans cet ordre : ils le restent, les autres s'insèrent autour.
        let visits = vec![visit(30, 3.0); 4];
        let m = line_matrix(&[3, 1, 2, 4, 0]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 19 * H,
            visits: &visits,
            start: Some(4),
            end: Some(4),
            travel: &m,
            fixed: &[3, 1],
        };
        let plan = schedule_day(&p);
        let o = order(&plan);
        assert_eq!(o.len(), 4);
        let pos = |v: usize| o.iter().position(|&x| x == v).unwrap();
        assert!(pos(3) < pos(1), "{o:?}");
        assert!(plan.stops.iter().all(|s| s.issue.is_none()));
    }

    #[test]
    fn impossible_locked_visits_are_kept_and_flagged() {
        let mut closed_morning = visit(60, 3.0);
        closed_morning.windows = Some(vec![(14 * H, 15 * H)]);
        let visits = vec![visit(480, 3.0), closed_morning, visit(60, 5.0)];
        let m = line_matrix(&[0, 0, 0]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 17 * H,
            visits: &visits,
            start: None,
            end: None,
            travel: &m,
            fixed: &[0, 1],
        };
        let plan = schedule_day(&p);
        // 8 h de visite puis un lieu ouvert de 14 h à 15 h : impossible, mais conservé.
        assert_eq!(order(&plan), vec![0, 1]);
        assert_eq!(plan.stops[1].issue, Some(StopIssue::OutsideOpeningHours));
        assert!(plan.overtime);
        assert_eq!(plan.unscheduled, vec![(2, Unscheduled::NoTime)]);
    }

    #[test]
    fn closed_locked_visit_leaves_room_for_the_others() {
        // Lieu verrouillé fermé ce jour-là : signalé, mais la journée reste libre pour le reste.
        let mut closed = visit(60, 3.0);
        closed.windows = Some(vec![]);
        let visits = vec![closed, visit(60, 3.0), visit(60, 4.0)];
        let m = line_matrix(&[0, 1, 2]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 17 * H,
            visits: &visits,
            start: None,
            end: None,
            travel: &m,
            fixed: &[0],
        };
        let plan = schedule_day(&p);
        assert_eq!(order(&plan).len(), 3, "{plan:?}");
        assert!(plan.unscheduled.is_empty());
        assert!(!plan.overtime);
        for s in &plan.stops {
            let expected = (s.visit == 0).then_some(StopIssue::OutsideOpeningHours);
            assert_eq!(s.issue, expected, "{plan:?}");
        }
    }

    #[test]
    fn manual_order_is_timed_with_flags() {
        let mut museum = visit(60, 3.0);
        museum.windows = Some(vec![(10 * H, 17 * H)]);
        let visits = vec![visit(300, 3.0), museum, visit(300, 3.0)];
        let m = line_matrix(&[0, 1, 2, 0]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 19 * H,
            visits: &visits,
            start: Some(3),
            end: Some(3),
            travel: &m,
            fixed: &[],
        };
        // Musée d'abord : attente de l'ouverture à 10 h.
        let plan = time_day(&p, &[1, 0]);
        assert_eq!(plan.stops[0].arrival_s, 9 * H + 600);
        assert_eq!(plan.stops[0].start_s, 10 * H);
        assert!(plan.stops.iter().all(|s| s.issue.is_none()) && !plan.overtime);
        // 9 h–14 h, puis 14 h 20–19 h 20 (après la fin), puis le musée déjà fermé.
        let plan = time_day(&p, &[0, 2, 1]);
        assert_eq!(order(&plan), vec![0, 2, 1]);
        assert_eq!(plan.stops[1].issue, Some(StopIssue::AfterDayEnd));
        assert_eq!(plan.stops[2].issue, Some(StopIssue::OutsideOpeningHours));
        assert!(plan.overtime);
        assert!(plan.finish_s > 19 * H);
    }

    #[test]
    fn empty_day() {
        let m = line_matrix(&[0]);
        let p = DayProblem {
            day_start_s: 9 * H,
            day_end_s: 19 * H,
            visits: &[],
            start: Some(0),
            end: Some(0),
            travel: &m,
            fixed: &[],
        };
        let plan = schedule_day(&p);
        assert!(plan.stops.is_empty() && plan.back.is_none());
        assert_eq!(plan.finish_s, 9 * H);

        // Changement d'hôtel sans visite : seul le trajet entre les deux.
        let m = line_matrix(&[0, 3]);
        let p = DayProblem {
            start: Some(0),
            end: Some(1),
            travel: &m,
            ..p
        };
        let plan = schedule_day(&p);
        assert_eq!(plan.back.map(|(_, l)| l.duration_s), Some(1800));
        assert_eq!(plan.finish_s, 9 * H + 1800);
    }
}
