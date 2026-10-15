//! Client Valhalla (moteur de routage auto-hébergé, données OpenStreetMap).
//!
//! <https://valhalla.github.io/valhalla/api/>

use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use planner::geo::LatLon;
use planner::travel::{Leg, LegSource, Mode};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{RoutedLeg, RoutingError, RoutingProvider};

/// Limites par défaut d'un serveur Valhalla (`service_limits`).
const MAX_MATRIX_PAIRS: usize = 2_500;
const MAX_ROUTE_POINTS: usize = 20;

/// Codes d'erreur Valhalla signifiant « pas de chemin » plutôt qu'une panne :
/// distance maximale dépassée (154), régions non reliées (170), aucune route près
/// d'un point (171), aucun chemin trouvé (442, 443).
const NO_PATH_CODES: [u64; 5] = [154, 170, 171, 442, 443];

pub struct Valhalla {
    http: reqwest::Client,
    base_url: String,
}

impl Valhalla {
    pub fn new(base_url: &str, user_agent: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(20))
            .build()
            .context("client HTTP Valhalla")?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }

    async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        action: &str,
        body: Value,
    ) -> Result<T, RoutingError> {
        let unavailable = |e: anyhow::Error| RoutingError::Unavailable(e);
        let res = self
            .http
            .post(format!("{}/{action}", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| unavailable(e.into()))?;
        let status = res.status();
        if status.is_client_error() {
            let err: Value = res.json().await.unwrap_or(Value::Null);
            let code = err["error_code"].as_u64().unwrap_or(0);
            if NO_PATH_CODES.contains(&code) {
                return Err(RoutingError::NoPath);
            }
            return Err(unavailable(anyhow::anyhow!(
                "Valhalla {action} : {status} {err}"
            )));
        }
        if !status.is_success() {
            return Err(unavailable(anyhow::anyhow!("Valhalla {action} : {status}")));
        }
        res.json()
            .await
            .with_context(|| format!("réponse Valhalla {action} illisible"))
            .map_err(unavailable)
    }
}

fn costing(mode: Mode) -> Result<&'static str, RoutingError> {
    match mode {
        Mode::Walk => Ok("pedestrian"),
        Mode::Bike => Ok("bicycle"),
        Mode::Car => Ok("auto"),
        Mode::Transit => Err(RoutingError::Unavailable(anyhow::anyhow!(
            "les transports en commun ne sont pas routés"
        ))),
    }
}

fn location(p: &LatLon) -> Value {
    json!({ "lat": p.lat, "lon": p.lon })
}

fn leg(time_s: f64, length_km: f64) -> Leg {
    Leg {
        duration_s: time_s.max(0.0).round() as u32,
        distance_m: (length_km.max(0.0) * 1000.0).round() as u32,
        source: LegSource::Router,
    }
}

#[derive(Deserialize)]
struct MatrixResponse {
    sources_to_targets: Vec<Vec<MatrixCell>>,
}

#[derive(Deserialize)]
struct MatrixCell {
    time: Option<f64>,
    /// Kilomètres.
    distance: Option<f64>,
}

#[derive(Deserialize)]
struct RouteResponse {
    trip: Trip,
}

#[derive(Deserialize)]
struct Trip {
    legs: Vec<TripLeg>,
}

#[derive(Deserialize)]
struct TripLeg {
    summary: Summary,
    /// Polyline encodée, précision 6.
    shape: String,
}

#[derive(Deserialize)]
struct Summary {
    time: f64,
    /// Kilomètres.
    length: f64,
}

#[async_trait]
impl RoutingProvider for Valhalla {
    fn max_matrix_pairs(&self) -> usize {
        MAX_MATRIX_PAIRS
    }

    fn max_route_points(&self) -> usize {
        MAX_ROUTE_POINTS
    }

    async fn matrix(
        &self,
        mode: Mode,
        sources: &[LatLon],
        targets: &[LatLon],
    ) -> Result<Vec<Vec<Option<Leg>>>, RoutingError> {
        let body = json!({
            "sources": sources.iter().map(location).collect::<Vec<_>>(),
            "targets": targets.iter().map(location).collect::<Vec<_>>(),
            "costing": costing(mode)?,
            "units": "kilometers",
            "verbose": true,
        });
        let res: MatrixResponse = self.post("sources_to_targets", body).await?;
        Ok(res
            .sources_to_targets
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|c| Some(leg(c.time?, c.distance?)))
                    .collect()
            })
            .collect())
    }

    async fn route(&self, mode: Mode, points: &[LatLon]) -> Result<Vec<RoutedLeg>, RoutingError> {
        let body = json!({
            "locations": points.iter().map(location).collect::<Vec<_>>(),
            "costing": costing(mode)?,
            "units": "kilometers",
            "directions_type": "none",
        });
        let res: RouteResponse = self.post("route", body).await?;
        res.trip
            .legs
            .into_iter()
            .map(|l| {
                Ok(RoutedLeg {
                    leg: leg(l.summary.time, l.summary.length),
                    geometry: decode_polyline6(&l.shape).ok_or_else(|| {
                        RoutingError::Unavailable(anyhow::anyhow!("tracé Valhalla illisible"))
                    })?,
                })
            })
            .collect()
    }
}

/// Décode une polyline encodée (algorithme Google, précision 1e-6 comme Valhalla).
pub fn decode_polyline6(encoded: &str) -> Option<Vec<LatLon>> {
    let mut points = Vec::new();
    let (mut lat, mut lon) = (0i64, 0i64);
    let mut bytes = encoded.bytes().peekable();
    let next_value = |bytes: &mut std::iter::Peekable<std::str::Bytes<'_>>| -> Option<i64> {
        let (mut result, mut shift) = (0i64, 0u32);
        loop {
            let b = i64::from(bytes.next()?).checked_sub(63)?;
            if !(0..64).contains(&b) || shift > 60 {
                return None;
            }
            result |= (b & 0x1f) << shift;
            shift += 5;
            if b < 0x20 {
                break;
            }
        }
        Some(if result & 1 == 1 {
            !(result >> 1)
        } else {
            result >> 1
        })
    };
    while bytes.peek().is_some() {
        lat += next_value(&mut bytes)?;
        lon += next_value(&mut bytes)?;
        points.push(LatLon::new(lat as f64 / 1e6, lon as f64 / 1e6));
    }
    Some(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_polyline6() {
        // Exemple de la documentation Valhalla / Google, encodé en précision 6.
        let pts = decode_polyline6("_izlhA~rlgdF_{geC~ywl@_kwzCn`{nI").unwrap();
        let expected = [(38.5, -120.2), (40.7, -120.95), (43.252, -126.453)];
        assert_eq!(pts.len(), 3);
        for (p, (lat, lon)) in pts.iter().zip(expected) {
            assert!(
                (p.lat - lat).abs() < 1e-9 && (p.lon - lon).abs() < 1e-9,
                "{p:?}"
            );
        }
        assert_eq!(decode_polyline6("").unwrap(), vec![]);
        assert!(decode_polyline6("_izlhA").is_none()); // longitude manquante
        assert!(decode_polyline6("\u{7f}").is_none());
    }

    #[test]
    fn converts_units() {
        let l = leg(743.947, 1.051);
        assert_eq!((l.duration_s, l.distance_m), (744, 1051));
        assert_eq!(l.source, LegSource::Router);
    }
}
