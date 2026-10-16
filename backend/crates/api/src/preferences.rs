//! Envies du groupe, posées à la création du voyage : ce qui l'intéresse, son rythme et son
//! budget d'hébergement. Elles orientent les suggestions et les régions tant que le groupe n'a
//! pas encore donné d'avis (les avis réels prennent peu à peu le relais).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{AppError, AppResult};
use crate::places::Category;

/// Ce qui intéresse le groupe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Interest {
    Nature,
    /// Plages (`natural=beach`, catégorie nature).
    Beach,
    Sight,
    Museum,
    Activity,
    Food,
    Shopping,
}

impl Interest {
    pub const ALL: [Interest; 7] = [
        Interest::Nature,
        Interest::Beach,
        Interest::Sight,
        Interest::Museum,
        Interest::Activity,
        Interest::Food,
        Interest::Shopping,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Interest::Nature => "nature",
            Interest::Beach => "beach",
            Interest::Sight => "sight",
            Interest::Museum => "museum",
            Interest::Activity => "activity",
            Interest::Food => "food",
            Interest::Shopping => "shopping",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.as_str() == s)
    }

    /// Catégorie dont les avis remplacent peu à peu cette envie.
    pub fn category(self) -> Category {
        match self {
            Interest::Nature | Interest::Beach => Category::Nature,
            Interest::Sight => Category::Sight,
            Interest::Museum => Category::Museum,
            Interest::Activity => Category::Activity,
            Interest::Food => Category::Food,
            Interest::Shopping => Category::Shopping,
        }
    }

    /// Le lieu (catégorie, tag OSM principal) y répond-il ?
    pub fn matches(self, category: Category, osm_tag: &str) -> bool {
        match self {
            Interest::Beach => osm_tag == "natural=beach",
            _ => self.category() == category,
        }
    }
}

/// Première envie à laquelle répond un lieu.
pub fn matching(interests: &[Interest], category: Category, osm_tag: &str) -> Option<Interest> {
    // Les plages d'abord : plus précises que « nature ».
    interests
        .iter()
        .copied()
        .filter(|i| i.matches(category, osm_tag))
        .min_by_key(|&i| i != Interest::Beach)
}

/// Rythme du voyage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pace {
    /// Moins de visites par jour, plus de nuits par étape.
    Relaxed,
    #[default]
    Balanced,
    /// Journées chargées, étapes courtes.
    Packed,
}

impl Pace {
    pub fn as_str(self) -> &'static str {
        match self {
            Pace::Relaxed => "relaxed",
            Pace::Balanced => "balanced",
            Pace::Packed => "packed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Pace::Relaxed, Pace::Balanced, Pace::Packed]
            .into_iter()
            .find(|p| p.as_str() == s)
    }

    /// Part de la journée réellement consacrée aux visites.
    pub fn day_factor(self) -> f64 {
        match self {
            Pace::Relaxed => 0.7,
            Pace::Balanced => 1.0,
            Pace::Packed => 1.25,
        }
    }

    /// Nuits minimum par étape proposées par défaut (« Où dormir ? »).
    pub fn min_nights(self) -> u8 {
        match self {
            Pace::Relaxed => 3,
            Pace::Balanced => 2,
            Pace::Packed => 1,
        }
    }
}

/// Budget d'hébergement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Budget {
    /// Auberges, maisons d'hôtes, campings.
    Budget,
    /// Hôtels et appartements.
    Comfort,
}

impl Budget {
    pub fn as_str(self) -> &'static str {
        match self {
            Budget::Budget => "budget",
            Budget::Comfort => "comfort",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Budget::Budget, Budget::Comfort]
            .into_iter()
            .find(|b| b.as_str() == s)
    }

    /// L'hébergement (tag OSM principal) correspond-il à ce budget ?
    pub fn matches(self, osm_tag: &str) -> bool {
        let kind = osm_tag.strip_prefix("tourism=").unwrap_or("");
        match self {
            Budget::Budget => matches!(kind, "hostel" | "guest_house" | "camp_site" | "chalet"),
            Budget::Comfort => matches!(kind, "hotel" | "apartment"),
        }
    }
}

/// Envies stockées en base (valeurs inconnues ignorées).
pub fn parse_interests(stored: &[String]) -> Vec<Interest> {
    stored.iter().filter_map(|s| Interest::parse(s)).collect()
}

/// Envies sans doublon, dans l'ordre de [`Interest::ALL`].
pub fn normalize_interests(interests: &[Interest]) -> Vec<String> {
    Interest::ALL
        .into_iter()
        .filter(|i| interests.contains(i))
        .map(|i| i.as_str().to_owned())
        .collect()
}

pub fn parse_pace(stored: &str) -> AppResult<Pace> {
    Pace::parse(stored)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("rythme inconnu : {stored}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beaches_are_a_kind_of_nature() {
        let both = [Interest::Nature, Interest::Beach];
        assert_eq!(
            matching(&both, Category::Nature, "natural=beach"),
            Some(Interest::Beach)
        );
        assert_eq!(
            matching(&both, Category::Nature, "natural=volcano"),
            Some(Interest::Nature)
        );
        assert_eq!(
            matching(&[Interest::Beach], Category::Nature, "natural=volcano"),
            None
        );
        assert_eq!(Interest::Beach.category(), Category::Nature);
    }

    #[test]
    fn budget_follows_the_kind_of_lodging() {
        assert!(Budget::Budget.matches("tourism=hostel"));
        assert!(!Budget::Budget.matches("tourism=hotel"));
        assert!(Budget::Comfort.matches("tourism=hotel"));
        assert!(!Budget::Comfort.matches("tourism=camp_site"));
    }

    #[test]
    fn interests_are_deduplicated_in_a_stable_order() {
        assert_eq!(
            normalize_interests(&[Interest::Food, Interest::Nature, Interest::Food]),
            vec!["nature", "food"]
        );
        assert_eq!(
            parse_interests(&["beach".into(), "x".into()]),
            vec![Interest::Beach]
        );
    }
}
