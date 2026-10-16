//! Suggestions de lieux à ajouter au voyage : score déterministe et explicable.
//!
//! Chaque candidat reçoit une somme pondérée de critères, chacun accompagné d'une raison
//! lisible quand il compte :
//!
//! - **proximité** : trajet estimé depuis l'hébergement (ou les lieux) du jour le plus proche ;
//!   au-delà de [`MAX_TRAVEL_S`], le lieu est écarté ;
//! - **notoriété** : nombre de langues de son article Wikipédia (échelle logarithmique) ;
//! - **intérêt pour le voyageur** : guide Wikivoyage, patrimoine mondial de l'UNESCO,
//!   superficie d'un espace protégé, consultations de ses articles Wikipédia (popularité
//!   actuelle, échelle logarithmique) ;
//! - **goûts du groupe** : moyenne des notes du groupe pour sa catégorie ;
//! - **envies** : le lieu répond aux envies déclarées (ou au budget, pour un hébergement),
//!   d'autant plus que le groupe n'a pas encore d'avis sur ce genre de lieux ;
//! - **informations** : horaires, site web, référence Wikipédia connus ;
//! - **temps libre** : le jour le plus proche a encore de la place dans l'itinéraire ;
//! - **saturation** : déjà beaucoup de lieux de cette catégorie, ou des lieux semblables écartés.
//!
//! La liste est ensuite diversifiée : un lieu proche d'un lieu de même catégorie déjà retenu
//! est pénalisé, et un homonyme tout proche (doublon OSM) est ignoré.

use std::collections::HashMap;

use serde::Serialize;

use crate::geo::{haversine_m, LatLon};
use crate::travel::{self, Mode};

/// Trajet (aller) au-delà duquel un lieu n'est pas proposé.
pub const MAX_TRAVEL_S: u32 = 150 * 60;
/// Trajet à partir duquel la proximité ne rapporte plus rien.
const PROXIMITY_HORIZON_S: f64 = 90.0 * 60.0;
/// Nombre de langues Wikipédia donnant la notoriété maximale.
const NOTABILITY_FULL: f64 = 50.0;
/// À partir de ce nombre de langues, la notoriété est citée comme raison.
const NOTABLE_LANGUAGES: u32 = 5;
/// Moyenne du groupe à partir de laquelle ses goûts sont cités comme raison.
const LIKED_AVERAGE: f64 = 4.0;
/// Pénalité de diversité par lieu de même catégorie déjà retenu à moins de 5 km.
const DIVERSITY_PENALTY: f64 = 8.0;
const DIVERSITY_RADIUS_M: f64 = 5_000.0;
/// Deux lieux homonymes à moins de 1 km sont considérés comme le même.
const DUPLICATE_RADIUS_M: f64 = 1_000.0;
/// Plafond de la pénalité de catégorie : varier sans faire remonter des lieux obscurs.
pub const SPREAD_MAX: f64 = 5.0;

const W_PROXIMITY: f64 = 30.0;
const W_NOTABILITY: f64 = 25.0;
/// Les signaux d'intérêt : moins que la notoriété, à laquelle ils sont liés, mais assez pour
/// départager deux lieux aussi connus l'un que l'autre.
const W_POPULARITY: f64 = 8.0;
const W_WIKIVOYAGE: f64 = 5.0;
const W_WORLD_HERITAGE: f64 = 8.0;
const W_AREA: f64 = 5.0;
/// Consultations mensuelles donnant la popularité maximale.
const POPULARITY_FULL: f64 = 20_000.0;
/// À partir de ces consultations mensuelles, la popularité est citée comme raison.
const POPULAR_VIEWS: u32 = 1_000;
/// Superficie d'espace protégé donnant le bonus maximal, et à partir de laquelle elle est citée.
const AREA_FULL_KM2: f64 = 1_000.0;
const LARGE_AREA_KM2: f64 = 50.0;
const W_TASTE: f64 = 15.0;
const W_WANTED: f64 = 12.0;
/// Force d'envie à partir de laquelle elle est citée comme raison.
const WANTED_REASON: f64 = 0.3;
const W_COMPLETENESS: f64 = 5.0;
const W_FREE_TIME: f64 = 10.0;
const SATURATION_STEP: f64 = 4.0;
const SATURATION_MAX: f64 = 12.0;

/// Lieu candidat. `category` est un identifiant opaque (les catégories de l'appelant).
#[derive(Debug, Clone)]
pub struct Candidate {
    pub location: LatLon,
    pub category: usize,
    /// Nom normalisé (détection des doublons).
    pub name_key: String,
    /// Durée de visite prévue.
    pub duration_s: u32,
    /// Langues de l'article Wikipédia (0 sans article).
    pub languages: u32,
    /// Part des informations utiles connues (0 à 1).
    pub completeness: f64,
    /// Fermé à chaque date du voyage : écarté.
    pub closed_all_days: bool,
    /// Répond aux envies du groupe (0 à 1 ; décroît à mesure que les avis arrivent).
    pub wanted: f64,
    /// A un guide Wikivoyage.
    pub wikivoyage: bool,
    /// Patrimoine mondial de l'UNESCO.
    pub world_heritage: bool,
    /// Superficie d'un espace protégé (parc national, réserve…), en km² ; `None` pour les
    /// autres lieux (la superficie d'une commune ne dit rien de son intérêt).
    pub protected_area_km2: Option<f64>,
    /// Consultations mensuelles de ses articles Wikipédia.
    pub monthly_views: u32,
}

/// Popularité actuelle (0 à 1) d'après les consultations mensuelles, échelle logarithmique.
pub fn popularity(monthly_views: u32) -> f64 {
    ((1.0 + f64::from(monthly_views)).ln() / (1.0 + POPULARITY_FULL).ln()).min(1.0)
}

/// Un jour du voyage vu par les suggestions.
#[derive(Debug, Clone)]
pub struct DayAnchors {
    /// Points de départ possibles (hébergements, sinon lieux du voyage).
    pub anchors: Vec<LatLon>,
    /// Mode servant à estimer les trajets.
    pub mode: Mode,
    /// Temps encore libre dans l'itinéraire (`None` : pas d'itinéraire).
    pub free_s: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Context {
    pub days: Vec<DayAnchors>,
    /// Moyenne des notes du groupe par catégorie (assez d'avis seulement).
    pub taste: HashMap<usize, f64>,
    /// Lieux déjà dans le voyage, par catégorie.
    pub in_trip: HashMap<usize, usize>,
    /// Suggestions écartées par le groupe, par catégorie.
    pub dismissed: HashMap<usize, usize>,
    /// Pénalité par lieu de même catégorie déjà retenu, où qu'il soit, plafonnée à
    /// [`SPREAD_MAX`] (0 : seulement les voisins, voir [`DIVERSITY_RADIUS_M`]).
    pub spread_penalty: f64,
}

/// Raison affichée à côté d'une suggestion.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// À `minutes` (estimation) du point de départ du jour `day`.
    Near { day: usize, minutes: u32 },
    /// Article Wikipédia dans `languages` langues.
    Notable { languages: u32 },
    /// Articles Wikipédia consultés `monthly_views` fois par mois.
    Popular { monthly_views: u32 },
    /// Guide Wikivoyage.
    Wikivoyage,
    /// Patrimoine mondial de l'UNESCO.
    WorldHeritage,
    /// Espace protégé de `km2` km².
    LargeArea { km2: u32 },
    /// Le groupe note bien cette catégorie.
    Taste { average: f64 },
    /// Le jour `day` a encore `minutes` libres dans l'itinéraire.
    FreeTime { day: usize, minutes: u32 },
    /// Répond aux envies (ou au budget) déclarés par le groupe.
    Wanted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    /// Indice dans les candidats.
    pub index: usize,
    pub score: f64,
    /// Jour le plus proche, et trajet estimé depuis son point de départ.
    pub day: Option<usize>,
    pub travel_s: u32,
    /// Raisons, de la plus à la moins déterminante.
    pub reasons: Vec<Reason>,
}

/// Jour le plus proche d'un point, et trajet (aller) depuis son point de départ le plus proche.
fn nearest_day(location: LatLon, days: &[DayAnchors]) -> Option<(usize, u32)> {
    days.iter()
        .enumerate()
        .filter_map(|(d, day)| {
            day.anchors
                .iter()
                .map(|&a| travel::straight_line(day.mode, a, location).duration_s)
                .min()
                .map(|s| (d, s))
        })
        // À trajet égal : le jour qui a le plus de temps libre, puis le premier.
        .min_by_key(|&(d, s)| (s, std::cmp::Reverse(days[d].free_s.unwrap_or(0)), d))
}

/// Score d'un candidat seul (avant diversification) ; `None` s'il est écarté.
fn score_one(index: usize, c: &Candidate, ctx: &Context) -> Option<Scored> {
    if c.closed_all_days {
        return None;
    }
    let (day, travel_s) = match nearest_day(c.location, &ctx.days) {
        Some((_, s)) if s > MAX_TRAVEL_S => return None,
        Some((d, s)) => (Some(d), s),
        None => (None, 0),
    };
    let mut parts: Vec<(f64, Option<Reason>)> = vec![];
    if let Some(d) = day {
        let proximity = 1.0 - (f64::from(travel_s) / PROXIMITY_HORIZON_S).min(1.0);
        parts.push((
            W_PROXIMITY * proximity,
            Some(Reason::Near {
                day: d,
                minutes: travel_s.div_ceil(60),
            }),
        ));
        if let Some(free) = ctx.days[d].free_s {
            if free >= c.duration_s + 2 * travel_s {
                parts.push((
                    W_FREE_TIME,
                    Some(Reason::FreeTime {
                        day: d,
                        minutes: free / 60,
                    }),
                ));
            }
        }
    }
    let notability = (1.0 + f64::from(c.languages)).ln() / (1.0 + NOTABILITY_FULL).ln();
    parts.push((
        W_NOTABILITY * notability.min(1.0),
        (c.languages >= NOTABLE_LANGUAGES).then_some(Reason::Notable {
            languages: c.languages,
        }),
    ));
    parts.push((
        W_POPULARITY * popularity(c.monthly_views),
        (c.monthly_views >= POPULAR_VIEWS).then_some(Reason::Popular {
            monthly_views: c.monthly_views,
        }),
    ));
    if c.wikivoyage {
        parts.push((W_WIKIVOYAGE, Some(Reason::Wikivoyage)));
    }
    if c.world_heritage {
        parts.push((W_WORLD_HERITAGE, Some(Reason::WorldHeritage)));
    }
    if let Some(km2) = c.protected_area_km2.filter(|a| *a > 0.0) {
        parts.push((
            W_AREA * ((1.0 + km2).ln() / (1.0 + AREA_FULL_KM2).ln()).min(1.0),
            (km2 >= LARGE_AREA_KM2).then_some(Reason::LargeArea {
                km2: km2.round() as u32,
            }),
        ));
    }
    if let Some(&average) = ctx.taste.get(&c.category) {
        parts.push((
            W_TASTE * (average - 3.0) / 2.0,
            (average >= LIKED_AVERAGE).then_some(Reason::Taste { average }),
        ));
    }
    let wanted = c.wanted.clamp(0.0, 1.0);
    parts.push((
        // Une envie compte surtout pour les lieux qui en valent la peine : un parc d'attractions
        // obscur ne passe pas devant un parc national célèbre.
        W_WANTED * wanted * (0.4 + 0.6 * notability.min(1.0)),
        (wanted >= WANTED_REASON).then_some(Reason::Wanted),
    ));
    parts.push((W_COMPLETENESS * c.completeness.clamp(0.0, 1.0), None));
    let crowded = ctx
        .in_trip
        .get(&c.category)
        .copied()
        .unwrap_or(0)
        .saturating_sub(2);
    let dismissed = ctx.dismissed.get(&c.category).copied().unwrap_or(0);
    parts.push((
        -(SATURATION_STEP * crowded as f64).min(SATURATION_MAX)
            - (SATURATION_STEP * dismissed as f64).min(SATURATION_MAX),
        None,
    ));

    let score = parts.iter().map(|(s, _)| s).sum();
    // Raisons par contribution décroissante (la proximité reste citée même si faible).
    parts.sort_by(|a, b| b.0.total_cmp(&a.0));
    let reasons = parts.into_iter().filter_map(|(_, r)| r).collect();
    Some(Scored {
        index,
        score,
        day,
        travel_s,
        reasons,
    })
}

/// Au plus `limit` suggestions, par score décroissant après diversification.
pub fn recommend(candidates: &[Candidate], ctx: &Context, limit: usize) -> Vec<Scored> {
    let mut pool: Vec<Scored> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| score_one(i, c, ctx))
        .collect();
    // Ordre stable : score, puis indice (les candidats arrivent dans un ordre déterministe).
    pool.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.index.cmp(&b.index)));

    let mut picked: Vec<Scored> = Vec::with_capacity(limit.min(pool.len()));
    while picked.len() < limit {
        let adjusted = |s: &Scored| {
            let c = &candidates[s.index];
            let same = picked
                .iter()
                .map(|p| &candidates[p.index])
                .filter(|o| o.category == c.category);
            let (all, near) = same.fold((0, 0), |(all, near), o| {
                let close = haversine_m(o.location, c.location) < DIVERSITY_RADIUS_M;
                (all + 1, near + usize::from(close))
            });
            s.score
                - DIVERSITY_PENALTY * near as f64
                - (ctx.spread_penalty * all as f64).min(SPREAD_MAX)
        };
        let best = pool
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                adjusted(a)
                    .total_cmp(&adjusted(b))
                    .then(b.index.cmp(&a.index))
            })
            .map(|(i, _)| i);
        let Some(i) = best else { break };
        let next = pool.swap_remove(i);
        let c = &candidates[next.index];
        let duplicate = picked.iter().any(|p| {
            let o = &candidates[p.index];
            !c.name_key.is_empty()
                && o.name_key == c.name_key
                && haversine_m(o.location, c.location) < DUPLICATE_RADIUS_M
        });
        if !duplicate {
            picked.push(next);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORTUNA: LatLon = LatLon::new(10.4706, -84.6453);
    const ARENAL: LatLon = LatLon::new(10.4621, -84.7034);
    const SAN_JOSE: LatLon = LatLon::new(9.9327, -84.0716);
    const NATURE: usize = 1;
    const FOOD: usize = 2;

    fn cand(location: LatLon, category: usize, name: &str) -> Candidate {
        Candidate {
            location,
            category,
            name_key: name.to_lowercase(),
            duration_s: 3600,
            languages: 0,
            completeness: 0.0,
            closed_all_days: false,
            wanted: 0.0,
            wikivoyage: false,
            world_heritage: false,
            protected_area_km2: None,
            monthly_views: 0,
        }
    }

    fn ctx(free_s: Option<u32>) -> Context {
        Context {
            days: vec![
                DayAnchors {
                    anchors: vec![SAN_JOSE],
                    mode: Mode::Car,
                    free_s: Some(0),
                },
                DayAnchors {
                    anchors: vec![FORTUNA],
                    mode: Mode::Car,
                    free_s,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn nearer_and_more_notable_places_rank_first_with_reasons() {
        let mut volcano = cand(ARENAL, NATURE, "Volcán Arenal");
        volcano.languages = 40;
        let viewpoint = cand(FORTUNA, NATURE, "Mirador");
        let far = cand(LatLon::new(8.6, -83.4), NATURE, "Très loin");
        let out = recommend(&[viewpoint, volcano, far], &ctx(None), 10);
        // Le lieu à plus de 2 h 30 n'est pas proposé.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].index, 1, "{out:?}");
        assert_eq!(out[0].day, Some(1));
        assert!(out[0].reasons.contains(&Reason::Notable { languages: 40 }));
        assert!(matches!(
            out[0].reasons[0],
            Reason::Notable { .. } | Reason::Near { .. }
        ));
    }

    #[test]
    fn group_taste_and_free_time_count() {
        let waterfall = cand(ARENAL, NATURE, "Catarata");
        let soda = cand(ARENAL, FOOD, "Soda");
        let mut c = ctx(Some(5 * 3600));
        c.taste.insert(NATURE, 4.6);
        c.taste.insert(FOOD, 2.0);
        let out = recommend(&[soda, waterfall], &c, 10);
        assert_eq!(out[0].index, 1);
        assert!(out[0].reasons.contains(&Reason::Taste { average: 4.6 }));
        assert!(out[0].reasons.iter().any(|r| matches!(
            r,
            Reason::FreeTime {
                day: 1,
                minutes: 300
            }
        )));
        // Note basse : pas de raison « goûts », score inférieur.
        assert!(!out[1]
            .reasons
            .iter()
            .any(|r| matches!(r, Reason::Taste { .. })));
        assert!(out[1].score < out[0].score);

        // Journée pleine : pas de bonus de temps libre.
        let full = recommend(&[cand(ARENAL, NATURE, "Catarata")], &ctx(Some(600)), 1);
        assert!(!full[0]
            .reasons
            .iter()
            .any(|r| matches!(r, Reason::FreeTime { .. })));
    }

    #[test]
    fn closed_duplicates_and_saturation() {
        let mut closed = cand(FORTUNA, NATURE, "Fermé");
        closed.closed_all_days = true;
        let a = cand(ARENAL, NATURE, "Mirador del Arenal");
        let mut b = cand(LatLon::new(10.4625, -84.7030), NATURE, "MIRADOR DEL ARENAL");
        b.completeness = 1.0;
        let out = recommend(&[closed, a, b], &ctx(None), 10);
        // Fermé tout le voyage : écarté ; homonyme voisin : un seul gardé (le plus complet).
        assert_eq!(out.iter().map(|s| s.index).collect::<Vec<_>>(), vec![2]);

        let mut c = ctx(None);
        let base = recommend(&[cand(ARENAL, FOOD, "Soda")], &c, 1)[0].score;
        c.in_trip.insert(FOOD, 5);
        c.dismissed.insert(FOOD, 1);
        let crowded = recommend(&[cand(ARENAL, FOOD, "Soda")], &c, 1)[0].score;
        assert!((base - crowded - 16.0).abs() < 1e-9, "{base} {crowded}");
    }

    #[test]
    fn results_are_diversified() {
        // Trois cafés au même endroit et un parc un peu moins bien noté : le parc remonte.
        let cafes: Vec<Candidate> = (0..3)
            .map(|i| {
                let mut c = cand(FORTUNA, FOOD, &format!("Café {i}"));
                c.completeness = 1.0;
                c
            })
            .collect();
        let mut all = cafes;
        all.push(cand(FORTUNA, NATURE, "Parque"));
        let out = recommend(&all, &ctx(None), 2);
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|s| s.index == 3), "{out:?}");
    }

    #[test]
    fn declared_interests_count_and_are_explained() {
        let plain = cand(ARENAL, NATURE, "Mirador");
        let mut wanted = cand(ARENAL, FOOD, "Soda");
        wanted.wanted = 1.0;
        let out = recommend(&[plain, wanted], &ctx(None), 2);
        assert_eq!(out[0].index, 1);
        assert!(out[0].reasons.contains(&Reason::Wanted));
        // Envie presque remplacée par les avis : plus citée.
        let mut faded = cand(ARENAL, FOOD, "Soda");
        faded.wanted = 0.1;
        let out = recommend(&[faded], &ctx(None), 1);
        assert!(!out[0].reasons.contains(&Reason::Wanted));
    }

    #[test]
    fn spread_penalty_varies_categories_country_wide() {
        // Parcs très connus, loin les uns des autres, et un musée un peu moins connu.
        let mut all: Vec<Candidate> = (0..4)
            .map(|i| {
                let mut c = cand(
                    LatLon::new(8.5 + f64::from(i), -84.0),
                    NATURE,
                    &format!("Parc {i}"),
                );
                c.languages = 40;
                c
            })
            .collect();
        let mut museum = cand(SAN_JOSE, FOOD, "Museo");
        museum.languages = 30;
        all.push(museum);
        let rank = |spread| {
            let c = Context {
                spread_penalty: spread,
                ..Default::default()
            };
            recommend(&all, &c, 5)
                .iter()
                .position(|s| s.index == 4)
                .unwrap()
        };
        assert_eq!(rank(0.0), 4);
        assert!(rank(1.0) < 4);
    }

    #[test]
    fn signals_of_interest_break_ties_and_are_explained() {
        // Deux parcs aussi connus : celui qui a un guide, le label UNESCO, une grande superficie
        // et beaucoup de lecteurs passe devant, et chaque signal est cité.
        let mut plain = cand(ARENAL, NATURE, "Parque A");
        plain.languages = 20;
        plain.protected_area_km2 = Some(20.0);
        plain.monthly_views = 300;
        let mut famous = cand(ARENAL, NATURE, "Parque B");
        famous.languages = 20;
        famous.wikivoyage = true;
        famous.world_heritage = true;
        famous.protected_area_km2 = Some(1_204.4);
        famous.monthly_views = 6_000;
        let out = recommend(&[plain, famous], &ctx(None), 2);
        assert_eq!(out[0].index, 1, "{out:?}");
        for r in [
            Reason::Wikivoyage,
            Reason::WorldHeritage,
            Reason::LargeArea { km2: 1_204 },
            Reason::Popular {
                monthly_views: 6_000,
            },
        ] {
            assert!(out[0].reasons.contains(&r), "{r:?} {out:?}");
        }
        // Petit parc, peu consulté : ces signaux comptent un peu, sans être cités.
        assert!(!out[1]
            .reasons
            .iter()
            .any(|r| matches!(r, Reason::LargeArea { .. } | Reason::Popular { .. })));
        let bare = recommend(&[cand(ARENAL, NATURE, "Parque A")], &ctx(None), 1)[0].score;
        assert!(out[1].score > bare);
        // Un lieu très consulté, sans article dans beaucoup de langues, remonte quand même.
        assert!(popularity(20_000) == 1.0 && popularity(0) == 0.0);
        assert!(popularity(1_000) > 0.6);
    }

    #[test]
    fn without_anchors_places_are_ranked_without_distance() {
        let mut notable = cand(SAN_JOSE, NATURE, "A");
        notable.languages = 10;
        let out = recommend(
            &[cand(ARENAL, NATURE, "B"), notable],
            &Context::default(),
            5,
        );
        assert_eq!(out[0].index, 1);
        assert_eq!(out[0].day, None);
    }
}
