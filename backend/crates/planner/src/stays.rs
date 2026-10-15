//! Proposition d'étapes de séjour : où dormir, et combien de nuits, d'après les lieux à visiter.
//!
//! 1. Les lieux sont groupés autour de `k` bases (k-médoïdes sur les temps de trajet estimés,
//!    lieux pondérés par la note du groupe).
//! 2. Pour chaque `k` possible (au moins `min_nights` nuits par étape), on évalue le coût :
//!    allers-retours quotidiens base ↔ lieux, trajets entre étapes (depuis l'arrivée et jusqu'au
//!    départ s'ils sont connus), un coût fixe par changement d'hébergement, et une forte
//!    pénalité quand une étape n'a pas assez de journées pour ses visites. Le moins cher gagne.
//! 3. Les nuits sont réparties selon le temps de visite de chaque étape (au moins
//!    `min_nights`), et les étapes ordonnées pour éviter les allers-retours.
//!
//! Tout est déterministe. Les temps de trajet sont fournis par l'appelant (matrice du moteur
//! de routage, ou estimations à vol d'oiseau : [`straight_line_times`]).

use crate::geo::LatLon;
use crate::travel::{self, Mode};

/// Coût d'un changement d'hébergement (bagages, check-in) : évite de bouger chaque soir.
pub const MOVE_COST_S: u32 = 45 * 60;
/// Pénalité par seconde de visite qui ne tient pas dans les journées d'une étape.
const OVERFLOW_FACTOR: u64 = 3;
/// Itérations maximales des k-médoïdes.
const MAX_ITERATIONS: usize = 30;
/// Au-delà, l'ordre des étapes est construit de proche en proche plutôt qu'exhaustivement.
const MAX_PERMUTED_STOPS: usize = 7;

#[derive(Debug, Clone)]
pub struct Visit {
    pub duration_s: u32,
    /// Importance (ex. note du groupe) : attire davantage la base.
    pub weight: f64,
}

/// Nœuds `0..visits.len()` : les visites ; puis l'arrivée et le départ éventuels.
#[derive(Debug, Clone)]
pub struct StayProblem<'a> {
    pub visits: &'a [Visit],
    pub nights: usize,
    pub min_nights: usize,
    /// Temps disponible par journée pour visites et trajets.
    pub day_budget_s: u32,
    /// Nœuds d'arrivée (ex. aéroport) et de départ, s'ils sont connus.
    pub start: Option<usize>,
    pub end: Option<usize>,
    /// `times[i][j]` : trajet du nœud `i` au nœud `j`, en secondes.
    pub times: &'a [Vec<u32>],
}

/// Matrice de temps estimés à vol d'oiseau entre des points.
pub fn straight_line_times(mode: Mode, points: &[LatLon]) -> Vec<Vec<u32>> {
    points
        .iter()
        .map(|&a| {
            points
                .iter()
                .map(|&b| travel::straight_line(mode, a, b).duration_s)
                .collect()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    /// Visite la plus centrale de l'étape (médoïde).
    pub base: usize,
    /// Indices des visites de l'étape.
    pub visits: Vec<usize>,
    pub nights: usize,
    /// Trajet depuis l'étape précédente (ou l'arrivée).
    pub transfer_s: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StayPlan {
    /// Étapes dans l'ordre du voyage.
    pub stops: Vec<Stop>,
    /// Temps de trajet estimé (allers-retours quotidiens et changements d'étape).
    pub travel_s: u64,
}

/// k-médoïdes pondérés ; renvoie les indices des médoïdes et l'étape de chaque visite.
fn cluster(p: &StayProblem<'_>, k: usize) -> (Vec<usize>, Vec<usize>) {
    let n = p.visits.len();
    let d = |a: usize, b: usize| p.times[a][b];
    // Initialisation déterministe : le lieu le plus proche de l'arrivée (sinon le plus
    // important), puis à chaque fois le lieu le plus éloigné des médoïdes déjà choisis.
    let first = match p.start {
        Some(s) => (0..n).min_by_key(|&i| (p.times[s][i], i)).unwrap_or(0),
        None => (0..n)
            .max_by(|&a, &b| {
                p.visits[a]
                    .weight
                    .total_cmp(&p.visits[b].weight)
                    .then(b.cmp(&a))
            })
            .unwrap_or(0),
    };
    let mut medoids = vec![first];
    while medoids.len() < k {
        let next = (0..n)
            .filter(|i| !medoids.contains(i))
            .max_by_key(|&i| {
                (
                    medoids.iter().map(|&m| d(i, m)).min().unwrap_or(0),
                    std::cmp::Reverse(i),
                )
            })
            .expect("k ≤ nombre de visites");
        medoids.push(next);
    }
    let assign = |medoids: &[usize]| -> Vec<usize> {
        (0..n)
            .map(|i| {
                (0..medoids.len())
                    .min_by_key(|&c| (d(i, medoids[c]), c))
                    .expect("au moins un médoïde")
            })
            .collect()
    };
    let mut groups = assign(&medoids);
    for _ in 0..MAX_ITERATIONS {
        let mut changed = false;
        for (c, medoid) in medoids.iter_mut().enumerate() {
            let members: Vec<usize> = (0..n).filter(|&i| groups[i] == c).collect();
            let cost = |m: usize| -> f64 {
                members
                    .iter()
                    .map(|&i| f64::from(d(m, i)) * p.visits[i].weight)
                    .sum()
            };
            let best = members
                .iter()
                .copied()
                .min_by(|&a, &b| cost(a).total_cmp(&cost(b)).then(a.cmp(&b)));
            if let Some(best) = best {
                if best != *medoid {
                    *medoid = best;
                    changed = true;
                }
            }
        }
        let next = assign(&medoids);
        if !changed && next == groups {
            break;
        }
        groups = next;
    }
    (medoids, groups)
}

/// Répartit `total` nuits au prorata des demandes, au moins `min` chacune (`total ≥ min × k`).
fn allocate(demands: &[f64], total: usize, min: usize) -> Vec<usize> {
    let sum: f64 = demands.iter().sum();
    let k = demands.len();
    let targets: Vec<f64> = demands
        .iter()
        .map(|d| {
            if sum > 0.0 {
                d / sum * total as f64
            } else {
                total as f64 / k as f64
            }
        })
        .collect();
    let mut nights: Vec<usize> = targets
        .iter()
        .map(|t| (t.floor() as usize).max(min))
        .collect();
    // Ajuste au total : une nuit de plus là où il en manque le plus, de moins là où il y en a
    // le plus en trop (sans descendre sous le minimum) ; à égalité, la première étape.
    loop {
        let assigned: usize = nights.iter().sum();
        let gap = |i: usize| targets[i] - nights[i] as f64;
        if assigned < total {
            let i = (0..k)
                .max_by(|&a, &b| gap(a).total_cmp(&gap(b)).then(b.cmp(&a)))
                .expect("au moins une étape");
            nights[i] += 1;
        } else if assigned > total {
            let i = (0..k)
                .filter(|&i| nights[i] > min)
                .min_by(|&a, &b| gap(a).total_cmp(&gap(b)).then(a.cmp(&b)))
                .expect("total ≥ min × k");
            nights[i] -= 1;
        } else {
            return nights;
        }
    }
}

/// Ordre des étapes minimisant les trajets entre elles (arrivée et départ compris).
fn order_stops(p: &StayProblem<'_>, bases: &[usize]) -> Vec<usize> {
    let k = bases.len();
    let path_cost = |order: &[usize]| -> u64 {
        let mut cost = 0u64;
        let mut prev = p.start;
        for &i in order {
            if let Some(a) = prev {
                cost += u64::from(p.times[a][bases[i]]);
            }
            prev = Some(bases[i]);
        }
        if let (Some(a), Some(e)) = (prev, p.end) {
            cost += u64::from(p.times[a][e]);
        }
        cost
    };
    if k <= MAX_PERMUTED_STOPS {
        let mut best: Option<(u64, Vec<usize>)> = None;
        let mut perm: Vec<usize> = (0..k).collect();
        permutations(&mut perm, 0, &mut |order| {
            let cost = path_cost(order);
            if best
                .as_ref()
                .is_none_or(|(c, o)| (cost, order) < (*c, o.as_slice()))
            {
                best = Some((cost, order.to_vec()));
            }
        });
        return best.map(|(_, o)| o).unwrap_or_default();
    }
    // De proche en proche depuis l'arrivée (sinon depuis la première étape).
    let mut left: Vec<usize> = (0..k).collect();
    let mut order = vec![];
    let mut at = p.start.unwrap_or(bases[0]);
    while !left.is_empty() {
        let (pos, _) = left
            .iter()
            .enumerate()
            .min_by_key(|&(_, &i)| (p.times[at][bases[i]], i))
            .expect("étapes restantes");
        let i = left.remove(pos);
        at = bases[i];
        order.push(i);
    }
    order
}

fn permutations(v: &mut Vec<usize>, i: usize, f: &mut impl FnMut(&[usize])) {
    if i == v.len() {
        f(v);
        return;
    }
    for j in i..v.len() {
        v.swap(i, j);
        permutations(v, i + 1, f);
        v.swap(i, j);
    }
}

/// Plan pour `k` étapes, et son coût.
fn plan_for(p: &StayProblem<'_>, k: usize) -> (u64, StayPlan) {
    let (medoids, groups) = cluster(p, k);
    let bases = medoids;
    let members: Vec<Vec<usize>> = (0..k)
        .map(|c| (0..p.visits.len()).filter(|&i| groups[i] == c).collect())
        .collect();
    // Temps nécessaire : visites, plus un aller-retour par journée et des trajets enchaînés.
    let round_trips: Vec<u64> = (0..k)
        .map(|c| {
            members[c]
                .iter()
                .map(|&i| 2 * u64::from(p.times[bases[c]][i]))
                .sum()
        })
        .collect();
    let demands: Vec<f64> = (0..k)
        .map(|c| {
            let visits: u64 = members[c]
                .iter()
                .map(|&i| u64::from(p.visits[i].duration_s))
                .sum();
            (visits + round_trips[c] / 2) as f64
        })
        .collect();
    let nights = allocate(&demands, p.nights, p.min_nights);
    let order = order_stops(p, &bases);

    let mut cost: u64 = round_trips.iter().sum();
    let mut travel: u64 = cost;
    let mut stops = Vec::with_capacity(k);
    let mut prev = p.start;
    for &c in &order {
        let transfer = prev.map_or(0, |a| p.times[a][bases[c]]);
        cost += u64::from(transfer);
        travel += u64::from(transfer);
        let capacity = nights[c] as u64 * u64::from(p.day_budget_s);
        cost += OVERFLOW_FACTOR * (demands[c] as u64).saturating_sub(capacity);
        stops.push(Stop {
            base: bases[c],
            visits: members[c].clone(),
            nights: nights[c],
            transfer_s: transfer,
        });
        prev = Some(bases[c]);
    }
    if let (Some(a), Some(e)) = (prev, p.end) {
        let back = u64::from(p.times[a][e]);
        cost += back;
        travel += back;
    }
    cost += u64::from(MOVE_COST_S) * (k as u64 - 1);
    (
        cost,
        StayPlan {
            stops,
            travel_s: travel,
        },
    )
}

/// Meilleur plan d'étapes, ou `None` sans visite ni nuit.
pub fn propose(p: &StayProblem<'_>) -> Option<StayPlan> {
    if p.visits.is_empty() || p.nights == 0 {
        return None;
    }
    let min = p.min_nights.clamp(1, p.nights);
    let p = StayProblem {
        min_nights: min,
        ..p.clone()
    };
    let max_k = (p.nights / min).min(p.visits.len()).max(1);
    (1..=max_k)
        .map(|k| plan_for(&p, k))
        .min_by_key(|(cost, plan)| (*cost, plan.stops.len()))
        .map(|(_, plan)| plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u32 = 3600;
    const SAN_JOSE: LatLon = LatLon::new(9.9327, -84.0716);
    const FORTUNA: LatLon = LatLon::new(10.4706, -84.6453);
    const MONTEVERDE: LatLon = LatLon::new(10.3009, -84.8257);
    const MANUEL_ANTONIO: LatLon = LatLon::new(9.3923, -84.1370);

    /// Lieux à visiter (positions et durées), arrivée et départ éventuels.
    #[derive(Default)]
    struct Trip {
        points: Vec<LatLon>,
        visits: Vec<Visit>,
        start: Option<LatLon>,
        end: Option<LatLon>,
    }

    impl Trip {
        fn around(mut self, center: LatLon, n: usize, duration_h: u32) -> Self {
            for i in 0..n {
                let d = 0.01 * i as f64;
                self.points
                    .push(LatLon::new(center.lat + d, center.lon + d));
                self.visits.push(Visit {
                    duration_s: duration_h * H,
                    weight: 1.0,
                });
            }
            self
        }

        /// Plan pour `nights` nuits (au moins `min_nights` par étape).
        fn propose(&self, nights: usize, min_nights: usize) -> Option<(StayPlan, Vec<LatLon>)> {
            let mut nodes = self.points.clone();
            let start = self.start.map(|s| {
                nodes.push(s);
                nodes.len() - 1
            });
            let end = self.end.map(|e| {
                nodes.push(e);
                nodes.len() - 1
            });
            let times = straight_line_times(Mode::Car, &nodes);
            let p = StayProblem {
                visits: &self.visits,
                nights,
                min_nights,
                day_budget_s: 10 * H,
                start,
                end,
                times: &times,
            };
            propose(&p).map(|plan| (plan, nodes))
        }
    }

    fn near(a: LatLon, b: LatLon) -> bool {
        crate::geo::haversine_m(a, b) < 10_000.0
    }

    #[test]
    fn distant_regions_become_separate_stops_with_nights_by_visit_time() {
        // San José (1 journée de visites) puis La Fortuna (3 journées) : 2 étapes.
        let trip = Trip {
            start: Some(SAN_JOSE),
            ..Default::default()
        }
        .around(SAN_JOSE, 2, 4)
        .around(FORTUNA, 6, 5);
        let (plan, nodes) = trip.propose(6, 2).unwrap();
        assert_eq!(plan.stops.len(), 2, "{plan:?}");
        assert!(near(nodes[plan.stops[0].base], SAN_JOSE));
        assert!(near(nodes[plan.stops[1].base], FORTUNA));
        assert_eq!(plan.stops.iter().map(|s| s.nights).sum::<usize>(), 6);
        assert!(plan.stops[1].nights > plan.stops[0].nights);
        assert_eq!(plan.stops[0].visits, vec![0, 1]);
        assert!(plan.stops[1].transfer_s > H);
    }

    #[test]
    fn a_compact_trip_keeps_a_single_stop() {
        let (plan, _) = Trip::default().around(FORTUNA, 5, 2).propose(4, 2).unwrap();
        assert_eq!(plan.stops.len(), 1);
        assert_eq!(plan.stops[0].nights, 4);
        assert_eq!(plan.stops[0].transfer_s, 0);
    }

    #[test]
    fn minimum_nights_limit_the_number_of_stops() {
        let trip = Trip::default()
            .around(SAN_JOSE, 2, 3)
            .around(FORTUNA, 2, 3)
            .around(MONTEVERDE, 2, 3)
            .around(MANUEL_ANTONIO, 2, 3);
        // 5 nuits, au moins 2 par étape : au plus 2 étapes.
        let (plan, _) = trip.propose(5, 2).unwrap();
        assert!(plan.stops.len() <= 2, "{plan:?}");
        assert!(plan.stops.iter().all(|s| s.nights >= 2));
        assert!(trip.propose(8, 1).unwrap().0.stops.len() >= 3);
    }

    #[test]
    fn stops_are_ordered_from_arrival_to_departure() {
        let trip = Trip {
            start: Some(FORTUNA),
            end: Some(SAN_JOSE),
            ..Default::default()
        }
        .around(MANUEL_ANTONIO, 4, 4)
        .around(FORTUNA, 4, 4);
        let (plan, nodes) = trip.propose(6, 2).unwrap();
        assert!(near(nodes[plan.stops[0].base], FORTUNA), "{plan:?}");
        let other = Trip {
            start: Some(MANUEL_ANTONIO),
            ..Default::default()
        }
        .around(MANUEL_ANTONIO, 4, 4)
        .around(FORTUNA, 4, 4);
        let (plan, nodes) = other.propose(6, 2).unwrap();
        assert!(near(nodes[plan.stops[0].base], MANUEL_ANTONIO));
        // Déterministe.
        assert_eq!(trip.propose(6, 2), trip.propose(6, 2));
    }

    #[test]
    fn real_driving_times_change_the_plan() {
        // Deux groupes proches à vol d'oiseau mais séparés par 4 h de route : deux étapes.
        let trip = Trip::default()
            .around(FORTUNA, 3, 3)
            .around(MONTEVERDE, 3, 3);
        let mut times = straight_line_times(Mode::Car, &trip.points);
        for (i, row) in times.iter_mut().enumerate() {
            for (j, t) in row.iter_mut().enumerate() {
                if (i < 3) != (j < 3) {
                    *t = 4 * H;
                }
            }
        }
        let p = StayProblem {
            visits: &trip.visits,
            nights: 4,
            min_nights: 2,
            day_budget_s: 10 * H,
            start: None,
            end: None,
            times: &times,
        };
        let plan = propose(&p).unwrap();
        assert_eq!(plan.stops.len(), 2, "{plan:?}");
        assert_eq!(plan.stops[1].transfer_s, 4 * H);
    }

    #[test]
    fn nights_allocation() {
        assert_eq!(allocate(&[1.0, 3.0], 6, 2), vec![2, 4]);
        assert_eq!(allocate(&[1.0, 1.0, 1.0], 7, 2), vec![3, 2, 2]);
        assert_eq!(allocate(&[0.0], 3, 2), vec![3]);
        assert!(Trip::default().propose(3, 2).is_none());
    }
}
