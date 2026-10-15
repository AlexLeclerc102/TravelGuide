//! Configuration lue depuis les variables d'environnement (aucun secret dans le code).

use std::net::SocketAddr;

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    /// Origine publique du frontend (CORS + vérification Origin + liens dans les emails).
    pub public_origin: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            bind_addr: optional("BIND_ADDR", "0.0.0.0:8080")
                .parse()
                .context("BIND_ADDR invalide")?,
            public_origin: optional("PUBLIC_ORIGIN", "http://localhost:5173"),
        })
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("variable d'environnement {key} manquante"))
}

fn optional(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}
