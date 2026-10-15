//! Moyens de transport et estimations de trajet.
//!
//! Le routage réel (Valhalla) est fait par l'API ; ce module fournit ce qui doit rester
//! déterministe et testable sans réseau :
//! - l'estimation à vol d'oiseau, utilisée en repli quand le moteur de routage ne répond pas ;
//! - l'approximation des transports en commun (aucune donnée GTFS exploitable au Costa Rica) ;
//! - le choix du moyen de transport d'une étape parmi ceux autorisés.

use serde::{Deserialize, Serialize};

use crate::geo::{haversine_m, LatLon};

/// Moyen de transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Walk,
    Bike,
    Car,
    /// Transports en commun : toujours une estimation (voir [`transit_from_car`]).
    Transit,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Walk, Mode::Bike, Mode::Car, Mode::Transit];

    /// Modes calculés par le moteur de routage (les transports en commun en sont dérivés).
    pub fn is_routed(self) -> bool {
        self != Mode::Transit
    }

    /// Vitesse moyenne retenue pour l'estimation à vol d'oiseau, en m/s.
    fn fallback_speed_mps(self) -> f64 {
        match self {
            Mode::Walk => 4.5 / 3.6,
            Mode::Bike => 14.0 / 3.6,
            // Routes de montagne et traversées de villes : 40 km/h de moyenne au Costa Rica.
            Mode::Car | Mode::Transit => 40.0 / 3.6,
        }
    }

    /// Rapport entre la distance par la route et la distance à vol d'oiseau.
    fn detour_factor(self) -> f64 {
        match self {
            Mode::Walk | Mode::Bike => 1.3,
            Mode::Car | Mode::Transit => 1.4,
        }
    }
}

/// Origine d'une durée de trajet, affichée pour signaler les approximations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegSource {
    /// Calculée par le moteur de routage sur le réseau OpenStreetMap.
    Router,
    /// Estimée à vol d'oiseau (moteur indisponible ou aucun chemin trouvé).
    StraightLine,
    /// Transports en commun estimés à partir du trajet en voiture.
    TransitEstimate,
}

impl LegSource {
    /// La durée est-elle une approximation ?
    pub fn is_estimate(self) -> bool {
        self != LegSource::Router
    }
}

/// Trajet entre deux points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Leg {
    pub duration_s: u32,
    pub distance_m: u32,
    pub source: LegSource,
}

/// Estimation à vol d'oiseau : distance × facteur de détour, à vitesse moyenne constante.
pub fn straight_line(mode: Mode, from: LatLon, to: LatLon) -> Leg {
    let distance = haversine_m(from, to) * mode.detour_factor();
    let leg = Leg {
        duration_s: (distance / mode.fallback_speed_mps()).round() as u32,
        distance_m: distance.round() as u32,
        source: LegSource::StraightLine,
    };
    if mode == Mode::Transit {
        transit_from_car(leg)
    } else {
        leg
    }
}

/// Multiplicateur appliqué au temps en voiture pour estimer les transports en commun.
pub const TRANSIT_FACTOR: f64 = 1.8;
/// Attente et marche d'accès ajoutées à chaque trajet en transports en commun (secondes).
pub const TRANSIT_OVERHEAD_S: u32 = 15 * 60;

/// Transports en commun estimés depuis un trajet en voiture : `temps × 1,8 + 15 min`.
///
/// Aucune donnée d'horaires exploitable ne couvre le Costa Rica : l'interface présente
/// toujours ce résultat comme une estimation. Un trajet nul reste nul.
pub fn transit_from_car(car: Leg) -> Leg {
    let duration_s = if car.distance_m == 0 {
        0
    } else {
        (f64::from(car.duration_s) * TRANSIT_FACTOR).round() as u32 + TRANSIT_OVERHEAD_S
    };
    Leg {
        duration_s,
        distance_m: car.distance_m,
        source: LegSource::TransitEstimate,
    }
}

/// À pied tant que le trajet ne dépasse pas cette durée (secondes).
pub const WALK_PREFERRED_MAX_S: u32 = 20 * 60;
/// À vélo tant que le trajet ne dépasse pas cette durée (secondes).
pub const BIKE_PREFERRED_MAX_S: u32 = 45 * 60;

/// Choisit le moyen de transport d'une étape parmi les options autorisées.
///
/// Règle (déterministe) : la marche si elle prend au plus 20 min, sinon le vélo s'il prend
/// au plus 45 min, sinon l'option la plus rapide. À durée égale, l'ordre de [`Mode`]
/// départage (marche, vélo, voiture, transports en commun).
pub fn choose_mode(options: &[(Mode, Leg)]) -> Option<(Mode, Leg)> {
    let find = |mode: Mode, max_s: u32| {
        options
            .iter()
            .find(|(m, leg)| *m == mode && leg.duration_s <= max_s)
            .copied()
    };
    find(Mode::Walk, WALK_PREFERRED_MAX_S)
        .or_else(|| find(Mode::Bike, BIKE_PREFERRED_MAX_S))
        .or_else(|| {
            options
                .iter()
                .min_by_key(|(m, leg)| (leg.duration_s, *m))
                .copied()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAN_JOSE: LatLon = LatLon::new(9.9327, -84.0716);
    const LA_FORTUNA: LatLon = LatLon::new(10.4621, -84.7034);

    fn leg(duration_min: u32) -> Leg {
        Leg {
            duration_s: duration_min * 60,
            distance_m: duration_min * 500,
            source: LegSource::Router,
        }
    }

    #[test]
    fn straight_line_is_flagged_and_scaled_by_mode() {
        let walk = straight_line(Mode::Walk, SAN_JOSE, LA_FORTUNA);
        let car = straight_line(Mode::Car, SAN_JOSE, LA_FORTUNA);
        assert_eq!(walk.source, LegSource::StraightLine);
        assert!(car.duration_s < walk.duration_s);
        // ~91 km à vol d'oiseau × 1,4 ≈ 128 km, à 40 km/h ≈ 3 h 10.
        assert!((120_000..135_000).contains(&car.distance_m), "{car:?}");
        assert!((10_800..12_600).contains(&car.duration_s), "{car:?}");
    }

    #[test]
    fn straight_line_transit_is_a_transit_estimate() {
        let car = straight_line(Mode::Car, SAN_JOSE, LA_FORTUNA);
        let transit = straight_line(Mode::Transit, SAN_JOSE, LA_FORTUNA);
        assert_eq!(transit.source, LegSource::TransitEstimate);
        assert_eq!(transit, transit_from_car(car));
    }

    #[test]
    fn same_point_costs_nothing() {
        for mode in Mode::ALL {
            let l = straight_line(mode, SAN_JOSE, SAN_JOSE);
            assert_eq!((l.duration_s, l.distance_m), (0, 0), "{mode:?}");
        }
    }

    #[test]
    fn transit_formula() {
        let t = transit_from_car(leg(60));
        assert_eq!(t.duration_s, (60.0 * 60.0 * 1.8) as u32 + 15 * 60);
        assert_eq!(t.distance_m, 30_000);
        assert_eq!(t.source, LegSource::TransitEstimate);
        assert!(t.source.is_estimate());
        assert!(!LegSource::Router.is_estimate());
    }

    #[test]
    fn short_walks_are_preferred() {
        let options = [(Mode::Walk, leg(18)), (Mode::Car, leg(5))];
        assert_eq!(choose_mode(&options).unwrap().0, Mode::Walk);
    }

    #[test]
    fn long_walks_give_way_to_bike_then_fastest() {
        let options = [
            (Mode::Walk, leg(50)),
            (Mode::Bike, leg(25)),
            (Mode::Car, leg(10)),
        ];
        assert_eq!(choose_mode(&options).unwrap().0, Mode::Bike);
        let options = [
            (Mode::Walk, leg(150)),
            (Mode::Bike, leg(60)),
            (Mode::Car, leg(25)),
            (Mode::Transit, leg(60)),
        ];
        assert_eq!(choose_mode(&options).unwrap().0, Mode::Car);
    }

    #[test]
    fn walking_beats_slower_transit() {
        // Marche de 25 min (au-delà du seuil) mais plus rapide que le bus estimé.
        let options = [(Mode::Walk, leg(25)), (Mode::Transit, leg(40))];
        assert_eq!(choose_mode(&options).unwrap().0, Mode::Walk);
    }

    #[test]
    fn ties_follow_mode_order_and_empty_gives_none() {
        let options = [(Mode::Transit, leg(30)), (Mode::Car, leg(30))];
        assert_eq!(choose_mode(&options).unwrap().0, Mode::Car);
        assert!(choose_mode(&[]).is_none());
    }
}
