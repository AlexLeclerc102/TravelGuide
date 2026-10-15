//! Limitation de débit en mémoire, par clé (fenêtre fixe).
//!
//! Suffisant pour une instance unique ; à remplacer par un stockage partagé
//! si l'API est déployée sur plusieurs instances.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::AppError;

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    /// Compte une tentative pour `key` ; erreur 429 au-delà de `max` dans `window`.
    pub fn check(&self, key: &str, max: u32, window: Duration) -> Result<(), AppError> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("mutex empoisonné");
        if buckets.len() > 10_000 {
            buckets.retain(|_, (start, _)| now.duration_since(*start) < window);
        }
        let entry = buckets.entry(key.to_owned()).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > max {
            Err(AppError::TooManyRequests)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_max_attempts() {
        let rl = RateLimiter::default();
        let w = Duration::from_secs(60);
        for _ in 0..3 {
            assert!(rl.check("k", 3, w).is_ok());
        }
        assert!(rl.check("k", 3, w).is_err());
        assert!(rl.check("other", 3, w).is_ok());
    }

    #[test]
    fn window_resets() {
        let rl = RateLimiter::default();
        let w = Duration::from_millis(10);
        assert!(rl.check("k", 1, w).is_ok());
        assert!(rl.check("k", 1, w).is_err());
        std::thread::sleep(Duration::from_millis(15));
        assert!(rl.check("k", 1, w).is_ok());
    }
}
