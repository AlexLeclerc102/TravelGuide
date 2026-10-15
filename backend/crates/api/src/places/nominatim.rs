//! Client Nominatim (géocodage OpenStreetMap).
//!
//! Politique d'usage du service public : User-Agent identifiant l'application,
//! au plus une requête par seconde, pas d'autocomplétion, attribution ODbL.
//! <https://operations.osmfoundation.org/policies/nominatim/>

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;
use tokio::time::Instant;

use super::{Category, OsmDetails, OsmRef, OsmType, Place, PlaceError, PlaceProvider, Viewbox};

/// Intervalle minimal entre deux requêtes (toutes requêtes confondues).
const MIN_INTERVAL: Duration = Duration::from_secs(1);
/// Au-delà de cette attente, on refuse plutôt que d'accumuler les requêtes.
const MAX_WAIT: Duration = Duration::from_secs(5);
const RESULT_LIMIT: &str = "8";

pub struct Nominatim {
    http: reqwest::Client,
    base_url: String,
    next_slot: Mutex<Instant>,
}

impl Nominatim {
    pub fn new(base_url: &str, user_agent: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(10))
            .build()
            .context("client HTTP Nominatim")?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            next_slot: Mutex::new(Instant::now()),
        })
    }

    /// Réserve le prochain créneau d'une seconde et attend son heure.
    async fn throttle(&self) -> Result<(), PlaceError> {
        let wait = {
            let mut next = self.next_slot.lock().expect("mutex empoisonné");
            let now = Instant::now();
            let slot = (*next).max(now);
            if slot - now > MAX_WAIT {
                return Err(PlaceError::Busy);
            }
            *next = slot + MIN_INTERVAL;
            slot - now
        };
        tokio::time::sleep(wait).await;
        Ok(())
    }

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<String, PlaceError> {
        self.throttle().await?;
        let res = self
            .http
            .get(format!("{}{path}", self.base_url))
            .query(params)
            .query(&[
                ("format", "jsonv2"),
                ("addressdetails", "1"),
                ("extratags", "1"),
                ("accept-language", "fr"),
            ])
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| PlaceError::Unavailable(e.into()))?;
        res.text()
            .await
            .map_err(|e| PlaceError::Unavailable(e.into()))
    }
}

#[async_trait]
impl PlaceProvider for Nominatim {
    async fn search(
        &self,
        query: &str,
        viewbox: Option<Viewbox>,
    ) -> Result<Vec<Place>, PlaceError> {
        let mut params = vec![("q", query.to_owned()), ("limit", RESULT_LIMIT.to_owned())];
        if let Some(vb) = viewbox {
            params.push((
                "viewbox",
                format!(
                    "{},{},{},{}",
                    vb.min_lon, vb.min_lat, vb.max_lon, vb.max_lat
                ),
            ));
        }
        let body = self.get("/search", &params).await?;
        parse_search(&body).map_err(PlaceError::Unavailable)
    }

    async fn reverse(&self, lat: f64, lon: f64) -> Result<Option<Place>, PlaceError> {
        let params = [
            ("lat", lat.to_string()),
            ("lon", lon.to_string()),
            ("zoom", "18".to_owned()),
        ];
        let body = self.get("/reverse", &params).await?;
        parse_reverse(&body).map_err(PlaceError::Unavailable)
    }
}

#[derive(Deserialize)]
struct Item {
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_name: String,
    lat: String,
    lon: String,
    #[serde(default)]
    category: String,
    #[serde(default, rename = "type")]
    kind: String,
    osm_type: Option<String>,
    osm_id: Option<i64>,
    #[serde(default)]
    address: HashMap<String, String>,
    #[serde(default)]
    extratags: Option<HashMap<String, String>>,
}

impl Item {
    fn into_place(self) -> anyhow::Result<Place> {
        let lat: f64 = self.lat.parse().context("latitude Nominatim")?;
        let lon: f64 = self.lon.parse().context("longitude Nominatim")?;
        let extratags = self.extratags.unwrap_or_default();
        let osm = match (
            self.osm_type.as_deref().and_then(OsmType::parse),
            self.osm_id,
        ) {
            (Some(osm_type), Some(id)) if id > 0 => Some(OsmRef { osm_type, id }),
            _ => None,
        };
        let details = OsmDetails::from_tags(|k| extratags.get(k).map(String::as_str));
        Ok(Place {
            name: self.name,
            display_name: self.display_name,
            lat,
            lon,
            category: Category::from_osm(&self.category, &self.kind),
            osm_tag: format!("{}={}", self.category, self.kind),
            osm,
            opening_hours: details.opening_hours,
            website: details.website,
            phone: details.phone,
            email: details.email,
            wikidata: details.wikidata,
            wikipedia: details.wikipedia,
            country_code: self
                .address
                .get("country_code")
                .filter(|c| c.len() == 2)
                .map(|c| c.to_uppercase()),
        })
    }
}

fn parse_search(body: &str) -> anyhow::Result<Vec<Place>> {
    let items: Vec<Item> = serde_json::from_str(body).context("réponse Nominatim illisible")?;
    items.into_iter().map(Item::into_place).collect()
}

fn parse_reverse(body: &str) -> anyhow::Result<Option<Place>> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("réponse Nominatim illisible")?;
    // Rien à cet endroit (ex. en mer) : `{"error": "Unable to geocode"}`.
    if value.get("error").is_some() {
        return Ok(None);
    }
    let item: Item = serde_json::from_value(value).context("réponse Nominatim illisible")?;
    item.into_place().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    // D'après une réponse réelle (Musée national, San José), tags de contact complétés.
    const SEARCH: &str = r#"[{"place_id":1,"osm_type":"relation","osm_id":4144800,
        "lat":"9.9327","lon":"-84.0716","category":"tourism","type":"museum",
        "name":"Museo Nacional de Costa Rica",
        "display_name":"Museo Nacional de Costa Rica, San José, Costa Rica",
        "address":{"city":"San José","country":"Costa Rica","country_code":"cr"},
        "extratags":{"phone":"+506 2257 1433;+506 2257 1434","opening_hours":"Tu-Sa 08:30-16:30; Su 09:00-16:30",
                     "website":"https://www.museocostarica.go.cr","contact:email":"info@museocostarica.go.cr",
                     "wikidata":"Q2638513","wikipedia":"es:Museo Nacional de Costa Rica"}},
        {"place_id":2,"osm_type":"node","osm_id":276216895,"lat":"10.4620845","lon":"-84.7034096",
         "category":"natural","type":"volcano","name":"Volcán Arenal","display_name":"Volcán Arenal",
         "address":{"country_code":"cr"},"extratags":null}]"#;

    #[test]
    fn parses_search_results() {
        let places = parse_search(SEARCH).unwrap();
        assert_eq!(places.len(), 2);
        let m = &places[0];
        assert_eq!(m.name, "Museo Nacional de Costa Rica");
        assert_eq!(m.category, Category::Museum);
        assert_eq!(m.osm_tag, "tourism=museum");
        assert_eq!(
            m.osm,
            Some(OsmRef {
                osm_type: OsmType::Relation,
                id: 4144800
            })
        );
        assert_eq!(
            m.opening_hours.as_deref(),
            Some("Tu-Sa 08:30-16:30; Su 09:00-16:30")
        );
        assert_eq!(m.country_code.as_deref(), Some("CR"));
        assert_eq!(m.phone.as_deref(), Some("+506 2257 1433"));
        assert_eq!(m.email.as_deref(), Some("info@museocostarica.go.cr"));
        assert_eq!(m.wikidata.as_deref(), Some("Q2638513"));
        assert_eq!(
            m.wikipedia.as_deref(),
            Some("es:Museo Nacional de Costa Rica")
        );
        assert_eq!(places[1].phone, None);
        assert!((m.lat - 9.9327).abs() < 1e-9);
        assert_eq!(places[1].category, Category::Nature);
        assert_eq!(places[1].opening_hours, None);
    }

    #[test]
    fn reverse_without_result_is_none() {
        assert_eq!(
            parse_reverse(r#"{"error":"Unable to geocode"}"#).unwrap(),
            None
        );
        let one = r#"{"osm_type":"way","osm_id":5,"lat":"9.9","lon":"-84.0","category":"amenity",
            "type":"restaurant","name":"Soda Tala","display_name":"Soda Tala, San José",
            "address":{"country_code":"cr"},"extratags":{"website":"javascript:alert(1)"}}"#;
        let p = parse_reverse(one).unwrap().unwrap();
        assert_eq!(p.category, Category::Food);
        // Seuls les liens http(s) sont conservés.
        assert_eq!(p.website, None);
    }

    #[tokio::test(start_paused = true)]
    async fn throttle_spaces_requests_and_rejects_backlog() {
        let n = Nominatim::new("http://127.0.0.1:9", "test").unwrap();
        let start = Instant::now();
        n.throttle().await.unwrap();
        n.throttle().await.unwrap();
        n.throttle().await.unwrap();
        assert!(start.elapsed() >= Duration::from_secs(2));
        // File d'attente trop longue : refus immédiat.
        *n.next_slot.lock().unwrap() = Instant::now() + Duration::from_secs(6);
        assert!(matches!(n.throttle().await, Err(PlaceError::Busy)));
    }
}
