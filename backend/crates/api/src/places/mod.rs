//! Recherche de lieux (OpenStreetMap) derrière un trait, avec cache.
//!
//! Le fournisseur réel est Nominatim ([`nominatim::Nominatim`]) ; les tests utilisent
//! [`StaticPlaces`]. Le cache évite de solliciter Nominatim pour des requêtes répétées
//! (politique d'usage : au plus une requête par seconde).

pub mod category;
pub mod nominatim;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub use category::Category;

use crate::error::AppError;
use crate::validate;

/// Type d'objet OpenStreetMap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OsmType {
    Node,
    Way,
    Relation,
}

impl OsmType {
    pub fn as_str(self) -> &'static str {
        match self {
            OsmType::Node => "node",
            OsmType::Way => "way",
            OsmType::Relation => "relation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "node" | "N" => OsmType::Node,
            "way" | "W" => OsmType::Way,
            "relation" | "R" => OsmType::Relation,
            _ => return None,
        })
    }
}

/// Référence vers un objet OpenStreetMap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OsmRef {
    #[serde(rename = "type")]
    pub osm_type: OsmType,
    pub id: i64,
}

/// Lieu trouvé dans OpenStreetMap, prêt à être ajouté comme POI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Place {
    /// Nom du lieu ; vide si l'objet n'en a pas (ex. adresse).
    pub name: String,
    /// Libellé complet (nom, ville, région, pays).
    pub display_name: String,
    pub lat: f64,
    pub lon: f64,
    pub category: Category,
    /// Tag OSM principal (ex. `tourism=museum`), à titre indicatif.
    pub osm_tag: String,
    pub osm: Option<OsmRef>,
    pub opening_hours: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    /// Identifiant Wikidata (ex. `Q190622`).
    pub wikidata: Option<String>,
    /// Article Wikipédia au format OSM `langue:Titre`.
    pub wikipedia: Option<String>,
    /// Code pays ISO en majuscules (ex. `CR`).
    pub country_code: Option<String>,
}

/// Informations d'un objet OSM reprises sur un lieu : la première valeur de chaque tag
/// (OSM sépare les valeurs multiples par `;`), les valeurs invalides étant ignorées.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OsmDetails {
    pub opening_hours: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub wikidata: Option<String>,
    pub wikipedia: Option<String>,
}

impl OsmDetails {
    pub fn from_tags<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Self {
        type Check = fn(Option<&str>) -> Result<Option<String>, AppError>;
        let tag = |keys: &[&str], check: Check| {
            let value = keys.iter().find_map(|k| get(k))?;
            check(Some(value.split(';').next().unwrap_or_default()))
                .ok()
                .flatten()
        };
        Self {
            opening_hours: get("opening_hours")
                .map(str::trim)
                .filter(|h| !h.is_empty() && h.len() <= 500)
                .map(str::to_owned),
            website: tag(&["website", "contact:website"], validate::website),
            phone: tag(
                &["phone", "contact:phone", "contact:mobile"],
                validate::phone,
            ),
            email: tag(&["email", "contact:email"], validate::contact_email),
            wikidata: tag(&["wikidata"], validate::wikidata),
            wikipedia: tag(&["wikipedia"], validate::wikipedia),
        }
    }
}

/// Zone de la carte affichée : les résultats proches sont privilégiés, sans exclure les autres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewbox {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl Viewbox {
    /// Lit `min_lon,min_lat,max_lon,max_lat`.
    pub fn parse(s: &str) -> Result<Self, AppError> {
        let bad = || AppError::BadRequest("zone de recherche invalide".into());
        let v: Vec<f64> = s
            .split(',')
            .map(|p| p.trim().parse::<f64>().map_err(|_| bad()))
            .collect::<Result<_, _>>()?;
        let [min_lon, min_lat, max_lon, max_lat] = v[..] else {
            return Err(bad());
        };
        let vb = Self {
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        };
        let lons_ok = [min_lon, max_lon]
            .iter()
            .all(|x| (-180.0..=180.0).contains(x));
        let lats_ok = [min_lat, max_lat]
            .iter()
            .all(|x| (-90.0..=90.0).contains(x));
        if lons_ok && lats_ok && min_lon < max_lon && min_lat < max_lat {
            Ok(vb)
        } else {
            Err(bad())
        }
    }

    /// Arrondi au dixième de degré (~11 km) : améliore le taux de succès du cache
    /// sans changer sensiblement le biais géographique.
    fn rounded(self) -> Self {
        let r = |x: f64| (x * 10.0).round() / 10.0;
        Self {
            min_lon: r(self.min_lon),
            min_lat: r(self.min_lat),
            max_lon: r(self.max_lon),
            max_lat: r(self.max_lat),
        }
    }
}

/// Erreur d'un fournisseur de lieux.
#[derive(Debug, thiserror::Error)]
pub enum PlaceError {
    #[error("le service de recherche de lieux est momentanément saturé")]
    Busy,
    #[error("le service de recherche de lieux est indisponible")]
    Unavailable(#[source] anyhow::Error),
}

impl From<PlaceError> for AppError {
    fn from(e: PlaceError) -> Self {
        match e {
            PlaceError::Busy => AppError::ServiceUnavailable(e.to_string()),
            PlaceError::Unavailable(ref source) => {
                tracing::warn!(error = ?source, "recherche de lieux en échec");
                AppError::ServiceUnavailable(e.to_string())
            }
        }
    }
}

#[async_trait]
pub trait PlaceProvider: Send + Sync {
    async fn search(&self, query: &str, viewbox: Option<Viewbox>)
        -> Result<Vec<Place>, PlaceError>;
    /// Lieu le plus proche d'un point, s'il y en a un.
    async fn reverse(&self, lat: f64, lon: f64) -> Result<Option<Place>, PlaceError>;
}

const CACHE_TTL: Duration = Duration::from_secs(24 * 3600);
const CACHE_MAX: usize = 5_000;

#[derive(Clone)]
enum Cached {
    Search(Vec<Place>),
    Reverse(Option<Box<Place>>),
}

/// Fournisseur + cache en mémoire (une instance ; TTL 24 h).
pub struct PlaceService {
    provider: Box<dyn PlaceProvider>,
    cache: Mutex<HashMap<String, (Instant, Cached)>>,
}

impl PlaceService {
    pub fn new(provider: impl PlaceProvider + 'static) -> Self {
        Self {
            provider: Box::new(provider),
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, key: &str) -> Option<Cached> {
        let cache = self.cache.lock().expect("mutex empoisonné");
        cache
            .get(key)
            .filter(|(at, _)| at.elapsed() < CACHE_TTL)
            .map(|(_, v)| v.clone())
    }

    fn put(&self, key: String, value: Cached) {
        let mut cache = self.cache.lock().expect("mutex empoisonné");
        if cache.len() >= CACHE_MAX {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
            if cache.len() >= CACHE_MAX {
                cache.clear();
            }
        }
        cache.insert(key, (Instant::now(), value));
    }

    pub async fn search(
        &self,
        query: &str,
        viewbox: Option<Viewbox>,
    ) -> Result<Vec<Place>, PlaceError> {
        let viewbox = viewbox.map(Viewbox::rounded);
        let key = format!("s|{}|{viewbox:?}", query.to_lowercase());
        if let Some(Cached::Search(v)) = self.get(&key) {
            return Ok(v);
        }
        let places = self.provider.search(query, viewbox).await?;
        self.put(key, Cached::Search(places.clone()));
        Ok(places)
    }

    pub async fn reverse(&self, lat: f64, lon: f64) -> Result<Option<Place>, PlaceError> {
        // ~1 m de précision : deux clics au même endroit partagent le cache.
        let key = format!("r|{lat:.5}|{lon:.5}");
        if let Some(Cached::Reverse(v)) = self.get(&key) {
            return Ok(v.map(|p| *p));
        }
        let place = self.provider.reverse(lat, lon).await?;
        self.put(key, Cached::Reverse(place.clone().map(Box::new)));
        Ok(place)
    }
}

/// Fournisseur figé pour les tests et le développement hors ligne : la recherche
/// renvoie les lieux dont le nom contient la requête, le géocodage inverse le plus proche.
#[derive(Default)]
pub struct StaticPlaces {
    pub places: Vec<Place>,
    pub calls: std::sync::atomic::AtomicUsize,
}

impl StaticPlaces {
    pub fn new(places: Vec<Place>) -> Self {
        Self {
            places,
            calls: Default::default(),
        }
    }
}

#[async_trait]
impl PlaceProvider for std::sync::Arc<StaticPlaces> {
    async fn search(
        &self,
        query: &str,
        _viewbox: Option<Viewbox>,
    ) -> Result<Vec<Place>, PlaceError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let q = query.to_lowercase();
        Ok(self
            .places
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&q))
            .cloned()
            .collect())
    }

    async fn reverse(&self, lat: f64, lon: f64) -> Result<Option<Place>, PlaceError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let here = planner::geo::LatLon::new(lat, lon);
        Ok(self
            .places
            .iter()
            .map(|p| {
                (
                    planner::geo::haversine_m(here, planner::geo::LatLon::new(p.lat, p.lon)),
                    p,
                )
            })
            .filter(|(d, _)| *d < 200.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use super::*;

    fn museum() -> Place {
        Place {
            name: "Museo Nacional".into(),
            display_name: "Museo Nacional, San José, Costa Rica".into(),
            lat: 9.9327,
            lon: -84.0716,
            category: Category::Museum,
            osm_tag: "tourism=museum".into(),
            osm: Some(OsmRef {
                osm_type: OsmType::Relation,
                id: 4144800,
            }),
            opening_hours: Some("Tu-Sa 08:30-16:30".into()),
            website: None,
            phone: None,
            email: None,
            wikidata: None,
            wikipedia: None,
            country_code: Some("CR".into()),
        }
    }

    #[test]
    fn viewbox_parsing() {
        let vb = Viewbox::parse("-85.1,8.0,-82.5,11.2").unwrap();
        assert_eq!(vb.min_lat, 8.0);
        for bad in [
            "",
            "1,2,3",
            "a,b,c,d",
            "-82,8,-85,11",
            "0,95,1,96",
            "-200,8,-82,11",
        ] {
            assert!(Viewbox::parse(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn repeated_queries_hit_the_cache() {
        let provider = Arc::new(StaticPlaces::new(vec![museum()]));
        let service = PlaceService::new(provider.clone());
        let vb = Viewbox::parse("-84.12,9.91,-84.03,9.96").ok();

        assert_eq!(service.search("museo", vb).await.unwrap().len(), 1);
        // Même requête (casse différente, zone quasi identique) : pas de nouvel appel.
        let vb2 = Viewbox::parse("-84.11,9.92,-84.04,9.95").ok();
        assert_eq!(service.search("MUSEO", vb2).await.unwrap().len(), 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        assert!(service.search("volcan", None).await.unwrap().is_empty());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);

        assert!(service.reverse(9.9327, -84.0716).await.unwrap().is_some());
        assert!(service.reverse(9.9327, -84.0716).await.unwrap().is_some());
        assert!(service.reverse(0.0, 0.0).await.unwrap().is_none());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 4);
    }
}
