//! Régions à découvrir : les lieux notables du pays regroupés autour de bases d'une journée.
//!
//! Chaque lieu pèse selon sa notoriété (langues de son article Wikipédia, puissance 1,5 : un
//! lieu célèbre l'emporte sur plusieurs lieux obscurs). On retient d'abord le lieu dont le
//! voisinage (rayon [`RADIUS_M`]) pèse le plus : ce voisinage forme une région. On recommence
//! avec les lieux restants, jusqu'à [`MAX_REGIONS`] régions ou une région trop légère. Simple,
//! déterministe, et le nombre de régions n'a pas à être fixé à l'avance (contrairement aux
//! k-médoïdes).
//!
//! La base d'une région ([`best_base`]) est la localité la plus proche de ses lieux (pondérés),
//! favorisée par le nombre d'hébergements autour et par sa taille.

use crate::geo::{haversine_m, LatLon};

/// Rayon d'une région : excursions d'une journée depuis une même base.
pub const RADIUS_M: f64 = 25_000.0;
pub const MAX_REGIONS: usize = 12;
/// Poids minimal d'une région : un lieu connu dans 8 langues environ (8^1,5).
pub const MIN_WEIGHT: f64 = 22.0;
/// Points forts retenus par région pour estimer les nuits conseillées.
const HIGHLIGHTS: usize = 6;
pub const MAX_NIGHTS: u32 = 4;

/// Lieu notable. `category` est un identifiant opaque (les catégories de l'appelant).
#[derive(Debug, Clone)]
pub struct Spot {
    pub location: LatLon,
    /// Langues de l'article Wikipédia.
    pub languages: u32,
    pub category: usize,
    pub duration_s: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// Barycentre des lieux, pondéré par leur notoriété.
    pub center: LatLon,
    /// Indices des lieux, du plus au moins notable.
    pub members: Vec<usize>,
    pub weight: f64,
    /// Catégories par poids décroissant.
    pub themes: Vec<usize>,
    /// Nuits conseillées pour en voir les points forts.
    pub nights: u32,
}

impl Spot {
    pub fn weight(&self) -> f64 {
        f64::from(self.languages).powf(1.5)
    }
}

/// Localité où loger (candidate pour la base d'une région).
#[derive(Debug, Clone)]
pub struct Settlement {
    pub location: LatLon,
    /// Hébergements connus autour.
    pub lodging: u32,
    /// Facteur de taille : plus petit pour une ville (ses quartiers sont des « villages »).
    pub size_factor: f64,
}

/// Meilleure base pour une région : distance moyenne (pondérée) à ses lieux, réduite selon le
/// nombre d'hébergements (logarithme : l'offre compte, sans écraser la proximité) et la taille.
pub fn best_base(spots: &[Spot], region: &Region, candidates: &[Settlement]) -> Option<usize> {
    let cost = |s: &Settlement| {
        let distance: f64 = region
            .members
            .iter()
            .map(|&j| spots[j].weight() * (haversine_m(s.location, spots[j].location) + 500.0))
            .sum::<f64>()
            / region.weight;
        distance * s.size_factor / (1.0 + f64::from(s.lodging).ln_1p())
    };
    candidates
        .iter()
        .enumerate()
        .min_by(|(i, a), (j, b)| cost(a).total_cmp(&cost(b)).then(i.cmp(j)))
        .map(|(i, _)| i)
}

/// Groupes de bases reliées par la route (`links[i][j]`, dans un sens ou l'autre) : `true`
/// pour les bases du plus grand groupe (à égalité, celui de la première base).
pub fn road_groups(links: &[Vec<bool>]) -> Vec<bool> {
    let n = links.len();
    let linked = |i: usize, j: usize| {
        links[i].get(j).copied().unwrap_or(false) || links[j].get(i).copied().unwrap_or(false)
    };
    let mut group = vec![usize::MAX; n];
    let mut sizes = vec![];
    for start in 0..n {
        if group[start] != usize::MAX {
            continue;
        }
        let id = sizes.len();
        let (mut stack, mut size) = (vec![start], 0);
        group[start] = id;
        while let Some(i) = stack.pop() {
            size += 1;
            let next: Vec<usize> = (0..n)
                .filter(|&j| group[j] == usize::MAX && linked(i, j))
                .collect();
            for j in next {
                group[j] = id;
                stack.push(j);
            }
        }
        sizes.push(size);
    }
    let main = (0..sizes.len()).max_by(|&a, &b| sizes[a].cmp(&sizes[b]).then(b.cmp(&a)));
    group.iter().map(|&g| Some(g) == main).collect()
}

/// Nuits pour visiter les points forts, `day_budget_s` par journée (au moins une).
fn nights(spots: &[Spot], members: &[usize], day_budget_s: u32) -> u32 {
    let visits: u32 = members
        .iter()
        .take(HIGHLIGHTS)
        .map(|&i| spots[i].duration_s)
        .sum();
    // Trajets sur place : un quart du temps de visite environ.
    let needed = visits + visits / 4;
    needed.div_ceil(day_budget_s.max(3600)).clamp(1, MAX_NIGHTS)
}

/// Régions par poids décroissant.
pub fn regions(spots: &[Spot], day_budget_s: u32) -> Vec<Region> {
    let near: Vec<Vec<usize>> = spots
        .iter()
        .map(|a| {
            spots
                .iter()
                .enumerate()
                .filter(|(_, b)| haversine_m(a.location, b.location) <= RADIUS_M)
                .map(|(j, _)| j)
                .collect()
        })
        .collect();
    let mut taken = vec![false; spots.len()];
    let mut out = vec![];
    while out.len() < MAX_REGIONS {
        let mass = |i: usize| -> f64 {
            near[i]
                .iter()
                .filter(|&&j| !taken[j])
                .map(|&j| spots[j].weight())
                .sum()
        };
        // Plus lourd voisinage ; à égalité, le premier lieu.
        let Some((seed, weight)) = (0..spots.len())
            .filter(|&i| !taken[i])
            .map(|i| (i, mass(i)))
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        else {
            break;
        };
        if weight < MIN_WEIGHT {
            break;
        }
        let mut members: Vec<usize> = near[seed].iter().copied().filter(|&j| !taken[j]).collect();
        for &j in &members {
            taken[j] = true;
        }
        members.sort_by(|&a, &b| {
            spots[b]
                .weight()
                .total_cmp(&spots[a].weight())
                .then(a.cmp(&b))
        });
        let (lat, lon) = members.iter().fold((0.0, 0.0), |(la, lo), &j| {
            let s = &spots[j];
            (
                la + s.location.lat * s.weight(),
                lo + s.location.lon * s.weight(),
            )
        });
        let mut by_theme: Vec<(usize, f64)> = vec![];
        for &j in &members {
            let s = &spots[j];
            match by_theme.iter_mut().find(|(c, _)| *c == s.category) {
                Some((_, w)) => *w += s.weight(),
                None => by_theme.push((s.category, s.weight())),
            }
        }
        by_theme.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out.push(Region {
            center: LatLon::new(lat / weight, lon / weight),
            nights: nights(spots, &members, day_budget_s),
            members,
            weight,
            themes: by_theme.into_iter().map(|(c, _)| c).collect(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORTUNA: LatLon = LatLon::new(10.4706, -84.6453);
    const MONTEVERDE: LatLon = LatLon::new(10.3010, -84.8250);
    const CORCOVADO: LatLon = LatLon::new(8.54, -83.57);
    const NATURE: usize = 1;
    const MUSEUM: usize = 2;

    fn spot(at: LatLon, dlat: f64, languages: u32, category: usize) -> Spot {
        Spot {
            location: LatLon::new(at.lat + dlat, at.lon),
            languages,
            category,
            duration_s: 2 * 3600,
        }
    }

    #[test]
    fn dense_notable_areas_become_regions() {
        let spots = vec![
            spot(FORTUNA, 0.0, 9, NATURE),
            spot(FORTUNA, 0.02, 4, NATURE),
            spot(FORTUNA, -0.03, 1, MUSEUM),
            spot(CORCOVADO, 0.0, 9, NATURE),
            spot(CORCOVADO, 0.05, 1, NATURE),
            // Isolé et peu connu : pas de région.
            spot(LatLon::new(9.6, -82.7), 0.0, 4, MUSEUM),
        ];
        let out = regions(&spots, 10 * 3600);
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0].members, vec![0, 1, 2]);
        assert!((out[0].weight - 36.0).abs() < 1e-9);
        assert_eq!(out[0].themes, vec![NATURE, MUSEUM]);
        assert!(haversine_m(out[0].center, FORTUNA) < 3_000.0);
        assert_eq!(out[1].members, vec![3, 4]);
    }

    #[test]
    fn one_famous_place_outweighs_several_obscure_ones() {
        let mut spots: Vec<Spot> = (0..5)
            .map(|i| spot(FORTUNA, f64::from(i) * 0.01, 4, NATURE))
            .collect();
        spots.push(spot(CORCOVADO, 0.0, 25, NATURE));
        let out = regions(&spots, 10 * 3600);
        assert_eq!(out[0].members, vec![5]);
    }

    #[test]
    fn neighbouring_regions_do_not_share_places() {
        // La Fortuna et Monteverde sont à ~28 km : deux régions distinctes.
        let spots = vec![
            spot(FORTUNA, 0.0, 16, NATURE),
            spot(FORTUNA, 0.01, 9, NATURE),
            spot(MONTEVERDE, 0.0, 9, NATURE),
            spot(MONTEVERDE, 0.01, 4, NATURE),
        ];
        let out = regions(&spots, 10 * 3600);
        assert_eq!(out.len(), 2, "{out:?}");
        let mut all: Vec<usize> = out.iter().flat_map(|r| r.members.clone()).collect();
        all.sort();
        assert_eq!(all, vec![0, 1, 2, 3]);
    }

    #[test]
    fn nights_follow_the_time_needed_for_highlights() {
        let few = vec![spot(FORTUNA, 0.0, 9, NATURE)];
        assert_eq!(regions(&few, 10 * 3600)[0].nights, 1);
        let many: Vec<Spot> = (0..10)
            .map(|i| spot(FORTUNA, f64::from(i) * 0.001, 4, NATURE))
            .collect();
        // Six points forts de 2 h, plus les trajets : 15 h, soit 2 journées de 10 h.
        assert_eq!(regions(&many, 10 * 3600)[0].nights, 2);
        // Plafonné.
        assert_eq!(regions(&many, 3600)[0].nights, MAX_NIGHTS);
    }

    #[test]
    fn the_base_is_near_the_places_and_has_lodging() {
        let volcano = LatLon::new(10.4621, -84.7034);
        let spots = vec![
            spot(volcano, 0.0, 41, NATURE),
            spot(volcano, 0.01, 15, NATURE),
        ];
        let region = &regions(&spots, 10 * 3600)[0];
        let town = |at: LatLon, lodging| Settlement {
            location: at,
            lodging,
            size_factor: 0.8,
        };
        // Monteverde a trois fois plus d'hébergements, mais La Fortuna est bien plus près.
        let candidates = [town(MONTEVERDE, 150), town(FORTUNA, 50)];
        assert_eq!(best_base(&spots, region, &candidates), Some(1));
        // Un hameau sans hébergement, même tout près, passe après.
        let hamlet = Settlement {
            location: LatLon::new(volcano.lat + 0.02, volcano.lon),
            lodging: 0,
            size_factor: 1.0,
        };
        assert_eq!(
            best_base(&spots, region, &[hamlet, town(FORTUNA, 50)]),
            Some(1)
        );
        assert_eq!(best_base(&spots, region, &[]), None);
    }

    #[test]
    fn the_largest_road_network_is_the_mainland() {
        // 0, 1, 2 reliés (dans un sens suffit) ; 3 isolé (île) ; 4 et 5 reliés entre eux.
        let mut links = vec![vec![false; 6]; 6];
        for (i, j) in [(0, 1), (2, 1), (4, 5)] {
            links[i][j] = true;
        }
        assert_eq!(
            road_groups(&links),
            vec![true, true, true, false, false, false]
        );
        assert!(road_groups(&[]).is_empty());
    }

    #[test]
    fn no_spots_no_regions() {
        assert!(regions(&[], 36_000).is_empty());
    }
}
