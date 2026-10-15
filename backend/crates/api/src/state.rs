use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Config;
use crate::mail::Mailer;
use crate::rate_limit::RateLimiter;

/// État partagé entre tous les handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub mailer: Arc<dyn Mailer>,
    pub rate_limiter: Arc<RateLimiter>,
}
