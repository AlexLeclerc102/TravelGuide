//! Import des lieux d'un extrait OpenStreetMap (`.osm.pbf`) dans la table `osm_places`.
//!
//! Base des suggestions, interrogée localement : rapide, sans service externe ni limite de
//! débit (les instances publiques d'Overpass sont trop lentes et instables pour une requête
//! interactive). L'extrait est celui que Valhalla télécharge déjà ; il est réimporté au
//! démarrage quand le fichier change.
//!
//! Trois passes sur le fichier : relations retenues (et leurs chemins membres), chemins
//! retenus (et leurs nœuds), puis nœuds (lieux ponctuels et coordonnées utiles). La position
//! d'un chemin est la moyenne de ses nœuds ; celle d'une relation, le centre de l'emprise de
//! ses membres : assez précis pour estimer un trajet.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use osmpbf::{Element, ElementReader, RelMemberType};
use sqlx::PgPool;
use time::OffsetDateTime;

use crate::places::{Category, OsmDetails, OsmType};

/// Tags retenus, par clé : sites, nature, culture, restauration et hébergements nommés.
const KINDS: &[(&str, &[&str])] = &[
    (
        "tourism",
        &[
            "attraction",
            "viewpoint",
            "museum",
            "gallery",
            "zoo",
            "theme_park",
            "aquarium",
            "hotel",
            "guest_house",
            "hostel",
            "motel",
            "chalet",
            "apartment",
            "camp_site",
        ],
    ),
    ("waterway", &["waterfall"]),
    (
        "natural",
        &["beach", "volcano", "hot_spring", "cave_entrance"],
    ),
    ("leisure", &["nature_reserve", "water_park"]),
    ("boundary", &["national_park", "protected_area"]),
    (
        "historic",
        &["monument", "ruins", "archaeological_site", "castle", "fort"],
    ),
    ("amenity", &["restaurant", "cafe"]),
];

/// Lieu importé.
#[derive(Debug, Clone, PartialEq)]
pub struct OsmPlace {
    pub osm_type: OsmType,
    pub osm_id: i64,
    pub name: String,
    pub category: Category,
    /// Tag principal (ex. `waterway=waterfall`).
    pub osm_tag: String,
    pub lat: f64,
    pub lon: f64,
    pub details: OsmDetails,
}

/// Ce qu'un objet OSM apporte s'il est retenu (sans sa position).
#[derive(Debug, Clone, PartialEq)]
struct Tagged {
    name: String,
    category: Category,
    osm_tag: String,
    details: OsmDetails,
}

/// Objet retenu ? Il doit avoir un nom et un tag de [`KINDS`] (le premier trouvé l'emporte).
fn classify<'a>(tags: impl Iterator<Item = (&'a str, &'a str)>) -> Option<Tagged> {
    let mut map: Option<HashMap<&str, &str>> = None;
    for (k, v) in tags {
        // La plupart des objets n'ont aucun tag utile : on ne construit la table qu'au besoin.
        if map.is_none() && !(k == "name" || KINDS.iter().any(|(key, _)| *key == k)) {
            continue;
        }
        map.get_or_insert_with(HashMap::new).insert(k, v);
    }
    let map = map?;
    let name = map
        .get("name")
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())?;
    let (key, value) = KINDS.iter().find_map(|(key, values)| {
        let v = *map.get(key)?;
        values.contains(&v).then_some((*key, v))
    })?;
    Some(Tagged {
        name: name.chars().take(200).collect(),
        category: Category::from_osm(key, value),
        osm_tag: format!("{key}={value}"),
        details: OsmDetails::from_tags(|k| map.get(k).copied()),
    })
}

fn mean(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    (!points.is_empty()).then(|| {
        let n = points.len() as f64;
        let (lat, lon) = points
            .iter()
            .fold((0.0, 0.0), |(a, b), (lat, lon)| (a + lat, b + lon));
        (lat / n, lon / n)
    })
}

fn bbox_center(points: impl Iterator<Item = (f64, f64)>) -> Option<(f64, f64)> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for (lat, lon) in points {
        let b = bounds.get_or_insert((lat, lon, lat, lon));
        *b = (b.0.min(lat), b.1.min(lon), b.2.max(lat), b.3.max(lon));
    }
    bounds.map(|(a, b, c, d)| ((a + c) / 2.0, (b + d) / 2.0))
}

/// Lit l'extrait et renvoie les lieux retenus, dans un ordre déterministe.
pub fn read_extract(path: &Path) -> anyhow::Result<Vec<OsmPlace>> {
    // 1. Relations retenues et leurs membres.
    let mut relations: Vec<(i64, Tagged, Vec<i64>, Vec<i64>)> = vec![];
    ElementReader::from_path(path)?.for_each(|e| {
        if let Element::Relation(r) = e {
            if let Some(t) = classify(r.tags()) {
                let (mut ways, mut nodes) = (vec![], vec![]);
                for m in r.members() {
                    match m.member_type {
                        RelMemberType::Way => ways.push(m.member_id),
                        RelMemberType::Node => nodes.push(m.member_id),
                        RelMemberType::Relation => {}
                    }
                }
                relations.push((r.id(), t, ways, nodes));
            }
        }
    })?;
    let member_ways: HashSet<i64> = relations.iter().flat_map(|r| r.2.iter().copied()).collect();

    // 2. Chemins retenus, et nœuds des chemins utiles.
    let mut way_nodes: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut ways: Vec<(i64, Tagged)> = vec![];
    ElementReader::from_path(path)?.for_each(|e| {
        if let Element::Way(w) = e {
            let tagged = classify(w.tags());
            if tagged.is_some() || member_ways.contains(&w.id()) {
                way_nodes.insert(w.id(), w.refs().collect());
            }
            if let Some(t) = tagged {
                ways.push((w.id(), t));
            }
        }
    })?;
    let needed: HashSet<i64> = way_nodes
        .values()
        .flatten()
        .copied()
        .chain(relations.iter().flat_map(|r| r.3.iter().copied()))
        .collect();

    // 3. Nœuds : lieux ponctuels et coordonnées des chemins.
    let mut coords: HashMap<i64, (f64, f64)> = HashMap::with_capacity(needed.len());
    let mut places: Vec<OsmPlace> = vec![];
    let mut node = |id: i64, lat: f64, lon: f64, tagged: Option<Tagged>| {
        if needed.contains(&id) {
            coords.insert(id, (lat, lon));
        }
        if let Some(t) = tagged {
            places.push(place(OsmType::Node, id, t, (lat, lon)));
        }
    };
    ElementReader::from_path(path)?.for_each(|e| match e {
        Element::Node(n) => node(n.id(), n.lat(), n.lon(), classify(n.tags())),
        Element::DenseNode(n) => node(n.id(), n.lat(), n.lon(), classify(n.tags())),
        _ => {}
    })?;

    let way_points = |id: i64| -> Vec<(f64, f64)> {
        way_nodes
            .get(&id)
            .map(|refs| refs.iter().filter_map(|r| coords.get(r).copied()).collect())
            .unwrap_or_default()
    };
    for (id, t) in ways {
        if let Some(center) = mean(&way_points(id)) {
            places.push(place(OsmType::Way, id, t, center));
        }
    }
    for (id, t, member_ways, member_nodes) in relations {
        let points = member_ways
            .iter()
            .flat_map(|&w| way_points(w))
            .chain(member_nodes.iter().filter_map(|n| coords.get(n).copied()));
        if let Some(center) = bbox_center(points) {
            places.push(place(OsmType::Relation, id, t, center));
        }
    }
    places.sort_by_key(|p| (p.osm_type.as_str(), p.osm_id));
    Ok(places)
}

fn place(osm_type: OsmType, osm_id: i64, t: Tagged, (lat, lon): (f64, f64)) -> OsmPlace {
    OsmPlace {
        osm_type,
        osm_id,
        name: t.name,
        category: t.category,
        osm_tag: t.osm_tag,
        lat,
        lon,
        details: t.details,
    }
}

/// Remplace le contenu de `osm_places` (une transaction : les suggestions restent servies
/// par l'ancien import pendant l'écriture).
pub async fn store(
    db: &PgPool,
    source: &str,
    file: Option<(i64, OffsetDateTime)>,
    places: &[OsmPlace],
) -> anyhow::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM osm_places")
        .execute(&mut *tx)
        .await?;
    for chunk in places.chunks(2_000) {
        let col = |f: fn(&OsmPlace) -> Option<String>| chunk.iter().map(f).collect::<Vec<_>>();
        sqlx::query(
            "INSERT INTO osm_places (osm_type, osm_id, name, category, osm_tag, location,
                 opening_hours, website, phone, email, wikidata, wikipedia)
             SELECT t, i, n, c, g, ST_SetSRID(ST_MakePoint(lo, la), 4326)::geography,
                    oh, w, p, e, wd, wp
             FROM UNNEST($1::text[], $2::int8[], $3::text[], $4::text[], $5::text[],
                         $6::float8[], $7::float8[], $8::text[], $9::text[], $10::text[],
                         $11::text[], $12::text[], $13::text[])
                  AS u(t, i, n, c, g, lo, la, oh, w, p, e, wd, wp)
             ON CONFLICT DO NOTHING",
        )
        .bind(
            chunk
                .iter()
                .map(|p| p.osm_type.as_str())
                .collect::<Vec<_>>(),
        )
        .bind(chunk.iter().map(|p| p.osm_id).collect::<Vec<_>>())
        .bind(chunk.iter().map(|p| p.name.as_str()).collect::<Vec<_>>())
        .bind(
            chunk
                .iter()
                .map(|p| p.category.as_str())
                .collect::<Vec<_>>(),
        )
        .bind(chunk.iter().map(|p| p.osm_tag.as_str()).collect::<Vec<_>>())
        .bind(chunk.iter().map(|p| p.lon).collect::<Vec<_>>())
        .bind(chunk.iter().map(|p| p.lat).collect::<Vec<_>>())
        .bind(col(|p| p.details.opening_hours.clone()))
        .bind(col(|p| p.details.website.clone()))
        .bind(col(|p| p.details.phone.clone()))
        .bind(col(|p| p.details.email.clone()))
        .bind(col(|p| p.details.wikidata.clone()))
        .bind(col(|p| p.details.wikipedia.clone()))
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO osm_import (id, source, file_size, file_modified, imported_at, place_count)
         VALUES (true, $1, $2, $3, now(), $4)
         ON CONFLICT (id) DO UPDATE SET source = EXCLUDED.source, file_size = EXCLUDED.file_size,
             file_modified = EXCLUDED.file_modified, imported_at = now(),
             place_count = EXCLUDED.place_count",
    )
    .bind(source)
    .bind(file.map(|f| f.0))
    .bind(file.map(|f| f.1))
    .bind(places.len() as i32)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Importe l'extrait s'il a changé depuis le dernier import (taille ou date).
pub async fn import_if_changed(db: &PgPool, path: &Path) -> anyhow::Result<()> {
    let meta = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("extrait OSM introuvable : {}", path.display()))?;
    let size = meta.len() as i64;
    let modified = OffsetDateTime::from(meta.modified()?);
    let previous: Option<(Option<i64>, Option<OffsetDateTime>)> =
        sqlx::query_as("SELECT file_size, file_modified FROM osm_import")
            .fetch_optional(db)
            .await?;
    // Précision de stockage : microseconde.
    let same = |at: OffsetDateTime| (at - modified).abs() < time::Duration::milliseconds(1);
    if let Some((Some(s), Some(at))) = previous {
        if s == size && same(at) {
            tracing::info!(path = %path.display(), "lieux OSM déjà importés");
            return Ok(());
        }
    }
    tracing::info!(path = %path.display(), "import des lieux OSM…");
    let started = std::time::Instant::now();
    let owned: PathBuf = path.to_owned();
    let places = tokio::task::spawn_blocking(move || read_extract(&owned)).await??;
    store(
        db,
        &path.display().to_string(),
        Some((size, modified)),
        &places,
    )
    .await?;
    tracing::info!(
        count = places.len(),
        seconds = started.elapsed().as_secs(),
        "lieux OSM importés"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_named_places_of_interest() {
        let t = classify(
            [
                ("name", "Catarata La Fortuna"),
                ("waterway", "waterfall"),
                ("website", "https://cataratalafortuna.com"),
                ("opening_hours", "Mo-Su 07:00-17:00"),
                ("wikidata", "Q5051585"),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(t.category, Category::Nature);
        assert_eq!(t.osm_tag, "waterway=waterfall");
        assert_eq!(
            t.details.website.as_deref(),
            Some("https://cataratalafortuna.com")
        );
        assert_eq!(t.details.wikidata.as_deref(), Some("Q5051585"));

        let hotel = classify([("tourism", "hotel"), ("name", "Hotel Arenal")].into_iter());
        assert_eq!(hotel.unwrap().category, Category::Lodging);
        // Sans nom, ou sans tag retenu : ignoré.
        assert_eq!(classify([("waterway", "waterfall")].into_iter()), None);
        assert_eq!(
            classify([("name", "Rue"), ("highway", "residential")].into_iter()),
            None
        );
        assert_eq!(
            classify([("name", "Pic"), ("natural", "peak")].into_iter()),
            None
        );
    }

    #[test]
    fn centers() {
        assert_eq!(mean(&[(10.0, -84.0), (10.2, -84.2)]), Some((10.1, -84.1)));
        assert_eq!(mean(&[]), None);
        let c = bbox_center([(10.0, -85.0), (11.0, -84.0), (10.5, -84.9)].into_iter());
        assert_eq!(c, Some((10.5, -84.5)));
    }
}

/// Lecture d'un vrai extrait : `TG_OSM_EXTRACT=… cargo test -- --ignored real_extract`.
#[cfg(test)]
#[test]
#[ignore = "nécessite un extrait OSM"]
fn real_extract() {
    let path = std::env::var("TG_OSM_EXTRACT").expect("TG_OSM_EXTRACT");
    let started = std::time::Instant::now();
    let places = read_extract(Path::new(&path)).unwrap();
    let mut by_tag: HashMap<&str, usize> = HashMap::new();
    for p in &places {
        *by_tag.entry(p.osm_tag.as_str()).or_default() += 1;
    }
    let mut tags: Vec<_> = by_tag.into_iter().collect();
    tags.sort_by_key(|a| std::cmp::Reverse(a.1));
    println!("{} lieux en {:?}", places.len(), started.elapsed());
    println!("{tags:?}");
    println!(
        "wikidata : {}",
        places
            .iter()
            .filter(|p| p.details.wikidata.is_some())
            .count()
    );
    for name in ["Catarata", "Arenal", "Manuel Antonio"] {
        if let Some(p) = places.iter().find(|p| p.name.contains(name)) {
            println!("{p:?}");
        }
    }
}
