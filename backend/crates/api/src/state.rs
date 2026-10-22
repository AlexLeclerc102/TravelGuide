use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Config;
use crate::mail::Mailer;
use crate::photos::PhotoService;
use crate::places::PlaceService;
use crate::rate_limit::RateLimiter;
use crate::realtime::Hub;
use crate::routing::RoutingService;
use crate::wiki::WikiService;

/// État partagé entre tous les handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub mailer: Arc<dyn Mailer>,
    pub rate_limiter: Arc<RateLimiter>,
    pub places: Arc<PlaceService>,
    pub routing: Arc<RoutingService>,
    /// Présentations Wikipédia des lieux.
    pub wiki: Arc<WikiService>,
    /// Canaux temps réel des voyages ouverts.
    pub realtime: Arc<Hub>,
    /// Photos des lieux (stockage, téléchargement des images importées).
    pub photos: Arc<PhotoService>,
}
