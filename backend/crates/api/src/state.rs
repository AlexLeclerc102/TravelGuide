use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Config;

/// État partagé entre tous les handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
}
