//! Utilitaires géographiques.

use serde::{Deserialize, Serialize};

/// Rayon moyen de la Terre en mètres.
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Coordonnée WGS84.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub const fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
}

/// Distance orthodromique (formule de Haversine) en mètres.
pub fn haversine_m(a: LatLon, b: LatLon) -> f64 {
    let (lat1, lat2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_point_is_zero() {
        let p = LatLon::new(9.9281, -84.0907);
        assert_eq!(haversine_m(p, p), 0.0);
    }

    #[test]
    fn san_jose_to_limon_is_about_115_km() {
        // San José -> Puerto Limón (à vol d'oiseau ≈ 114 km)
        let sj = LatLon::new(9.9281, -84.0907);
        let limon = LatLon::new(9.9907, -83.0359);
        let d = haversine_m(sj, limon);
        assert!((110_000.0..120_000.0).contains(&d), "distance = {d}");
    }

    #[test]
    fn is_symmetric() {
        let a = LatLon::new(10.4633, -84.7033);
        let b = LatLon::new(9.3923, -84.1370);
        assert!((haversine_m(a, b) - haversine_m(b, a)).abs() < 1e-6);
    }
}
