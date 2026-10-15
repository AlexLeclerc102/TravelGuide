//! Synthèse des notes du groupe sur un POI.
//!
//! Partagée entre l'API (affichage) et l'algorithme d'itinéraire (pondération),
//! pour que « désaccord » et « moyenne » aient partout la même définition.

use serde::Serialize;

/// Écart minimal entre la note la plus basse et la plus haute pour signaler un désaccord.
pub const DISAGREEMENT_SPREAD: u8 = 2;

/// Notes valides : de 1 à 5.
pub const RATING_RANGE: std::ops::RangeInclusive<u8> = 1..=5;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RatingSummary {
    pub count: usize,
    /// Moyenne arithmétique, absente sans avis.
    pub average: Option<f64>,
    pub min: Option<u8>,
    pub max: Option<u8>,
    /// Au moins deux avis et un écart `max - min` ≥ [`DISAGREEMENT_SPREAD`].
    pub disagreement: bool,
}

/// Résume des notes (supposées comprises entre 1 et 5).
pub fn summarize(ratings: &[u8]) -> RatingSummary {
    let min = ratings.iter().copied().min();
    let max = ratings.iter().copied().max();
    let average = (!ratings.is_empty())
        .then(|| ratings.iter().map(|&r| f64::from(r)).sum::<f64>() / ratings.len() as f64);
    let disagreement = match (min, max) {
        (Some(lo), Some(hi)) => ratings.len() >= 2 && hi - lo >= DISAGREEMENT_SPREAD,
        _ => false,
    };
    RatingSummary {
        count: ratings.len(),
        average,
        min,
        max,
        disagreement,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_ratings() {
        let s = summarize(&[]);
        assert_eq!(s.count, 0);
        assert_eq!(s.average, None);
        assert!(!s.disagreement);
    }

    #[test]
    fn single_rating_never_disagrees() {
        let s = summarize(&[1]);
        assert_eq!(s.average, Some(1.0));
        assert!(!s.disagreement);
    }

    #[test]
    fn average_and_bounds() {
        let s = summarize(&[4, 5, 4]);
        assert!((s.average.unwrap() - 13.0 / 3.0).abs() < 1e-9);
        assert_eq!((s.min, s.max), (Some(4), Some(5)));
        assert!(!s.disagreement);
    }

    #[test]
    fn spread_of_two_is_a_disagreement() {
        assert!(!summarize(&[3, 4, 4]).disagreement);
        assert!(summarize(&[3, 5]).disagreement);
        assert!(summarize(&[1, 5, 5, 5]).disagreement);
    }
}
