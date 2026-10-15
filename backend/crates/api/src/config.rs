//! Configuration lue depuis les variables d'environnement (aucun secret dans le code).

use std::net::SocketAddr;

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
