//! Suggestions de lieux à ajouter au voyage, ou d'hébergements autour d'un séjour.
//!
//! Les candidats viennent de `osm_places` (extrait OSM importé), autour des points de départ
//! de chaque jour (hébergements, y compris les zones à préciser ; sinon les lieux du voyage).
//! Le score est calculé par [`planner::recommend`] à partir des goûts du groupe (notes par
//! catégorie), de l'itinéraire (temps libre par jour), de la notoriété Wikipédia, des signaux
//! d'intérêt (guide Wikivoyage, patrimoine mondial, superficie des parcs, consultations) et des
//! suggestions écartées.

use std::collections::{HashMap, HashSet};

use planner::geo::LatLon;
use planner::itinerary::fastest_mode;
use planner::recommend::{self, Candidate, Context, DayAnchors, Reason};
use planner::travel::Mode;
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, Time};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::days;
use crate::error::{AppError, AppResult};
use crate::opening;
use crate::places::{Category, OsmRef, OsmType, Place};
use crate::pois::{self, Poi};
use crate::preferences::{self, Budget, Interest};
use crate::trips;
use crate::wiki::{Signals, WikiService};

/// Nombre de suggestions renvoyées.
const LIMIT: usize = 30;
/// Candidats examinés au plus (les plus proches d'abord).
const MAX_CANDIDATES: i64 = 2_000;
/// Candidats examinés en découverte du pays (les plus notables d'abord).
const MAX_DISCOVERY_CANDIDATES: i64 = 500;
/// Pénalité par lieu de même catégorie déjà retenu, en découverte du pays.
const DISCOVERY_SPREAD: f64 = 1.0;
/// Nombre d'avis du groupe sur une catégorie pour en déduire ses goûts.
const TASTE_MIN_REVIEWS: i64 = 2;
/// Une envie déclarée pèse comme ce nombre d'avis : elle s'efface à mesure que le groupe en
/// donne sur ce genre de lieux (force = F / (F + avis)).
const INTEREST_FADE: f64 = 2.0;

/// Une suggestion : le lieu (prêt à être ajouté) et pourquoi il est proposé.
#[derive(Debug, Serialize, ToSchema)]
pub struct Suggestion {
    pub place: Place,
    pub score: f64,
    /// Jour du voyage le plus proche.
    #[schema(value_type = Option<String>, format = Date)]
    pub date: Option<Date>,
    /// Trajet estimé (vol d'oiseau majoré) depuis le point de départ de ce jour.
    pub travel_min: u32,
    /// Raisons, de la plus à la moins déterminante.
    pub reasons: Vec<SuggestionReason>,
}

#[derive(Debug, PartialEq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuggestionReason {
    /// À `minutes` du point de départ du jour `date`.
    Near {
        #[schema(value_type = Option<String>, format = Date)]
        date: Option<Date>,
        minutes: u32,
    },
    /// Article Wikipédia dans `languages` langues.
    Notable { languages: u32 },
    /// Articles Wikipédia (fr, en, es) consultés `monthly_views` fois par mois.
    Popular { monthly_views: u32 },
    /// Guide Wikivoyage.
    Wikivoyage,
    /// Patrimoine mondial de l'UNESCO.
    WorldHeritage,
    /// Parc national (ou autre espace protégé) de `km2` km².
    ProtectedArea { km2: u32, national_park: bool },
    /// Le groupe note bien cette catégorie (moyenne `average`).
    Taste { category: Category, average: f64 },
    /// Le jour `date` a encore `minutes` libres dans l'itinéraire.
    FreeTime {
        #[schema(value_type = String, format = Date)]
        date: Date,
        minutes: u32,
    },
    /// Répond à une envie déclarée du groupe.
    Interest { interest: Interest },
    /// Hébergement adapté au budget déclaré.
    Budget { budget: Budget },
}

/// Ce à quoi répond un candidat parmi les préférences du voyage.
#[derive(Debug, Clone, Copy)]
enum Want {
    Interest(Interest),
    Budget(Budget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionStatus {
    Ready,
    /// Les lieux OSM ne sont pas (encore) importés.
    NotImported,
    /// Ni hébergement ni lieu dans le voyage : les lieux les plus notables de tout le pays.
    Discovery,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Suggestions {
    pub status: SuggestionStatus,
    pub suggestions: Vec<Suggestion>,
}

/// Ce qui est demandé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    /// Lieux à visiter, éventuellement d'une catégorie. Sans catégorie : ni hébergements ni
    /// restaurants (proches de tout, ils évinceraient les sites ; filtre `food` à part).
    Places(Option<Category>),
    /// Hébergements autour d'un lieu du voyage (typiquement une zone d'hébergement).
    LodgingNear(Uuid),
}

#[derive(sqlx::FromRow)]
struct OsmRow {
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
    /// Notoriété et signaux déjà connus (`wikidata_notability`).
    languages: Option<i32>,
    wikivoyage: Option<bool>,
    world_heritage: Option<bool>,
    area_km2: Option<f64>,
    monthly_views: Option<i32>,
}

/// Espace protégé, dont la superficie dit l'ampleur (celle d'une commune ne dit rien).
fn protected_area(osm_tag: &str) -> bool {
    matches!(
        osm_tag,
        "boundary=national_park" | "boundary=protected_area" | "leisure=nature_reserve"
    )
}

fn location(p: &Poi) -> LatLon {
    LatLon::new(p.lat, p.lon)
}

fn seconds(t: Time) -> u32 {
    u32::from(t.hour()) * 3600 + u32::from(t.minute()) * 60
}

/// Rayon de recherche autour des points de départ, selon le mode le plus rapide.
fn radius_m(mode: Mode, lodging: bool) -> f64 {
    match (mode, lodging) {
        (Mode::Walk, true) => 1_500.0,
        (_, true) => 5_000.0,
        (Mode::Walk, false) => 3_000.0,
        (Mode::Bike, false) => 10_000.0,
        (Mode::Car | Mode::Transit, false) => 40_000.0,
    }
}

fn category_index(c: Category) -> usize {
    Category::ALL.iter().position(|&x| x == c).unwrap_or(0)
}

/// Jour de l'itinéraire : date, plage horaire, fin de la dernière visite, trajet de retour.
type ItineraryDayRow = (Date, Time, Time, Option<i32>, Option<i32>);

/// Temps libre de chaque jour d'après l'itinéraire généré (vide sans itinéraire).
async fn free_time(db: &PgPool, trip_id: Uuid, dates: &[Date]) -> AppResult<HashMap<Date, u32>> {
    let rows: Vec<ItineraryDayRow> = sqlx::query_as(
        "SELECT d.date, d.day_start, d.day_end, max(s.end_s), d.back_duration_s
         FROM itinerary_days d
         LEFT JOIN itinerary_stops s ON s.trip_id = d.trip_id AND s.date = d.date
         WHERE d.trip_id = $1
         GROUP BY d.date, d.day_start, d.day_end, d.back_duration_s",
    )
    .bind(trip_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|r| dates.contains(&r.0))
        .map(|(date, start, end, last_end, back)| {
            let busy_until = last_end.map_or(seconds(start), |e| e.max(0) as u32)
                + back.unwrap_or(0).max(0) as u32;
            (date, seconds(end).saturating_sub(busy_until))
        })
        .collect())
}

pub async fn suggest(
    db: &PgPool,
    wiki: &WikiService,
    trip_id: Uuid,
    user_id: Uuid,
    request: Request,
) -> AppResult<Suggestions> {
    let empty = |status| Suggestions {
        status,
        suggestions: vec![],
    };
    let imported: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM osm_places)")
        .fetch_one(db)
        .await?;
    if !imported {
        return Ok(empty(SuggestionStatus::NotImported));
    }
    let trip = trips::get_for_user(db, trip_id, user_id).await?;
    let days = days::list(db, trip_id).await?;
    let pois = pois::list(db, trip_id, user_id, (trip.start_date, trip.end_date)).await?;
    let by_id: HashMap<Uuid, &Poi> = pois.iter().map(|p| (p.id, p)).collect();
    let place_of = |id: Option<Uuid>| id.and_then(|id| by_id.get(&id)).map(|p| location(p));
    let dates: Vec<Date> = days.iter().map(|d| d.date).collect();

    // Points de départ de chaque jour.
    let lodging = matches!(request, Request::LodgingNear(_));
    let day_anchors: Vec<DayAnchors> = match request {
        Request::LodgingNear(poi_id) => {
            let near = by_id.get(&poi_id).ok_or(AppError::NotFound)?;
            vec![DayAnchors {
                anchors: vec![location(near)],
                mode: fastest_mode(
                    &trip
                        .default_modes
                        .iter()
                        .map(|&m| m.into())
                        .collect::<Vec<_>>(),
                ),
                free_s: None,
            }]
        }
        Request::Places(_) => {
            let visits: Vec<LatLon> = pois
                .iter()
                .filter(|p| p.category != Category::Lodging)
                .map(location)
                .collect();
            let free = free_time(db, trip_id, &dates).await?;
            days.iter()
                .enumerate()
                .map(|(i, day)| {
                    let previous = i.checked_sub(1).and_then(|p| days[p].base_poi_id);
                    let mut anchors: Vec<LatLon> = [place_of(previous), place_of(day.base_poi_id)]
                        .into_iter()
                        .flatten()
                        .collect();
                    if anchors.is_empty() {
                        anchors = visits.clone();
                    }
                    DayAnchors {
                        anchors,
                        mode: fastest_mode(
                            &day.modes.iter().map(|&m| m.into()).collect::<Vec<_>>(),
                        ),
                        free_s: free.get(&day.date).copied(),
                    }
                })
                .collect()
        }
    };
    let all_anchors: Vec<LatLon> = day_anchors.iter().flat_map(|d| d.anchors.clone()).collect();
    // Rien autour de quoi chercher : découverte de tout le pays, par notoriété.
    let discovery = all_anchors.is_empty();
    let widest = day_anchors
        .iter()
        .map(|d| radius_m(d.mode, lodging))
        .fold(0.0, f64::max);

    // Candidats autour des points de départ (ou les plus notables), hors lieux du voyage et
    // suggestions écartées.
    let category_filter: Option<&str> = match request {
        Request::Places(c) => c.map(Category::as_str),
        Request::LodgingNear(_) => Some("lodging"),
    };
    let (area, order) = if discovery {
        (
            "true",
            "coalesce(n.world_heritage, false) DESC, coalesce(n.languages, 0) DESC,
             coalesce(n.monthly_views, 0) DESC, (o.wikidata IS NOT NULL) DESC",
        )
    } else {
        (
            "ST_DWithin(o.location, anchors.g, $3)",
            "ST_Distance(o.location, anchors.g)",
        )
    };
    let rows: Vec<OsmRow> = sqlx::query_as(&format!(
        "WITH anchors AS (
             SELECT ST_Collect(ST_SetSRID(ST_MakePoint(lo, la), 4326))::geography AS g
             FROM UNNEST($1::float8[], $2::float8[]) AS a(lo, la)
         )
         SELECT o.osm_type, o.osm_id, o.name, o.category, o.osm_tag,
                ST_Y(o.location::geometry) AS lat, ST_X(o.location::geometry) AS lon,
                o.opening_hours, o.website, o.phone, o.email, o.wikidata, o.wikipedia,
                n.languages, n.wikivoyage, n.world_heritage, n.area_km2, n.monthly_views
         FROM osm_places o
         CROSS JOIN anchors
         LEFT JOIN wikidata_notability n ON n.qid = o.wikidata
         WHERE {area}
           AND ($4::text IS NULL OR o.category = $4)
           AND ($5 OR o.category <> 'lodging')
           AND ($4::text IS NOT NULL OR o.category <> 'food')
           AND o.osm_tag NOT LIKE 'place=%' AND o.osm_tag <> 'aeroway=aerodrome'
           AND NOT EXISTS (SELECT 1 FROM pois p WHERE p.trip_id = $6
                           AND p.osm_type = o.osm_type AND p.osm_id = o.osm_id)
           AND NOT EXISTS (SELECT 1 FROM trip_dismissed_suggestions d WHERE d.trip_id = $6
                           AND d.osm_type = o.osm_type AND d.osm_id = o.osm_id)
         ORDER BY {order}, o.osm_type, o.osm_id
         LIMIT $7"
    ))
    .bind(all_anchors.iter().map(|a| a.lon).collect::<Vec<_>>())
    .bind(all_anchors.iter().map(|a| a.lat).collect::<Vec<_>>())
    .bind(widest)
    .bind(category_filter)
    .bind(lodging)
    .bind(trip_id)
    .bind(if discovery {
        MAX_DISCOVERY_CANDIDATES
    } else {
        MAX_CANDIDATES
    })
    .fetch_all(db)
    .await?;

    // Notoriété et signaux : stockés, sinon demandés à la volée (lieux pas encore rafraîchis ;
    // sans les consultations, réservées à la tâche de fond).
    let unknown: Vec<String> = rows
        .iter()
        .filter(|r| r.languages.is_none())
        .filter_map(|r| r.wikidata.clone())
        .collect();
    let fetched = wiki.signals(&unknown).await;
    // Pays le plus fréquent parmi les lieux du voyage (jours fériés des horaires).
    let country = {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for c in pois.iter().filter_map(|p| p.country_code.as_deref()) {
            *counts.entry(c).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by_key(|&(c, n)| (n, std::cmp::Reverse(c)))
            .map(|(c, _)| c.to_owned())
    };

    let mut places = Vec::with_capacity(rows.len());
    let mut candidates = Vec::with_capacity(rows.len());
    // Un même élément Wikidata porté par plusieurs objets OSM (parc en `boundary=national_park`
    // et `protected_area`…) : le premier (le plus proche ou le plus notable) seulement.
    let mut seen = HashSet::new();
    // Avis du groupe par catégorie : goûts (assez d'avis), et envies déclarées qui s'effacent
    // à mesure que les avis arrivent.
    let reviews: Vec<(String, f64, i64)> = sqlx::query_as(
        "SELECT p.category, avg(r.rating)::float8, count(*) FROM reviews r
         JOIN pois p ON p.id = r.poi_id WHERE p.trip_id = $1 GROUP BY p.category",
    )
    .bind(trip_id)
    .fetch_all(db)
    .await?;
    let mut reviews_of: HashMap<Category, (f64, i64)> = HashMap::new();
    for (c, avg, n) in reviews {
        reviews_of.insert(c.parse()?, (avg, n));
    }
    // Envie (ou budget) à laquelle répond chaque candidat retenu, pour la raison affichée.
    let mut wants: Vec<Option<Want>> = Vec::with_capacity(rows.len());
    for r in rows {
        if r.wikidata.as_ref().is_some_and(|q| !seen.insert(q.clone())) {
            continue;
        }
        let category: Category = r.category.parse()?;
        let closed_all_days = match opening::parse(r.opening_hours.as_deref(), country.as_deref()) {
            opening::Parsed::Valid(oh) => {
                opening::closed_dates(&oh, trip.start_date, trip.end_date).len() == dates.len()
            }
            _ => false,
        };
        let want = match request {
            Request::LodgingNear(_) => trip
                .budget
                .filter(|b| b.matches(&r.osm_tag))
                .map(Want::Budget),
            Request::Places(_) => {
                preferences::matching(&trip.interests, category, &r.osm_tag).map(Want::Interest)
            }
        };
        let wanted = match want {
            Some(Want::Budget(_)) => 1.0,
            Some(Want::Interest(i)) => {
                let n = reviews_of.get(&i.category()).map_or(0, |&(_, n)| n);
                INTEREST_FADE / (INTEREST_FADE + n as f64)
            }
            None => 0.0,
        };
        wants.push(want);
        let signals = match r.languages {
            Some(languages) => Signals {
                languages: languages.max(0) as u32,
                wikivoyage: r.wikivoyage.unwrap_or(false),
                world_heritage: r.world_heritage.unwrap_or(false),
                area_km2: r.area_km2,
                articles: vec![],
            },
            None => r
                .wikidata
                .as_ref()
                .and_then(|q| fetched.get(q))
                .cloned()
                .unwrap_or_default(),
        };
        let known = [
            r.opening_hours.is_some(),
            r.website.is_some() || r.phone.is_some(),
            r.wikidata.is_some() || r.wikipedia.is_some(),
        ];
        candidates.push(Candidate {
            location: LatLon::new(r.lat, r.lon),
            category: category_index(category),
            name_key: r.name.trim().to_lowercase(),
            duration_s: category.default_duration_min().max(0) as u32 * 60,
            languages: signals.languages,
            completeness: known.iter().filter(|&&k| k).count() as f64 / known.len() as f64,
            closed_all_days,
            wanted,
            wikivoyage: signals.wikivoyage,
            world_heritage: signals.world_heritage,
            protected_area_km2: signals.area_km2.filter(|_| protected_area(&r.osm_tag)),
            monthly_views: r.monthly_views.unwrap_or(0).max(0) as u32,
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
            country_code: country.clone(),
        });
    }

    // Goûts du groupe, saturation et refus, par catégorie.
    let mut ctx = Context {
        days: day_anchors,
        // Sans proximité pour départager, varier les catégories (pas que des parcs nationaux).
        spread_penalty: if discovery { DISCOVERY_SPREAD } else { 0.0 },
        ..Default::default()
    };
    if !lodging {
        for (&c, &(avg, n)) in &reviews_of {
            if n >= TASTE_MIN_REVIEWS {
                ctx.taste.insert(category_index(c), avg);
            }
        }
        for p in &pois {
            *ctx.in_trip.entry(category_index(p.category)).or_default() += 1;
        }
    }
    let dismissed: Vec<String> =
        sqlx::query_scalar("SELECT category FROM trip_dismissed_suggestions WHERE trip_id = $1")
            .bind(trip_id)
            .fetch_all(db)
            .await?;
    for c in dismissed {
        if let Ok(c) = c.parse() {
            *ctx.dismissed.entry(category_index(c)).or_default() += 1;
        }
    }

    let date_of = |day: Option<usize>| {
        if lodging {
            None
        } else {
            day.and_then(|d| dates.get(d).copied())
        }
    };
    let mut places: Vec<Option<Place>> = places.into_iter().map(Some).collect();
    let suggestions = recommend::recommend(&candidates, &ctx, LIMIT)
        .into_iter()
        .map(|s| {
            let (category, national_park) = places[s.index]
                .as_ref()
                .map_or((Category::Other, false), |p| {
                    (p.category, p.osm_tag == "boundary=national_park")
                });
            let reasons = s
                .reasons
                .iter()
                .filter_map(|r| match *r {
                    Reason::Near { day, minutes } => Some(SuggestionReason::Near {
                        date: date_of(Some(day)),
                        minutes,
                    }),
                    Reason::Notable { languages } => Some(SuggestionReason::Notable { languages }),
                    Reason::Popular { monthly_views } => {
                        Some(SuggestionReason::Popular { monthly_views })
                    }
                    Reason::Wikivoyage => Some(SuggestionReason::Wikivoyage),
                    Reason::WorldHeritage => Some(SuggestionReason::WorldHeritage),
                    Reason::LargeArea { km2 } => {
                        Some(SuggestionReason::ProtectedArea { km2, national_park })
                    }
                    Reason::Taste { average } => Some(SuggestionReason::Taste {
                        category,
                        average: (average * 10.0).round() / 10.0,
                    }),
                    Reason::FreeTime { day, minutes } => {
                        date_of(Some(day)).map(|date| SuggestionReason::FreeTime { date, minutes })
                    }
                    Reason::Wanted => match wants[s.index] {
                        Some(Want::Interest(interest)) => {
                            Some(SuggestionReason::Interest { interest })
                        }
                        Some(Want::Budget(budget)) => Some(SuggestionReason::Budget { budget }),
                        None => None,
                    },
                })
                .collect();
            Suggestion {
                place: places[s.index].take().expect("suggestion unique"),
                score: (s.score * 10.0).round() / 10.0,
                date: date_of(s.day),
                travel_min: s.travel_s.div_ceil(60),
                reasons,
            }
        })
        .collect();
    Ok(Suggestions {
        status: if discovery {
            SuggestionStatus::Discovery
        } else {
            SuggestionStatus::Ready
        },
        suggestions,
    })
}

/// Le groupe ne veut pas de ce lieu : il n'est plus proposé (et les lieux de même catégorie
/// un peu moins).
pub async fn dismiss(
    db: &PgPool,
    trip_id: Uuid,
    user_id: Uuid,
    osm: OsmRef,
    category: Category,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO trip_dismissed_suggestions (trip_id, osm_type, osm_id, category, dismissed_by)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
    )
    .bind(trip_id)
    .bind(osm.osm_type.as_str())
    .bind(osm.id)
    .bind(category.as_str())
    .bind(user_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Toutes les suggestions écartées sont de nouveau proposées.
pub async fn reset_dismissals(db: &PgPool, trip_id: Uuid) -> AppResult<u64> {
    Ok(
        sqlx::query("DELETE FROM trip_dismissed_suggestions WHERE trip_id = $1")
            .bind(trip_id)
            .execute(db)
            .await?
            .rows_affected(),
    )
}
