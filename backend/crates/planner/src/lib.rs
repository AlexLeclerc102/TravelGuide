//! Algorithme de planification d'itinéraire.
//!
//! Cette crate est volontairement pure (aucune I/O) : elle reçoit des POI,
//! une matrice de temps de trajet et des contraintes, et produit un
//! itinéraire de façon déterministe. Elle est testée unitairement.

pub mod geo;
pub mod itinerary;
pub mod ratings;
pub mod recommend;
pub mod travel;
