//! Circuits types : quelles régions enchaîner, dans quel ordre et combien de nuits chacune,
//! pour la durée du voyage (et depuis l'aéroport d'arrivée jusqu'à celui de départ).
//!
//! Insertion gloutonne : on ajoute la région qui rapporte le plus (sa notoriété, moins le
//! détour de route qu'elle impose) à la meilleure place du circuit, tant que ses nuits
//! conseillées tiennent dans le voyage — et même à perte tant que les étapes retenues ne
//! peuvent pas occuper toutes les nuits ; puis 2-opt sur l'ordre, et les nuits restantes vont
//! aux régions les plus riches. Les variantes rejouent l'insertion en dévaluant les régions
//! déjà proposées, pour montrer d'autres façons de voir le pays. Déterministe.

/// Région candidate.
#[derive(Debug, Clone)]
pub struct Stop {
    /// Intérêt de la région (notoriété, envies du groupe).
    pub weight: f64,
    /// Nuits conseillées (au moins une).
    pub nights: usize,
}

/// Nœuds `0..stops.len()` : les régions ; puis l'arrivée et le départ éventuels.
#[derive(Debug, Clone)]
pub struct CircuitProblem<'a> {
    pub stops: &'a [Stop],
    /// Nuits du voyage.
    pub nights: usize,
    pub start: Option<usize>,
    pub end: Option<usize>,
    /// `times[i][j]` : trajet du nœud `i` au nœud `j`, en secondes.
    pub times: &'a [Vec<u32>],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Leg {
    pub stop: usize,
    pub nights: usize,
    /// Trajet depuis l'étape précédente (ou l'arrivée ; 0 sans arrivée pour la première).
    pub transfer_s: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Circuit {
    pub legs: Vec<Leg>,
    /// Trajet de la dernière étape au départ (0 sans départ).
    pub return_s: u32,
    /// Route totale entre étapes (arrivée et départ compris).
    pub drive_s: u64,
}

/// Coût d'une heure de route, en points d'intérêt (la région la plus intéressante vaut 100).
const DRIVE_COST_PER_H: f64 = 10.0;
/// Trajet au-delà duquel deux étapes ne s'enchaînent pas (journée entière de route).
pub const MAX_LEG_S: u32 = 7 * 3600;
/// Deux étapes plus proches que cela feraient changer d'hôtel pour rien : l'une se visite
/// depuis l'autre.
pub const MIN_GAP_S: u32 = 45 * 60;
/// Une région déjà proposée dans une variante précédente ne vaut plus que cette part.
const REUSE_FACTOR: f64 = 0.35;
/// Nuits ajoutées au plus aux nuits conseillées d'une région : au-delà, mieux vaut une étape
/// de plus (même un peu moins intéressante) que de rester sur place.
const EXTRA_NIGHTS: usize = 3;

fn leg_time(p: &CircuitProblem<'_>, a: Option<usize>, b: Option<usize>) -> u32 {
    match (a, b) {
        (Some(a), Some(b)) => p.times[a][b],
        _ => 0,
    }
}

/// Route totale d'un ordre d'étapes, arrivée et départ compris.
fn route_time(p: &CircuitProblem<'_>, order: &[usize]) -> u64 {
    let mut nodes: Vec<Option<usize>> = vec![p.start];
    nodes.extend(order.iter().map(|&s| Some(s)));
    nodes.push(p.end);
    nodes
        .windows(2)
        .map(|w| u64::from(leg_time(p, w[0], w[1])))
        .sum()
}

fn too_long(p: &CircuitProblem<'_>, order: &[usize]) -> bool {
    let mut nodes: Vec<Option<usize>> = vec![p.start];
    nodes.extend(order.iter().map(|&s| Some(s)));
    nodes.push(p.end);
    nodes
        .windows(2)
        .any(|w| leg_time(p, w[0], w[1]) > MAX_LEG_S)
}

/// Un circuit, avec des poids déjà ajustés (variantes).
fn build(p: &CircuitProblem<'_>, weights: &[f64]) -> Option<Circuit> {
    let n = p.stops.len();
    let mut order: Vec<usize> = vec![];
    let mut used_nights = 0;
    // Nuits que les étapes retenues peuvent occuper (conseillées + quelques-unes de plus).
    let capacity = |order: &[usize]| -> usize {
        order
            .iter()
            .map(|&s| p.stops[s].nights.max(1) + EXTRA_NIGHTS)
            .sum()
    };
    loop {
        let base = route_time(p, &order);
        let mut best: Option<(f64, usize, usize)> = None;
        for c in (0..n).filter(|c| !order.contains(c)) {
            let nights = p.stops[c].nights.max(1);
            if used_nights + nights > p.nights
                || order
                    .iter()
                    .any(|&o| p.times[o][c].min(p.times[c][o]) < MIN_GAP_S)
            {
                continue;
            }
            for pos in 0..=order.len() {
                let mut tried = order.clone();
                tried.insert(pos, c);
                if too_long(p, &tried) {
                    continue;
                }
                let detour_h = (route_time(p, &tried) as f64 - base as f64) / 3600.0;
                let gain = weights[c] - DRIVE_COST_PER_H * detour_h;
                let needed = capacity(&order) < p.nights;
                // À gain égal : la région de plus petit indice, puis la première place.
                if (gain > 0.0 || needed) && best.is_none_or(|(g, _, _)| gain > g) {
                    best = Some((gain, c, pos));
                }
            }
        }
        let Some((_, c, pos)) = best else { break };
        order.insert(pos, c);
        used_nights += p.stops[c].nights.max(1);
    }
    if order.is_empty() {
        // Voyage plus court que les nuits conseillées : la meilleure région, en raccourci.
        let best = (0..n)
            .filter(|&c| weights[c] > 0.0 && !too_long(p, &[c]))
            .max_by(|&a, &b| weights[a].total_cmp(&weights[b]).then(b.cmp(&a)))?;
        if p.nights == 0 {
            return None;
        }
        order.push(best);
    }
    two_opt(p, &mut order);

    // Nuits : celles conseillées, puis le reste à la région qui en profite le plus.
    let mut nights: Vec<usize> = order
        .iter()
        .map(|&s| p.stops[s].nights.clamp(1, p.nights))
        .collect();
    let mut left = p.nights.saturating_sub(nights.iter().sum());
    while left > 0 {
        let pick = (0..order.len())
            .filter(|&i| nights[i] < p.stops[order[i]].nights.max(1) + EXTRA_NIGHTS)
            .max_by(|&a, &b| {
                let value = |i: usize| weights[order[i]] / (nights[i] + 1) as f64;
                value(a).total_cmp(&value(b)).then(b.cmp(&a))
            })
            // Toutes au plafond : la plus intéressante prend le reste.
            .unwrap_or(0);
        nights[pick] += 1;
        left -= 1;
    }

    let mut prev = p.start;
    let legs = order
        .iter()
        .zip(nights)
        .map(|(&stop, nights)| {
            let transfer_s = leg_time(p, prev, Some(stop));
            prev = Some(stop);
            Leg {
                stop,
                nights,
                transfer_s,
            }
        })
        .collect();
    Some(Circuit {
        legs,
        return_s: leg_time(p, prev, p.end),
        drive_s: route_time(p, &order),
    })
}

/// Inverse des portions de l'ordre tant que la route raccourcit.
fn two_opt(p: &CircuitProblem<'_>, order: &mut [usize]) {
    let mut improved = true;
    while improved {
        improved = false;
        for i in 0..order.len() {
            for j in i + 1..order.len() {
                let before = route_time(p, order);
                order[i..=j].reverse();
                if route_time(p, order) < before && !too_long(p, order) {
                    improved = true;
                } else {
                    order[i..=j].reverse();
                }
            }
        }
    }
}

/// Jusqu'à `count` circuits différents, le premier étant le plus riche.
pub fn circuits(p: &CircuitProblem<'_>, count: usize) -> Vec<Circuit> {
    let max = p.stops.iter().map(|s| s.weight).fold(0.0, f64::max);
    if max <= 0.0 {
        return vec![];
    }
    let mut weights: Vec<f64> = p.stops.iter().map(|s| s.weight / max * 100.0).collect();
    let mut out: Vec<Circuit> = vec![];
    // Quelques essais de plus que demandé : une variante peut retomber sur un circuit déjà vu.
    for _ in 0..count * 2 {
        if out.len() == count {
            break;
        }
        let Some(c) = build(p, &weights) else { break };
        for leg in &c.legs {
            weights[leg.stop] *= REUSE_FACTOR;
        }
        let mut set: Vec<usize> = c.legs.iter().map(|l| l.stop).collect();
        set.sort();
        let seen = out.iter().any(|o| {
            let mut s: Vec<usize> = o.legs.iter().map(|l| l.stop).collect();
            s.sort();
            s == set
        });
        if !seen {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::LatLon;
    use crate::stays::straight_line_times;
    use crate::travel::Mode;

    const SAN_JOSE: LatLon = LatLon::new(9.93, -84.08);
    const FORTUNA: LatLon = LatLon::new(10.47, -84.65);
    const QUEPOS: LatLon = LatLon::new(9.43, -84.16);
    const LIBERIA: LatLon = LatLon::new(10.63, -85.44);
    const LIMON: LatLon = LatLon::new(9.99, -83.03);

    fn stop(weight: f64, nights: usize) -> Stop {
        Stop { weight, nights }
    }

    /// Temps entre les régions, puis San José (arrivée et départ) ; renvoie aussi son nœud.
    fn times_with_san_jose(points: &[LatLon]) -> (Vec<Vec<u32>>, usize) {
        let mut all = points.to_vec();
        all.push(SAN_JOSE);
        (straight_line_times(Mode::Car, &all), points.len())
    }

    #[test]
    fn a_short_trip_keeps_the_best_regions_that_fit() {
        let stops = [stop(100.0, 2), stop(80.0, 2), stop(30.0, 2), stop(20.0, 2)];
        let points = [FORTUNA, QUEPOS, LIBERIA, LIMON];
        let (times, sj) = times_with_san_jose(&points);
        let p = CircuitProblem {
            stops: &stops,
            nights: 5,
            start: Some(sj),
            end: Some(sj),
            times: &times,
        };
        let out = circuits(&p, 3);
        let first = &out[0];
        let picked: Vec<usize> = first.legs.iter().map(|l| l.stop).collect();
        assert_eq!(picked.len(), 2, "{first:?}");
        assert!(picked.contains(&0) && picked.contains(&1));
        // Toutes les nuits du voyage sont attribuées.
        assert_eq!(first.legs.iter().map(|l| l.nights).sum::<usize>(), 5);
        assert!(first.legs[0].transfer_s > 0 && first.return_s > 0);
        // Les variantes proposent autre chose.
        assert!(out.len() >= 2);
        assert_ne!(out[1].legs, first.legs);
    }

    #[test]
    fn a_long_trip_visits_more_regions_in_a_sensible_order() {
        let stops = [stop(100.0, 2), stop(80.0, 2), stop(60.0, 2), stop(50.0, 2)];
        let points = [FORTUNA, QUEPOS, LIBERIA, LIMON];
        let (times, sj) = times_with_san_jose(&points);
        let p = CircuitProblem {
            stops: &stops,
            nights: 12,
            start: Some(sj),
            end: Some(sj),
            times: &times,
        };
        let first = &circuits(&p, 1)[0];
        // Limón, peu intéressante et à l'écart, peut être laissée de côté.
        assert!(first.legs.len() >= 3, "{first:?}");
        // La Fortuna et Liberia (au nord) se suivent : pas d'aller-retour par le sud.
        let pos = |s: usize| first.legs.iter().position(|l| l.stop == s).unwrap();
        assert_eq!(pos(0).abs_diff(pos(2)), 1, "{first:?}");
        assert_eq!(first.legs.iter().map(|l| l.nights).sum::<usize>(), 12);
    }

    #[test]
    fn a_dominant_region_does_not_absorb_the_whole_trip() {
        // Une région très riche et deux modestes : 10 nuits ne se passent pas toutes au même
        // endroit (au plus nuits conseillées + 3).
        let stops = [stop(100.0, 2), stop(5.0, 2), stop(5.0, 2)];
        let points = [FORTUNA, QUEPOS, LIBERIA];
        let (times, sj) = times_with_san_jose(&points);
        let p = CircuitProblem {
            stops: &stops,
            nights: 10,
            start: Some(sj),
            end: Some(sj),
            times: &times,
        };
        let first = &circuits(&p, 1)[0];
        assert!(first.legs.len() >= 2, "{first:?}");
        assert!(first.legs.iter().all(|l| l.nights <= 5), "{first:?}");
        assert_eq!(first.legs.iter().map(|l| l.nights).sum::<usize>(), 10);
    }

    #[test]
    fn neighbouring_bases_are_not_both_stops() {
        // Alajuela à 20 km de San José : visitée depuis San José, pas une étape de plus.
        let alajuela = LatLon::new(10.02, -84.21);
        let stops = [stop(100.0, 2), stop(90.0, 2), stop(60.0, 2)];
        let points = [SAN_JOSE, alajuela, FORTUNA];
        let times = straight_line_times(Mode::Car, &points);
        let p = CircuitProblem {
            stops: &stops,
            nights: 6,
            start: None,
            end: None,
            times: &times,
        };
        let first = &circuits(&p, 1)[0];
        let picked: Vec<usize> = first.legs.iter().map(|l| l.stop).collect();
        assert!(picked.contains(&0) && picked.contains(&2) && !picked.contains(&1));
    }

    #[test]
    fn without_endpoints_or_enough_nights() {
        let stops = [stop(100.0, 3)];
        let times = vec![vec![0]];
        let p = CircuitProblem {
            stops: &stops,
            nights: 2,
            start: None,
            end: None,
            times: &times,
        };
        // Les nuits conseillées ne tiennent pas : la région quand même, en raccourci.
        assert_eq!(circuits(&p, 2)[0].legs[0].nights, 2);
        assert!(circuits(
            &CircuitProblem {
                nights: 0,
                ..p.clone()
            },
            2
        )
        .is_empty());
        let p = CircuitProblem { nights: 4, ..p };
        let out = circuits(&p, 2);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].legs[0].nights, 4);
        assert_eq!((out[0].legs[0].transfer_s, out[0].return_s), (0, 0));
    }
}
