use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

/// Adresse IP du client si connue (absente dans les tests sans socket réel).
///
/// Derrière un reverse proxy, il faudra lire `X-Forwarded-For` depuis un proxy de confiance.
pub struct ClientIp(pub Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.ip()),
        ))
    }
}

impl ClientIp {
    /// Clé de limitation de débit ; `unknown` regroupe les requêtes sans IP.
    pub fn key(&self) -> String {
        self.0.map_or_else(|| "unknown".into(), |ip| ip.to_string())
    }
}
