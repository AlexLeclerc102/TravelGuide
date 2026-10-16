//! Circuits types : des enchaînements de régions ([`crate::regions`]) adaptés à la durée du
//! voyage, depuis l'aéroport d'arrivée jusqu'à celui de départ (aéroports à code IATA de
//! l'extrait OSM). Calcul seul : appliquer un circuit reprend « Où dormir ? » (une zone
//! d'hébergement par étape) et ajoute les points forts au voyage.

use std::collections::HashMap;

use planner::circuits::{self, CircuitProblem, Stop};
use planner::geo::LatLon;
use planner::travel::Mode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, Duration as Days};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::places::{Category, OsmRef, OsmType, Place};
use crate::regions::{self, Access, RegionStatus};
use crate::routing::RoutingService;
use crate::trips;

/// Circuits proposés au plus (le premier, puis des variantes).
const COUNT: usize = 3;

/// Aéroport de ligne (arrivée ou départ d'un circuit).
#[derive(Debug, Serialize, ToSchema)]
pub struct Airport {
    /// Nom suivi du code IATA, ex. « Aeropuerto Internacional Juan Santamaría (SJO) ».
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub osm: OsmRef,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct CircuitRequest {
    /// Aéroport d'arrivée (voir `GET /trips/{id}/airports`).
    pub start: Option<OsmRef>,
    /// Aéroport de départ.
    pub end: Option<OsmRef>,
}

/// Une étape : prête à être appliquée comme proposition de séjours.
#[derive(Debug, Serialize, ToSchema)]
pub struct CircuitStop {
    /// Localité où loger.
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub osm: Option<OsmRef>,
    #[schema(value_type = String, format = Date)]
    pub first_night: Date,
    #[schema(value_type = String, format = Date)]
    pub last_night: Date,
    pub nights: u32,
    /// Route estimée depuis l'étape précédente (ou l'aéroport d'arrivée).
    pub transfer_min: u32,
    pub themes: Vec<Category>,
    pub highlights: Vec<Place>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Circuit {
    /// Thèmes dominants du circuit.
    pub themes: Vec<Category>,
    pub stops: Vec<CircuitStop>,
    /// Route totale entre étapes, aéroports compris.
    pub drive_min: u32,
    /// Route de la dernière étape à l'aéroport de départ.
    pub return_min: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Circuits {
    pub status: RegionStatus,
    /// Nuits du voyage.
    pub nights: u32,
    /// Le premier : les incontournables ; puis des variantes.
    pub circuits: Vec<Circuit>,
    /// Régions sans route depuis le reste du pays (bateau, avion), à ajouter en expédition.
    pub boat_or_plane: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct AirportRow {
    name: String,
    osm_type: String,
    osm_id: i64,
    lat: f64,
    lon: f64,
}

pub async fn airports(db: &PgPool) -> AppResult<Vec<Airport>> {
    let rows: Vec<AirportRow> = sqlx::query_as(
        "SELECT name, osm_type, osm_id,
                ST_Y(location::geometry) AS lat, ST_X(location::geometry) AS lon
         FROM osm_places WHERE osm_tag = 'aeroway=aerodrome'
         -- Les aéroports internationaux (arrivée dans le pays) d'abord.
         ORDER BY name !~* 'internaci|internati', name, osm_type, osm_id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(Airport {
                osm: OsmRef {
                    osm_type: OsmType::parse(&r.osm_type)?,
                    id: r.osm_id,
                },
                name: r.name,
                lat: r.lat,
                lon: r.lon,
            })
        })
        .collect())
}

async fn airport(db: &PgPool, osm: Option<OsmRef>) -> AppResult<Option<LatLon>> {
    let Some(osm) = osm else { return Ok(None) };
    let at: Option<(f64, f64)> = sqlx::query_as(
        "SELECT ST_Y(location::geometry), ST_X(location::geometry) FROM osm_places
         WHERE osm_tag = 'aeroway=aerodrome' AND osm_type = $1 AND osm_id = $2",
    )
    .bind(osm.osm_type.as_str())
    .bind(osm.id)
    .fetch_optional(db)
    .await?;
    at.map(|(lat, lon)| LatLon::new(lat, lon))
        .map(Some)
        .ok_or_else(|| AppError::BadRequest("aéroport inconnu".into()))
}

/// Thèmes dominants : ceux des régions, pondérés par leur intérêt et leur rang.
fn themes<'a>(regions: impl Iterator<Item = &'a regions::Region>) -> Vec<Category> {
    let mut score: HashMap<Category, f64> = HashMap::new();
    for r in regions {
        for (rank, &c) in r.themes.iter().enumerate() {
            *score.entry(c).or_default() += r.weight / (rank + 1) as f64;
        }
    }
    let mut themes: Vec<(Category, f64)> = score.into_iter().collect();
    themes.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.as_str().cmp(b.0.as_str())));
    themes.into_iter().take(3).map(|(c, _)| c).collect()
}

pub async fn circuits(
    db: &PgPool,
    routing: &RoutingService,
    trip_id: Uuid,
    user_id: Uuid,
    req: CircuitRequest,
) -> AppResult<Circuits> {
    let trip = trips::get_for_user(db, trip_id, user_id).await?;
    let nights = ((trip.end_date - trip.start_date).whole_days() + 1) as usize;
    let (start, end) = (airport(db, req.start).await?, airport(db, req.end).await?);
    let found = regions::regions(db, routing, trip_id, user_id).await?;
    let (road, boat): (Vec<_>, Vec<_>) = found
        .regions
        .into_iter()
        .partition(|r| r.access != Access::BoatOrPlane);
    let mut out = Circuits {
        status: found.status,
        nights: nights as u32,
        circuits: vec![],
        boat_or_plane: boat.into_iter().map(|r| r.name).collect(),
    };
    if road.is_empty() {
        return Ok(out);
    }

    // Nœuds : les régions, puis l'arrivée et le départ.
    let mut points: Vec<LatLon> = road.iter().map(|r| LatLon::new(r.lat, r.lon)).collect();
    let mut node = |at: Option<LatLon>| {
        at.map(|a| {
            points.push(a);
            points.len() - 1
        })
    };
    let (start, end) = (node(start), node(end));
    let times: Vec<Vec<u32>> = routing
        .matrix(Mode::Car, &points)
        .await
        .into_iter()
        .map(|row| row.into_iter().map(|leg| leg.duration_s).collect())
        .collect();
    let stops: Vec<Stop> = road
        .iter()
        .map(|r| Stop {
            weight: r.weight,
            // Au moins les nuits par étape du rythme choisi : arriver après des heures de route
            // et repartir le lendemain ne laisse rien voir.
            nights: (r.nights as usize).max(usize::from(trip.pace.min_nights())),
        })
        .collect();
    let problem = CircuitProblem {
        stops: &stops,
        nights,
        start,
        end,
        times: &times,
    };
    for c in circuits::circuits(&problem, COUNT) {
        let mut night = trip.start_date;
        let mut plan_stops = Vec::with_capacity(c.legs.len());
        for leg in &c.legs {
            let r = &road[leg.stop];
            let last = night + Days::days(leg.nights as i64 - 1);
            plan_stops.push(CircuitStop {
                name: r.name.clone(),
                lat: r.lat,
                lon: r.lon,
                osm: r.osm,
                first_night: night,
                last_night: last,
                nights: leg.nights as u32,
                transfer_min: leg.transfer_s.div_ceil(60),
                themes: r.themes.clone(),
                highlights: r.highlights.clone(),
            });
            night = last + Days::days(1);
        }
        out.circuits.push(Circuit {
            themes: themes(c.legs.iter().map(|l| &road[l.stop])),
            stops: plan_stops,
            drive_min: (c.drive_s / 60) as u32,
            return_min: c.return_s.div_ceil(60),
        });
    }
    Ok(out)
}
