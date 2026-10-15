//! Catégories de POI et correspondance déterministe avec les tags OSM.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Monument, point de vue, site remarquable.
    Sight,
    /// Parc, plage, volcan, cascade, réserve.
    Nature,
    Museum,
    /// Restaurant, café, bar.
    Food,
    /// Activité, zoo, parc d'attractions, spectacle.
    Activity,
    /// Hébergement (point de départ des journées).
    Lodging,
    Shopping,
    Other,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Sight,
        Category::Nature,
        Category::Museum,
        Category::Food,
        Category::Activity,
        Category::Lodging,
        Category::Shopping,
        Category::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Sight => "sight",
            Category::Nature => "nature",
            Category::Museum => "museum",
            Category::Food => "food",
            Category::Activity => "activity",
            Category::Lodging => "lodging",
            Category::Shopping => "shopping",
            Category::Other => "other",
        }
    }

    /// Durée de visite proposée par défaut (minutes), modifiable par les membres.
    pub fn default_duration_min(self) -> i32 {
        match self {
            Category::Sight => 60,
            Category::Nature => 150,
            Category::Museum => 90,
            Category::Food => 75,
            Category::Activity => 120,
            Category::Lodging => 0,
            Category::Shopping => 45,
            Category::Other => 60,
        }
    }

    /// Catégorie déduite de la clé et de la valeur OSM principales
    /// (`category` et `type` de Nominatim, ex. `tourism` / `museum`).
    pub fn from_osm(key: &str, value: &str) -> Self {
        match (key, value) {
            ("tourism", "museum" | "gallery") => Category::Museum,
            (
                "tourism",
                "hotel" | "hostel" | "guest_house" | "motel" | "chalet" | "apartment" | "camp_site"
                | "caravan_site" | "alpine_hut" | "wilderness_hut",
            ) => Category::Lodging,
            ("tourism", "zoo" | "theme_park" | "aquarium") => Category::Activity,
            ("tourism", "picnic_site") => Category::Nature,
            ("tourism", "information") => Category::Other,
            ("tourism", _) | ("historic", _) => Category::Sight,
            ("natural", _) | ("waterway", "waterfall") => Category::Nature,
            ("boundary", "national_park" | "protected_area") => Category::Nature,
            ("leisure", "park" | "nature_reserve" | "garden" | "beach_resort") => Category::Nature,
            ("leisure", _) => Category::Activity,
            (
                "amenity",
                "restaurant" | "cafe" | "bar" | "pub" | "fast_food" | "food_court" | "ice_cream"
                | "biergarten",
            ) => Category::Food,
            ("amenity", "theatre" | "cinema" | "arts_centre" | "nightclub") => Category::Activity,
            ("amenity", "place_of_worship") | ("building", "church" | "cathedral") => {
                Category::Sight
            }
            ("amenity", "marketplace") | ("shop", _) => Category::Shopping,
            _ => Category::Other,
        }
    }
}

impl FromStr for Category {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, AppError> {
        Category::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("catégorie inconnue : {s}")))
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osm_tags_map_to_categories() {
        use Category::*;
        let cases = [
            ("tourism", "museum", Museum),
            ("tourism", "hotel", Lodging),
            ("tourism", "viewpoint", Sight),
            ("natural", "volcano", Nature),
            ("waterway", "waterfall", Nature),
            ("boundary", "national_park", Nature),
            ("leisure", "park", Nature),
            ("leisure", "water_park", Activity),
            ("amenity", "restaurant", Food),
            ("amenity", "place_of_worship", Sight),
            ("shop", "souvenir", Shopping),
            ("historic", "ruins", Sight),
            ("place", "town", Other),
            ("highway", "bus_stop", Other),
        ];
        for (k, v, expected) in cases {
            assert_eq!(Category::from_osm(k, v), expected, "{k}={v}");
        }
    }

    #[test]
    fn roundtrip_and_defaults() {
        for c in Category::ALL {
            assert_eq!(c.as_str().parse::<Category>().unwrap(), c);
            assert!((0..=720).contains(&c.default_duration_min()));
        }
        assert_eq!(Category::Lodging.default_duration_min(), 0);
    }
}
