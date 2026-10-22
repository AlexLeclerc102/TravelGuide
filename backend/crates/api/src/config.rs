//! Configuration lue depuis les variables d'environnement (aucun secret dans le code).

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    /// Origine publique du frontend (CORS, vérification `Origin`, liens dans les emails).
    pub public_origin: String,
    /// Origines supplémentaires autorisées à envoyer des requêtes d'écriture (ex. docs de l'API).
    pub extra_allowed_origins: Vec<String>,
    /// Ajoute l'attribut `Secure` au cookie de session (à désactiver uniquement en dev HTTP).
    pub cookie_secure: bool,
    /// URL SMTP (`smtp://host:port` ou `smtps://user:pass@host`). Absente : emails journalisés.
    pub smtp_url: Option<String>,
    pub mail_from: String,
    /// Abonnement simulé : emails recevant le plan `organizer` (`*` = tout le monde).
    pub organizer_emails: Vec<String>,
    /// Instance Nominatim (recherche de lieux).
    pub nominatim_url: String,
    /// Contact (email ou URL) ajouté au User-Agent, exigé par la politique d'usage de Nominatim.
    pub nominatim_contact: Option<String>,
    /// Moteur de routage Valhalla. Absent : trajets estimés à vol d'oiseau.
    pub valhalla_url: Option<String>,
    /// Extrait OpenStreetMap (`.osm.pbf`) dont les lieux nourrissent les suggestions.
    pub osm_extract: Option<PathBuf>,
    /// Assistant IA (OpenRouter). Absent sans `OPENROUTER_API_KEY` : assistant désactivé.
    pub ai: Option<AiConfig>,
}

/// Configuration de l'assistant IA.
#[derive(Debug, Clone)]
pub struct AiConfig {
    pub api_key: Secret,
    pub base_url: String,
    /// Modèle de l'agent principal.
    pub model: String,
    /// Modèle des sous-appels (résumés, extraction de pages, recherche web).
    pub model_small: String,
    /// Modèles sélectionnables par conversation (dev, comparaison), en plus de `model`.
    pub models_allowed: Vec<String>,
    /// Hébergeurs OpenRouter autorisés (`provider.only`), couverts par un accord de
    /// traitement des données. Vide : tous les hébergeurs sans collecte de données.
    pub providers_only: Vec<String>,
    /// Budget quotidien par utilisateur, en dollars.
    pub daily_budget_usd: f64,
    /// Tours d'outils au plus par message de l'utilisateur.
    pub max_rounds: u32,
}

/// Valeur secrète : jamais affichée par `Debug` (donc jamais journalisée).
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl AiConfig {
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(api_key) = std::env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
        else {
            return Ok(None);
        };
        let model = optional("AI_MODEL", "z-ai/glm-5.3-flash");
        let model_small = std::env::var("AI_MODEL_SMALL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| model.clone());
        Ok(Some(Self {
            api_key: Secret::new(api_key.trim()),
            base_url: optional("OPENROUTER_URL", "https://openrouter.ai/api/v1"),
            model,
            model_small,
            models_allowed: list("AI_MODELS_ALLOWED"),
            providers_only: list("AI_PROVIDERS_ONLY"),
            daily_budget_usd: optional("AI_DAILY_BUDGET_USD", "0.50")
                .parse()
                .context("AI_DAILY_BUDGET_USD invalide")?,
            max_rounds: optional("AI_MAX_ROUNDS", "8")
                .parse()
                .context("AI_MAX_ROUNDS invalide")?,
        }))
    }

    /// Le modèle demandé pour une conversation est-il autorisé ?
    pub fn allows_model(&self, model: &str) -> bool {
        model == self.model || self.models_allowed.iter().any(|m| m == model)
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            bind_addr: optional("BIND_ADDR", "0.0.0.0:8080")
                .parse()
                .context("BIND_ADDR invalide")?,
            public_origin: optional("PUBLIC_ORIGIN", "http://localhost:5173"),
            extra_allowed_origins: optional("EXTRA_ALLOWED_ORIGINS", "")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            cookie_secure: optional("COOKIE_SECURE", "true")
                .parse()
                .context("COOKIE_SECURE doit valoir true ou false")?,
            smtp_url: std::env::var("SMTP_URL").ok().filter(|s| !s.is_empty()),
            mail_from: optional("MAIL_FROM", "TravelGuide <no-reply@travelguide.local>"),
            organizer_emails: optional("ORGANIZER_EMAILS", "")
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect(),
            nominatim_url: optional("NOMINATIM_URL", "https://nominatim.openstreetmap.org"),
            nominatim_contact: std::env::var("NOMINATIM_CONTACT")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            valhalla_url: std::env::var("VALHALLA_URL")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            osm_extract: std::env::var("OSM_EXTRACT")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from),
            ai: AiConfig::from_env()?,
        })
    }

    /// Configuration minimale pour les tests.
    pub fn for_tests() -> Self {
        Self {
            database_url: String::new(),
            bind_addr: "127.0.0.1:0".parse().expect("adresse valide"),
            public_origin: "http://localhost:5173".into(),
            extra_allowed_origins: vec![],
            cookie_secure: false,
            smtp_url: None,
            mail_from: "TravelGuide <no-reply@travelguide.local>".into(),
            organizer_emails: vec![],
            nominatim_url: String::new(),
            nominatim_contact: None,
            valhalla_url: None,
            osm_extract: None,
            ai: None,
        }
    }

    /// L'abonnement (simulé) organisateur est-il accordé à cet email ?
    pub fn grants_organizer(&self, email: &str) -> bool {
        self.organizer_emails
            .iter()
            .any(|e| e == "*" || e.eq_ignore_ascii_case(email))
    }

    /// User-Agent identifiant l'application auprès des services OpenStreetMap.
    pub fn user_agent(&self) -> String {
        let version = env!("CARGO_PKG_VERSION");
        match &self.nominatim_contact {
            Some(contact) => format!("TravelGuide/{version} ({contact})"),
            None => format!("TravelGuide/{version} (+{})", self.public_origin),
        }
    }

    pub fn is_allowed_origin(&self, origin: &str) -> bool {
        origin == self.public_origin || self.extra_allowed_origins.iter().any(|o| o == origin)
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("variable d'environnement {key} manquante"))
}

fn optional(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

/// Liste séparée par des virgules (vide si la variable est absente).
fn list(key: &str) -> Vec<String> {
    optional(key, "")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
