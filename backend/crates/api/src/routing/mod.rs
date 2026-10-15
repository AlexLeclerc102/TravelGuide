//! Temps de trajet (marche, vélo, voiture, transports en commun estimés).
//!
//! Le moteur réel est Valhalla ([`valhalla::Valhalla`]), derrière le trait
//! [`RoutingProvider`] ; les tests utilisent [`StaticRouter`]. [`RoutingService`] ne
//! renvoie jamais d'erreur : ce que le moteur ne calcule pas (panne, aucun chemin) est
//! estimé à vol d'oiseau, et chaque trajet indique sa source pour que l'interface
//! signale les approximations.

pub mod valhalla;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use planner::geo::LatLon;
use planner::travel::{self, Leg, LegSource, Mode};

/// Erreur d'un moteur de routage.
#[derive(Debug, thiserror::Error)]
pub enum RoutingError {
    /// Aucun chemin entre ces points (région non reliée, lieu loin de toute route, trop loin).
    #[error("aucun chemin trouvé")]
    NoPath,
    #[error("moteur de routage indisponible")]
    Unavailable(#[source] anyhow::Error),
}

/// Trajet avec son tracé (de départ à arrivée).
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedLeg {
    pub leg: Leg,
    pub geometry: Vec<LatLon>,
}

#[async_trait]
pub trait RoutingProvider: Send + Sync {
    /// Nombre maximal de paires (sources × cibles) par requête de matrice.
    fn max_matrix_pairs(&self) -> usize;
    /// Nombre maximal de points par requête d'itinéraire.
    fn max_route_points(&self) -> usize;
    /// Trajets de chaque source vers chaque cible (`None` : pas de chemin pour cette paire).
    /// Jamais appelé pour [`Mode::Transit`].
    async fn matrix(
        &self,
        mode: Mode,
        sources: &[LatLon],
        targets: &[LatLon],
    ) -> Result<Vec<Vec<Option<Leg>>>, RoutingError>;
    /// Itinéraire passant par `points` dans l'ordre : un trajet par paire consécutive.
    /// Jamais appelé pour [`Mode::Transit`].
    async fn route(&self, mode: Mode, points: &[LatLon]) -> Result<Vec<RoutedLeg>, RoutingError>;
}

const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const CACHE_MAX: usize = 200_000;

/// Coordonnées arrondies au 1e-5 degré (~1 m) : clé de cache.
type PointKey = (i64, i64);
type CacheKey = (Mode, PointKey, PointKey);

fn point_key(p: LatLon) -> PointKey {
    ((p.lat * 1e5).round() as i64, (p.lon * 1e5).round() as i64)
}

fn zero_leg() -> Leg {
    Leg {
        duration_s: 0,
        distance_m: 0,
        source: LegSource::Router,
    }
}

/// Moteur de routage + repli à vol d'oiseau + cache en mémoire des trajets calculés.
pub struct RoutingService {
    provider: Option<Box<dyn RoutingProvider>>,
    cache: Mutex<HashMap<CacheKey, (Instant, Leg)>>,
}

impl RoutingService {
    pub fn new(provider: impl RoutingProvider + 'static) -> Self {
        Self {
            provider: Some(Box::new(provider)),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Sans moteur : tous les trajets sont estimés à vol d'oiseau.
    pub fn straight_line_only() -> Self {
        Self {
            provider: None,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cached(&self, mode: Mode, a: LatLon, b: LatLon) -> Option<Leg> {
        let cache = self.cache.lock().expect("mutex empoisonné");
        cache
            .get(&(mode, point_key(a), point_key(b)))
            .filter(|(at, _)| at.elapsed() < CACHE_TTL)
            .map(|(_, leg)| *leg)
    }

    /// Seuls les trajets calculés par le moteur sont mis en cache : un repli sera
    /// recalculé dès que le moteur répondra.
    fn remember(&self, mode: Mode, a: LatLon, b: LatLon, leg: Leg) {
        if leg.source != LegSource::Router {
            return;
        }
        let mut cache = self.cache.lock().expect("mutex empoisonné");
        if cache.len() >= CACHE_MAX {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
            if cache.len() >= CACHE_MAX {
                cache.clear();
            }
        }
        cache.insert((mode, point_key(a), point_key(b)), (Instant::now(), leg));
    }

    /// Matrice N×N des trajets entre `points` (ligne `i` : départ de `points[i]`).
    ///
    /// Les transports en commun sont dérivés de la matrice voiture. Les requêtes au moteur
    /// sont découpées par groupes de lignes ; un groupe sans chemin est réessayé ligne par
    /// ligne, et ce qui reste sans réponse est estimé à vol d'oiseau.
    pub async fn matrix(&self, mode: Mode, points: &[LatLon]) -> Vec<Vec<Leg>> {
        if mode == Mode::Transit {
            let car = Box::pin(self.matrix(Mode::Car, points)).await;
            return car
                .into_iter()
                .map(|row| row.into_iter().map(travel::transit_from_car).collect())
                .collect();
        }
        let n = points.len();
        let mut grid: Vec<Vec<Option<Leg>>> = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| {
                        if i == j {
                            Some(zero_leg())
                        } else {
                            self.cached(mode, points[i], points[j])
                        }
                    })
                    .collect()
            })
            .collect();

        if let Some(provider) = &self.provider {
            let missing: Vec<usize> = (0..n)
                .filter(|&i| grid[i].iter().any(Option::is_none))
                .collect();
            let rows_per_request = (provider.max_matrix_pairs() / n.max(1)).max(1);
            let mut down = false;
            for chunk in missing.chunks(rows_per_request) {
                if down {
                    break;
                }
                // Un groupe sans chemin est réessayé ligne par ligne.
                let attempts: Vec<&[usize]> = match self
                    .fetch_rows(provider.as_ref(), mode, points, chunk, &mut grid)
                    .await
                {
                    Err(RoutingError::NoPath) if chunk.len() > 1 => chunk.chunks(1).collect(),
                    Err(e) => {
                        down = self.note_failure(e);
                        vec![]
                    }
                    Ok(()) => vec![],
                };
                for row in attempts {
                    match self
                        .fetch_rows(provider.as_ref(), mode, points, row, &mut grid)
                        .await
                    {
                        Ok(()) | Err(RoutingError::NoPath) => {}
                        Err(e) => {
                            down = self.note_failure(e);
                            break;
                        }
                    }
                }
            }
        }

        grid.into_iter()
            .enumerate()
            .map(|(i, row)| {
                row.into_iter()
                    .enumerate()
                    .map(|(j, leg)| {
                        leg.unwrap_or_else(|| travel::straight_line(mode, points[i], points[j]))
                    })
                    .collect()
            })
            .collect()
    }

    async fn fetch_rows(
        &self,
        provider: &dyn RoutingProvider,
        mode: Mode,
        points: &[LatLon],
        rows: &[usize],
        grid: &mut [Vec<Option<Leg>>],
    ) -> Result<(), RoutingError> {
        let sources: Vec<LatLon> = rows.iter().map(|&i| points[i]).collect();
        let result = provider.matrix(mode, &sources, points).await?;
        if result.len() != rows.len() || result.iter().any(|r| r.len() != points.len()) {
            return Err(RoutingError::Unavailable(anyhow::anyhow!(
                "matrice de taille inattendue"
            )));
        }
        for (&i, row) in rows.iter().zip(result) {
            for (j, leg) in row.into_iter().enumerate() {
                if i != j && grid[i][j].is_none() {
                    if let Some(leg) = leg {
                        self.remember(mode, points[i], points[j], leg);
                        grid[i][j] = Some(leg);
                    }
                }
            }
        }
        Ok(())
    }

    /// Journalise une panne du moteur ; renvoie `true` (moteur considéré hors service
    /// pour le reste de la requête, afin de ne pas cumuler les délais d'attente).
    fn note_failure(&self, e: RoutingError) -> bool {
        tracing::warn!(error = ?e, "moteur de routage en échec : estimation à vol d'oiseau");
        true
    }

    /// Itinéraire passant par `points` dans l'ordre : un trajet (avec tracé) par paire
    /// consécutive. Les transports en commun reprennent le tracé de la voiture.
    pub async fn route(&self, mode: Mode, points: &[LatLon]) -> Vec<RoutedLeg> {
        if mode == Mode::Transit {
            let car = Box::pin(self.route(Mode::Car, points)).await;
            return car
                .into_iter()
                .map(|r| RoutedLeg {
                    leg: travel::transit_from_car(r.leg),
                    geometry: r.geometry,
                })
                .collect();
        }
        if points.len() < 2 {
            return vec![];
        }
        let fallback = |a: LatLon, b: LatLon| RoutedLeg {
            leg: travel::straight_line(mode, a, b),
            geometry: vec![a, b],
        };
        let Some(provider) = &self.provider else {
            return points.windows(2).map(|w| fallback(w[0], w[1])).collect();
        };

        let mut legs = Vec::with_capacity(points.len() - 1);
        let mut down = false;
        // Fenêtres de `max_route_points` points qui se chevauchent d'un point.
        let step = provider.max_route_points().max(2) - 1;
        let mut start = 0;
        while start < points.len() - 1 {
            let window = &points[start..(start + step + 1).min(points.len())];
            start += step;
            if down {
                legs.extend(window.windows(2).map(|w| fallback(w[0], w[1])));
                continue;
            }
            match provider.route(mode, window).await {
                Ok(routed) if routed.len() == window.len() - 1 => legs.extend(routed),
                Ok(_) => {
                    down = self.note_failure(RoutingError::Unavailable(anyhow::anyhow!(
                        "nombre de trajets inattendu"
                    )));
                    legs.extend(window.windows(2).map(|w| fallback(w[0], w[1])));
                }
                // Un trajet impossible fait échouer toute la requête : on reprend paire par paire.
                Err(RoutingError::NoPath) => {
                    for w in window.windows(2) {
                        let leg = if down {
                            None
                        } else {
                            match provider.route(mode, w).await {
                                Ok(mut r) if r.len() == 1 => r.pop(),
                                Ok(_) | Err(RoutingError::NoPath) => None,
                                Err(e) => {
                                    down = self.note_failure(e);
                                    None
                                }
                            }
                        };
                        legs.push(leg.unwrap_or_else(|| fallback(w[0], w[1])));
                    }
                }
                Err(e) => {
                    down = self.note_failure(e);
                    legs.extend(window.windows(2).map(|w| fallback(w[0], w[1])));
                }
            }
        }
        for (w, r) in points.windows(2).zip(&legs) {
            self.remember(mode, w[0], w[1], r.leg);
        }
        legs
    }
}

/// Faux moteur pour les tests : durées « à vol d'oiseau » marquées comme calculées,
/// aucun chemin vers ou depuis l'hémisphère sud (`lat < 0`), panne simulable.
pub struct StaticRouter {
    pub calls: AtomicUsize,
    pub down: AtomicBool,
    pub max_pairs: usize,
    pub max_points: usize,
}

impl Default for StaticRouter {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            down: AtomicBool::new(false),
            max_pairs: 2_500,
            max_points: 20,
        }
    }
}

impl StaticRouter {
    fn leg(mode: Mode, a: LatLon, b: LatLon) -> Option<Leg> {
        (a.lat >= 0.0 && b.lat >= 0.0).then(|| Leg {
            source: LegSource::Router,
            ..travel::straight_line(mode, a, b)
        })
    }

    fn enter(&self) -> Result<(), RoutingError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.down.load(Ordering::SeqCst) {
            Err(RoutingError::Unavailable(anyhow::anyhow!("panne simulée")))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl RoutingProvider for std::sync::Arc<StaticRouter> {
    fn max_matrix_pairs(&self) -> usize {
        self.max_pairs
    }

    fn max_route_points(&self) -> usize {
        self.max_points
    }

    async fn matrix(
        &self,
        mode: Mode,
        sources: &[LatLon],
        targets: &[LatLon],
    ) -> Result<Vec<Vec<Option<Leg>>>, RoutingError> {
        self.enter()?;
        assert!(mode.is_routed() && sources.len() * targets.len() <= self.max_pairs);
        Ok(sources
            .iter()
            .map(|&a| {
                targets
                    .iter()
                    .map(|&b| StaticRouter::leg(mode, a, b))
                    .collect()
            })
            .collect())
    }

    async fn route(&self, mode: Mode, points: &[LatLon]) -> Result<Vec<RoutedLeg>, RoutingError> {
        self.enter()?;
        assert!(mode.is_routed() && points.len() <= self.max_points);
        points
            .windows(2)
            .map(|w| {
                let leg = StaticRouter::leg(mode, w[0], w[1]).ok_or(RoutingError::NoPath)?;
                let mid = LatLon::new((w[0].lat + w[1].lat) / 2.0, (w[0].lon + w[1].lon) / 2.0);
                Ok(RoutedLeg {
                    leg,
                    geometry: vec![w[0], mid, w[1]],
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    const SAN_JOSE: LatLon = LatLon::new(9.9327, -84.0716);
    const FORTUNA: LatLon = LatLon::new(10.4621, -84.7034);
    const MONTEVERDE: LatLon = LatLon::new(10.3009, -84.8257);
    const NOWHERE: LatLon = LatLon::new(-10.0, -84.0);

    fn service(router: &Arc<StaticRouter>) -> RoutingService {
        RoutingService::new(router.clone())
    }

    #[tokio::test]
    async fn matrix_is_routed_and_cached() {
        let router = Arc::new(StaticRouter::default());
        let svc = service(&router);
        let pts = [SAN_JOSE, FORTUNA, MONTEVERDE];
        let m = svc.matrix(Mode::Car, &pts).await;
        assert_eq!(m.len(), 3);
        assert_eq!(m[0][0].duration_s, 0);
        assert!(m.iter().flatten().all(|l| l.source == LegSource::Router));
        assert_eq!(router.calls.load(Ordering::SeqCst), 1);

        // Deuxième appel (même sous-ensemble, autre ordre) : tout vient du cache.
        let again = svc.matrix(Mode::Car, &[FORTUNA, SAN_JOSE]).await;
        assert_eq!(again[0][1], m[1][0]);
        assert_eq!(router.calls.load(Ordering::SeqCst), 1);
        // Autre mode : nouveau calcul.
        svc.matrix(Mode::Walk, &pts).await;
        assert_eq!(router.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn matrix_requests_are_chunked() {
        let router = Arc::new(StaticRouter {
            max_pairs: 6,
            ..Default::default()
        });
        let svc = service(&router);
        let pts = [SAN_JOSE, FORTUNA, MONTEVERDE];
        // 6 paires par requête et 3 cibles : 2 lignes, puis 1.
        let m = svc.matrix(Mode::Bike, &pts).await;
        assert_eq!(router.calls.load(Ordering::SeqCst), 2);
        assert!(m.iter().flatten().all(|l| l.source == LegSource::Router));
    }

    #[tokio::test]
    async fn unreachable_pairs_fall_back_to_straight_line() {
        let router = Arc::new(StaticRouter::default());
        let svc = service(&router);
        let m = svc.matrix(Mode::Car, &[SAN_JOSE, NOWHERE, FORTUNA]).await;
        assert_eq!(m[0][2].source, LegSource::Router);
        assert_eq!(m[0][1].source, LegSource::StraightLine);
        assert_eq!(m[1][2].source, LegSource::StraightLine);
        assert!(m[0][1].duration_s > 0);
    }

    #[tokio::test]
    async fn outage_falls_back_without_caching() {
        let router = Arc::new(StaticRouter {
            max_pairs: 3,
            ..Default::default()
        });
        router.down.store(true, Ordering::SeqCst);
        let svc = service(&router);
        let pts = [SAN_JOSE, FORTUNA, MONTEVERDE];
        let m = svc.matrix(Mode::Car, &pts).await;
        assert!(m[0][1].source == LegSource::StraightLine);
        // Panne détectée dès la première requête : pas d'autre tentative.
        assert_eq!(router.calls.load(Ordering::SeqCst), 1);

        router.down.store(false, Ordering::SeqCst);
        let m = svc.matrix(Mode::Car, &pts).await;
        assert!(m.iter().flatten().all(|l| l.source == LegSource::Router));
    }

    #[tokio::test]
    async fn transit_is_derived_from_car() {
        let router = Arc::new(StaticRouter::default());
        let svc = service(&router);
        let pts = [SAN_JOSE, FORTUNA];
        let car = svc.matrix(Mode::Car, &pts).await;
        let transit = svc.matrix(Mode::Transit, &pts).await;
        assert_eq!(transit[0][1], travel::transit_from_car(car[0][1]));
        assert_eq!(transit[0][0].duration_s, 0);
        let legs = svc.route(Mode::Transit, &pts).await;
        assert_eq!(legs[0].leg.source, LegSource::TransitEstimate);
        assert_eq!(legs[0].geometry.len(), 3);
    }

    #[tokio::test]
    async fn route_windows_and_no_path_retries() {
        let router = Arc::new(StaticRouter {
            max_points: 3,
            ..Default::default()
        });
        let svc = service(&router);
        // 4 points, fenêtres de 3 : [0,1,2] puis [2,3].
        let legs = svc
            .route(Mode::Car, &[SAN_JOSE, FORTUNA, MONTEVERDE, SAN_JOSE])
            .await;
        assert_eq!(legs.len(), 3);
        assert_eq!(router.calls.load(Ordering::SeqCst), 2);
        assert!(legs.iter().all(|l| l.leg.source == LegSource::Router));
        // Le trajet suit les points.
        assert_eq!(legs[1].geometry.first(), Some(&FORTUNA));
        assert_eq!(legs[1].geometry.last(), Some(&MONTEVERDE));

        // Une paire impossible : les autres restent calculées.
        let legs = svc.route(Mode::Walk, &[SAN_JOSE, FORTUNA, NOWHERE]).await;
        assert_eq!(legs[0].leg.source, LegSource::Router);
        assert_eq!(legs[1].leg.source, LegSource::StraightLine);
        assert_eq!(legs[1].geometry, vec![FORTUNA, NOWHERE]);
    }

    #[tokio::test]
    async fn without_provider_everything_is_estimated() {
        let svc = RoutingService::straight_line_only();
        let legs = svc.route(Mode::Bike, &[SAN_JOSE, FORTUNA]).await;
        assert_eq!(legs[0].leg.source, LegSource::StraightLine);
        let m = svc.matrix(Mode::Walk, &[SAN_JOSE, FORTUNA]).await;
        assert_eq!(m[1][0].source, LegSource::StraightLine);
    }
}
