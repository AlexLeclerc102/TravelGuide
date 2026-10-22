//! Régions à découvrir dans le pays, pour un groupe qui ne sait pas encore où aller.
//!
//! Les lieux importés dont l'article Wikipédia existe dans plusieurs langues sont regroupés
//! par [`planner::regions`] ; chaque région est nommée d'après la localité où loger
//! ([`regions::best_base`], qui préfère une localité décrite par Wikivoyage) et présentée avec
//! ses points forts, prêts à être ajoutés. Les signaux d'intérêt (patrimoine mondial, guide,
//! consultations) pèsent sur le classement, et la fiche dit lesquels.
//!
//! L'accès est vérifié avec le moteur de routage : une base qu'aucune route ne relie au réseau
//! des autres (île Cocos…) se rejoint en bateau ou en avion, ce que la fiche dit clairement.

use planner::geo::{haversine_m, LatLon};
use planner::recommend::popularity;
use planner::regions::{self, Settlement, Spot, GUIDE_SIZE_FACTOR, RADIUS_M};
use serde::Serialize;
use sqlx::PgPool;
use time::Time;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::AppResult;
use crate::places::{Category, OsmRef, OsmType, Place};
use crate::pois;
use crate::preferences::{self, Interest};
use crate::routing::RoutingService;
use crate::trips;

/// Langues Wikipédia minimales pour compter comme lieu notable.
const MIN_LANGUAGES: i32 = 2;
/// Poids d'un lieu qui répond aux envies du groupe.
const INTEREST_BOOST: f64 = 2.0;
/// Notoriété prêtée à une plage sans article Wikipédia (envie « plages » seulement).
const UNKNOWN_BEACH_LANGUAGES: i32 = 3;
/// Points forts qui font l'intérêt d'une région dans un circuit.
const CIRCUIT_HIGHLIGHTS: usize = 3;
/// Points forts présentés par région.
const HIGHLIGHTS: usize = 5;
/// Rayon de recherche de la localité où loger : plus large que la région, car le centre d'un
/// grand parc (centre de son emprise) peut tomber loin de tout, voire en mer pour un parc marin.
const BASE_SEARCH_M: f64 = 2.0 * RADIUS_M;

/// Comment on rejoint une région.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Road,
    /// Aucune route jusqu'au réseau des autres régions : bateau ou avion (île Cocos…).
    BoatOrPlane,
    /// Pas de moteur de routage, moteur en panne, ou région sans localité où loger.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RegionStatus {
    Ready,
    /// Lieux OSM ou notoriété Wikidata pas encore importés.
    NotImported,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Region {
    /// Localité où loger (ou, à défaut, le lieu le plus notable).
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Objet OSM de la localité.
    pub osm: Option<OsmRef>,
    /// Catégories dominantes, de la plus à la moins représentée.
    pub themes: Vec<Category>,
    /// Nuits conseillées pour en voir les points forts (sans les trajets pour y aller).
    pub nights: u32,
    pub access: Access,
    /// Lieux notables de la région.
    pub place_count: usize,
    /// Les plus notables, prêts à être ajoutés au voyage.
    pub highlights: Vec<Place>,
    /// Lieux du voyage (hors hébergements) déjà dans la région.
    pub in_trip: usize,
    /// Lieux de la région inscrits au patrimoine mondial de l'UNESCO.
    pub world_heritage: Vec<String>,
    /// La localité où loger a un guide Wikivoyage.
    pub wikivoyage: bool,
    /// Références Wikipédia de la localité où loger (ou du lieu le plus notable), pour sa
    /// présentation.
    pub wikidata: Option<String>,
    pub wikipedia: Option<String>,
    /// Rayon de la région autour de son centre, en mètres.
    pub radius_m: f64,
    /// Intérêt de la région pour un circuit : ses meilleurs points forts seulement (on ne voit
    /// pas tout d'une capitale en une étape), notoriété pondérée par les envies.
    #[serde(skip)]
    pub weight: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Regions {
    pub status: RegionStatus,
    pub regions: Vec<Region>,
}

#[derive(sqlx::FromRow)]
struct SpotRow {
    osm_type: String,
    osm_id: i64,
    name: String,
    category: String,
    osm_tag: String,
    lat: f64,
    lon: f64,
    opening_hours: Option<String>,
    website: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    wikidata: Option<String>,
    wikipedia: Option<String>,
    languages: i32,
    wikivoyage: bool,
    world_heritage: bool,
    monthly_views: i32,
}

#[derive(sqlx::FromRow)]
struct SettlementRow {
    name: String,
    osm_type: String,
    osm_id: i64,
    osm_tag: String,
    lat: f64,
    lon: f64,
    /// Hébergements connus à moins de 2 km.
    lodging: i64,
    /// Guide Wikivoyage de la localité.
    wikivoyage: bool,
    wikidata: Option<String>,
    wikipedia: Option<String>,
}

/// Localité où loger pour une région : parmi les villes et villages autour, hors celles
/// des régions précédentes (`used`, objets OSM) : deux régions ne portent pas le même nom.
async fn base(
    db: &PgPool,
    spots: &[Spot],
    region: &regions::Region,
    used: &[(String, i64)],
) -> AppResult<Option<SettlementRow>> {
    let mut rows: Vec<SettlementRow> = sqlx::query_as(
        "WITH here AS (SELECT ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography AS g)
         SELECT s.name, s.osm_type, s.osm_id, s.osm_tag,
                ST_Y(s.location::geometry) AS lat, ST_X(s.location::geometry) AS lon,
                (SELECT count(*) FROM osm_places h
                 WHERE h.category = 'lodging' AND ST_DWithin(h.location, s.location, 2000)) AS lodging,
                coalesce(n.wikivoyage, false) AS wikivoyage, s.wikidata, s.wikipedia
         FROM osm_places s CROSS JOIN here
         LEFT JOIN wikidata_notability n ON n.qid = s.wikidata
         WHERE s.osm_tag IN ('place=city', 'place=town', 'place=village')
           AND ST_DWithin(s.location, here.g, $3)
         ORDER BY s.osm_type, s.osm_id",
    )
    .bind(region.center.lon)
    .bind(region.center.lat)
    .bind(BASE_SEARCH_M)
    .fetch_all(db)
    .await?;
    rows.retain(|r| !used.contains(&(r.osm_type.clone(), r.osm_id)));
    let candidates: Vec<Settlement> = rows
        .iter()
        .map(|r| Settlement {
            location: LatLon::new(r.lat, r.lon),
            lodging: r.lodging.clamp(0, i64::from(u32::MAX)) as u32,
            // Les quartiers d'une ville sont souvent des « villages » : la ville prime. Une
            // localité décrite par Wikivoyage accueille des voyageurs.
            size_factor: match r.osm_tag.as_str() {
                "place=city" => 0.5,
                "place=town" => 0.8,
                _ => 1.0,
            } * if r.wikivoyage { GUIDE_SIZE_FACTOR } else { 1.0 },
        })
        .collect();
    Ok(regions::best_base(spots, region, &candidates).map(|i| rows.swap_remove(i)))
}

/// Nom comparable : minuscules, sans accents.
fn fold(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// Même lieu sous deux objets (volcan et « Parque Nacional Volcán … », doublon OSM) ?
fn same_place(a: &str, b: &str) -> bool {
    let (a, b) = (fold(a), fold(b));
    a.contains(&b) || b.contains(&a)
}

fn seconds(t: Time) -> u32 {
    u32::from(t.hour()) * 3600 + u32::from(t.minute()) * 60
}

pub async fn regions(
    db: &PgPool,
    routing: &RoutingService,
    trip_id: Uuid,
    user_id: Uuid,
) -> AppResult<Regions> {
    let trip = trips::get_for_user(db, trip_id, user_id).await?;
    // Un même élément Wikidata peut être porté par plusieurs objets OSM (parc national en
    // `boundary` et en `leisure`…) : un seul compte.
    let rows: Vec<SpotRow> = sqlx::query_as(
        "SELECT DISTINCT ON (coalesce(o.wikidata, o.osm_type || o.osm_id))
                o.osm_type, o.osm_id, o.name, o.category, o.osm_tag,
                ST_Y(o.location::geometry) AS lat, ST_X(o.location::geometry) AS lon,
                o.opening_hours, o.website, o.phone, o.email, o.wikidata, o.wikipedia,
                coalesce(n.languages, $4) AS languages,
                coalesce(n.wikivoyage, false) AS wikivoyage,
                coalesce(n.world_heritage, false) AS world_heritage,
                coalesce(n.monthly_views, 0) AS monthly_views
         FROM osm_places o LEFT JOIN wikidata_notability n ON n.qid = o.wikidata
         WHERE (n.languages >= $1 OR ($3 AND o.osm_tag = 'natural=beach'))
           AND o.category NOT IN ('lodging', 'food')
           AND o.osm_tag NOT LIKE 'place=%' AND o.osm_tag <> 'aeroway=aerodrome'
           AND NOT EXISTS (SELECT 1 FROM trip_dismissed_suggestions d WHERE d.trip_id = $2
                           AND d.osm_type = o.osm_type AND d.osm_id = o.osm_id)
         ORDER BY coalesce(o.wikidata, o.osm_type || o.osm_id), o.osm_type, o.osm_id",
    )
    .bind(MIN_LANGUAGES)
    .bind(trip_id)
    // Les plages ont rarement un article Wikipédia : pour qui les aime, toutes comptent (un
    // peu), et les côtes qui en sont pleines deviennent des régions.
    .bind(trip.interests.contains(&Interest::Beach))
    .bind(UNKNOWN_BEACH_LANGUAGES)
    .fetch_all(db)
    .await?;
    let ready: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM wikidata_notability)")
        .fetch_one(db)
        .await?;
    if rows.is_empty() || !ready {
        return Ok(Regions {
            status: RegionStatus::NotImported,
            regions: vec![],
        });
    }

    let mut places = Vec::with_capacity(rows.len());
    let mut spots = Vec::with_capacity(rows.len());
    let mut world_heritage = Vec::with_capacity(rows.len());
    for r in rows {
        world_heritage.push(r.world_heritage);
        let category: Category = r.category.parse()?;
        spots.push(Spot {
            location: LatLon::new(r.lat, r.lon),
            languages: r.languages.max(0) as u32,
            boost: if preferences::matching(&trip.interests, category, &r.osm_tag).is_some() {
                INTEREST_BOOST
            } else {
                1.0
            },
            appeal: regions::appeal(
                r.wikivoyage,
                r.world_heritage,
                popularity(r.monthly_views.max(0) as u32),
            ),
            category: category_index(category),
            duration_s: category.default_duration_min().max(0) as u32 * 60,
        });
        places.push(Place {
            display_name: r.name.clone(),
            name: r.name,
            lat: r.lat,
            lon: r.lon,
            category,
            osm_tag: r.osm_tag,
            osm: OsmType::parse(&r.osm_type).map(|osm_type| OsmRef {
                osm_type,
                id: r.osm_id,
            }),
            opening_hours: r.opening_hours,
            website: r.website,
            phone: r.phone,
            email: r.email,
            wikidata: r.wikidata,
            wikipedia: r.wikipedia,
            country_code: None,
        });
    }

    let visits: Vec<LatLon> = pois::list(db, trip_id, user_id, (trip.start_date, trip.end_date))
        .await?
        .iter()
        .filter(|p| p.category != Category::Lodging)
        .map(|p| LatLon::new(p.lat, p.lon))
        .collect();
    // Le rythme du groupe : moins (ou plus) de visites par journée.
    let day_budget_s = (f64::from(seconds(trip.day_end).saturating_sub(seconds(trip.day_start)))
        * trip.pace.day_factor()) as u32;
    let mut out = vec![];
    let mut used = vec![];
    for region in regions::regions(&spots, day_budget_s) {
        let (name, lat, lon, osm, wikivoyage, wikidata, wikipedia) =
            match base(db, &spots, &region, &used).await? {
                Some(s) => {
                    used.push((s.osm_type.clone(), s.osm_id));
                    (
                        s.name,
                        s.lat,
                        s.lon,
                        OsmType::parse(&s.osm_type).map(|osm_type| OsmRef {
                            osm_type,
                            id: s.osm_id,
                        }),
                        s.wikivoyage,
                        s.wikidata,
                        s.wikipedia,
                    )
                }
                None => {
                    let top = &places[region.members[0]];
                    let at = region.center;
                    (
                        top.name.clone(),
                        at.lat,
                        at.lon,
                        None,
                        false,
                        top.wikidata.clone(),
                        top.wikipedia.clone(),
                    )
                }
            };
        // Un même site sous plusieurs objets (parc et volcan…) : cité une fois.
        let mut unesco: Vec<String> = vec![];
        for &i in region.members.iter().filter(|&&i| world_heritage[i]) {
            if !unesco.iter().any(|n| same_place(n, &places[i].name)) {
                unesco.push(places[i].name.clone());
            }
        }
        let mut highlights: Vec<Place> = vec![];
        for &i in &region.members {
            let p = &places[i];
            if !highlights.iter().any(|h| same_place(&h.name, &p.name)) {
                highlights.push(p.clone());
            }
            if highlights.len() == HIGHLIGHTS {
                break;
            }
        }
        out.push(Region {
            name,
            lat,
            lon,
            osm,
            themes: region
                .themes
                .iter()
                .map(|&c| Category::ALL[c])
                .take(3)
                .collect(),
            nights: region.nights,
            // Vérifié une fois toutes les bases connues.
            access: if osm.is_some() {
                Access::Road
            } else {
                Access::Unknown
            },
            place_count: region.members.len(),
            highlights,
            in_trip: visits
                .iter()
                .filter(|&&v| haversine_m(v, region.center) <= RADIUS_M)
                .count(),
            world_heritage: unesco,
            wikivoyage,
            wikidata,
            wikipedia,
            radius_m: RADIUS_M,
            // Ses meilleurs points forts, plus la moitié de tout ce qui répond aux envies du groupe
            // (une côte pleine de plages vaut le détour pour qui les aime).
            weight: region
                .members
                .iter()
                .enumerate()
                .map(|(rank, &i)| {
                    let s = &spots[i];
                    match (rank < CIRCUIT_HIGHLIGHTS, s.boost > 1.0) {
                        (true, _) => s.weight(),
                        (false, true) => s.weight() * 0.5,
                        (false, false) => 0.0,
                    }
                })
                .sum(),
        });
    }
    // Accès : les bases reliées entre elles par la route forment des groupes ; le plus grand
    // est le réseau routier du pays, les autres (îles…) se rejoignent en bateau ou en avion.
    let bases: Vec<usize> = (0..out.len())
        .filter(|&i| out[i].access == Access::Road)
        .collect();
    let points: Vec<LatLon> = bases
        .iter()
        .map(|&i| LatLon::new(out[i].lat, out[i].lon))
        .collect();
    let access = match routing.road_links(&points).await {
        Some(links) if points.len() >= 2 => regions::road_groups(&links)
            .into_iter()
            .map(|main| {
                if main {
                    Access::Road
                } else {
                    Access::BoatOrPlane
                }
            })
            .collect(),
        _ => vec![Access::Unknown; bases.len()],
    };
    for (&i, a) in bases.iter().zip(access) {
        out[i].access = a;
    }
    Ok(Regions {
        status: RegionStatus::Ready,
        regions: out,
    })
}

fn category_index(c: Category) -> usize {
    Category::ALL.iter().position(|&x| x == c).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_volcano_and_its_national_park_are_one_highlight() {
        assert!(same_place(
            "Volcan Turrialba",
            "Parque Nacional Volcán Turrialba"
        ));
        assert!(same_place("Poás", "Parque Nacional Volcán Poás"));
        assert!(!same_place("Volcán Irazú", "Volcán Turrialba"));
    }
}
