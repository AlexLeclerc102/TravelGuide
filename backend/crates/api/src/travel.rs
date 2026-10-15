//! Trajets tels que l'API les expose (calcul de trajets et itinéraire).

use planner::travel::{Leg, LegSource, Mode};
use serde::Serialize;
use utoipa::ToSchema;

use crate::trips::TransportMode;

/// Origine d'une durée de trajet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TravelSource {
    /// Calculée sur le réseau OpenStreetMap (Valhalla).
    Router,
    /// Estimée à vol d'oiseau (moteur indisponible ou aucun chemin trouvé).
    StraightLine,
    /// Transports en commun estimés depuis le temps en voiture (× 1,8 + 15 min).
    TransitEstimate,
}

impl TravelSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Router => "router",
            Self::StraightLine => "straight_line",
            Self::TransitEstimate => "transit_estimate",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "router" => Self::Router,
            "straight_line" => Self::StraightLine,
            "transit_estimate" => Self::TransitEstimate,
            _ => return None,
        })
    }

    pub fn is_estimate(self) -> bool {
        self != Self::Router
    }
}

impl From<LegSource> for TravelSource {
    fn from(s: LegSource) -> Self {
        match s {
            LegSource::Router => Self::Router,
            LegSource::StraightLine => Self::StraightLine,
            LegSource::TransitEstimate => Self::TransitEstimate,
        }
    }
}

/// Trajet avec son moyen de transport.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TravelOption {
    pub mode: TransportMode,
    pub duration_s: u32,
    pub distance_m: u32,
    pub source: TravelSource,
}

impl TravelOption {
    pub fn new(mode: Mode, leg: Leg) -> Self {
        Self {
            mode: mode.into(),
            duration_s: leg.duration_s,
            distance_m: leg.distance_m,
            source: leg.source.into(),
        }
    }
}
