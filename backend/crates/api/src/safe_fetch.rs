//! Téléchargement d'adresses fournies par les utilisateurs ou par l'assistant (photos,
//! pages web), protégé contre la falsification de requêtes côté serveur (SSRF).
//!
//! Seules les adresses https vers des IP publiques sont suivies : l'hôte est résolu une
//! fois, chaque adresse obtenue est vérifiée, et la connexion se fait vers l'IP vérifiée
//! (une seconde réponse DNS ne peut pas la rediriger vers le réseau interne). Les
//! redirections sont suivies à la main et revérifiées à chaque étape.
//!
//! Le trait [`WebFetcher`] permet aux tests d'utiliser [`StaticWeb`], sans réseau.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{StatusCode, Url};

/// Redirections suivies au plus.
const MAX_REDIRECTS: usize = 3;
/// Durée maximale d'un téléchargement, redirections comprises.
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// Adresse refusée (pas https, IP non publique…) : message affichable.
    #[error("{0}")]
    Refused(String),
    #[error("fichier trop volumineux (plus de {0} Mo)")]
    TooLarge(usize),
    #[error("la page a répondu {0}")]
    Status(StatusCode),
    #[error("téléchargement impossible")]
    Unavailable(#[source] anyhow::Error),
}

/// Contenu téléchargé.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// Adresse finale, après redirections.
    pub url: String,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

#[async_trait]
pub trait WebFetcher: Send + Sync {
    /// Télécharge `url` (au plus `max_bytes` octets).
    async fn get(&self, url: &str, max_bytes: usize) -> Result<Fetched, FetchError>;
}

/// Client réel, limité aux adresses https publiques.
pub struct SafeFetcher {
    user_agent: String,
}

impl SafeFetcher {
    pub fn new(user_agent: &str) -> Self {
        Self {
            user_agent: user_agent.to_owned(),
        }
    }

    /// Une étape : résolution, vérification, requête vers l'IP vérifiée, sans redirection.
    async fn hop(&self, url: &Url) -> Result<reqwest::Response, FetchError> {
        let host = url
            .host_str()
            .ok_or_else(|| FetchError::Refused("adresse sans hôte".into()))?;
        let addr = resolve_public(url).await?;
        let client = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(TIMEOUT)
            // L'hôte est contacté à l'IP vérifiée ; le nom reste utilisé pour TLS (SNI).
            .resolve(host, addr)
            .no_proxy()
            .build()
            .map_err(|e| FetchError::Unavailable(e.into()))?;
        client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| FetchError::Unavailable(e.into()))
    }
}

#[async_trait]
impl WebFetcher for SafeFetcher {
    async fn get(&self, url: &str, max_bytes: usize) -> Result<Fetched, FetchError> {
        let work = async {
            let mut url = check_url(url)?;
            for _ in 0..=MAX_REDIRECTS {
                let res = self.hop(&url).await?;
                let status = res.status();
                if status.is_redirection() {
                    let location = res
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                        .ok_or(FetchError::Status(status))?;
                    url = next_hop(&url, location)?;
                    continue;
                }
                if !status.is_success() {
                    return Err(FetchError::Status(status));
                }
                return read_body(res, max_bytes).await;
            }
            Err(FetchError::Refused("trop de redirections".into()))
        };
        tokio::time::timeout(TIMEOUT, work)
            .await
            .map_err(|_| FetchError::Unavailable(anyhow::anyhow!("délai dépassé")))?
    }
}

async fn read_body(res: reqwest::Response, max_bytes: usize) -> Result<Fetched, FetchError> {
    let too_large = || FetchError::TooLarge(max_bytes / 1_000_000);
    if res.content_length().is_some_and(|n| n > max_bytes as u64) {
        return Err(too_large());
    }
    let url = res.url().to_string();
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut body = Vec::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| FetchError::Unavailable(e.into()))?;
        // La taille annoncée peut mentir : on compte ce qui arrive vraiment.
        if body.len() + chunk.len() > max_bytes {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Fetched {
        url,
        content_type,
        body,
    })
}

/// Adresse acceptable : https, port par défaut, sans identifiants.
pub fn check_url(raw: &str) -> Result<Url, FetchError> {
    let url = Url::parse(raw.trim()).map_err(|_| FetchError::Refused("adresse invalide".into()))?;
    if url.scheme() != "https" {
        return Err(FetchError::Refused(
            "seules les adresses https sont acceptées".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FetchError::Refused(
            "adresse avec identifiants refusée".into(),
        ));
    }
    if url.port().is_some_and(|p| p != 443) {
        return Err(FetchError::Refused("port non standard refusé".into()));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(FetchError::Refused("adresse sans hôte".into()));
    }
    Ok(url)
}

/// Cible d'une redirection, vérifiée comme une adresse de départ.
fn next_hop(from: &Url, location: &str) -> Result<Url, FetchError> {
    let next = from
        .join(location)
        .map_err(|_| FetchError::Refused("redirection invalide".into()))?;
    check_url(next.as_str())
}

/// Résout l'hôte et renvoie une adresse publique ; refuse si une seule ne l'est pas
/// (un hôte qui mêle IP publiques et internes est suspect).
async fn resolve_public(url: &Url) -> Result<SocketAddr, FetchError> {
    let host = url
        .host_str()
        .ok_or_else(|| FetchError::Refused("adresse sans hôte".into()))?;
    // `[::1]` : crochets des IPv6 littérales.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<SocketAddr> = match bare.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, 443)],
        Err(_) => tokio::net::lookup_host((bare, 443))
            .await
            .map_err(|e| FetchError::Unavailable(e.into()))?
            .collect(),
    };
    if addrs.is_empty() {
        return Err(FetchError::Unavailable(anyhow::anyhow!("hôte introuvable")));
    }
    if addrs.iter().any(|a| !is_public(a.ip())) {
        return Err(FetchError::Refused(
            "adresse interne ou réservée refusée".into(),
        ));
    }
    Ok(addrs[0])
}

/// IP joignable sur Internet (ni privée, ni locale, ni réservée).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        // Espace partagé des opérateurs (CGNAT) 100.64.0.0/10.
        || (a == 100 && (64..128).contains(&b))
        // Protocoles IETF 192.0.0.0/24.
        || (a == 192 && b == 0 && c == 0)
        // Tests de performance 198.18.0.0/15.
        || (a == 198 && (b == 18 || b == 19))
        // Réservé 240.0.0.0/4.
        || a >= 240)
}

/// Seules les adresses unicast globales (2000::/3) sont acceptées, hors documentation et
/// mécanismes qui embarquent une IPv4 (6to4, NAT64, Teredo).
fn is_public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let global = s[0] & 0xe000 == 0x2000;
    let documentation = s[0] == 0x2001 && s[1] == 0x0db8;
    let teredo = s[0] == 0x2001 && s[1] == 0;
    let six_to_four = s[0] == 0x2002;
    let nat64 = s[0] == 0x0064 && s[1] == 0xff9b;
    global && !documentation && !teredo && !six_to_four && !nat64
}

/// Faux web pour les tests : adresses connues → `(type, contenu)`, compteur d'appels.
#[derive(Default)]
pub struct StaticWeb {
    pages: HashMap<String, (String, Vec<u8>)>,
    calls: AtomicUsize,
}

impl StaticWeb {
    pub fn new(pages: impl IntoIterator<Item = (String, String, Vec<u8>)>) -> Self {
        Self {
            pages: pages.into_iter().map(|(u, t, b)| (u, (t, b))).collect(),
            calls: AtomicUsize::new(0),
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl WebFetcher for std::sync::Arc<StaticWeb> {
    async fn get(&self, url: &str, max_bytes: usize) -> Result<Fetched, FetchError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // Mêmes règles d'adresse que le client réel.
        let url = check_url(url)?;
        let (content_type, body) = self
            .pages
            .get(url.as_str())
            .ok_or(FetchError::Status(StatusCode::NOT_FOUND))?;
        if body.len() > max_bytes {
            return Err(FetchError::TooLarge(max_bytes / 1_000_000));
        }
        Ok(Fetched {
            url: url.to_string(),
            content_type: Some(content_type.clone()),
            body: body.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn only_public_ips_are_allowed() {
        for public in [
            "8.8.8.8",
            "151.101.1.69",
            "2a00:1450:4007:80e::200e",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_public(ip(public)), "{public}");
        }
        for internal in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "169.254.169.254", // métadonnées des clouds
            "100.64.0.1",
            "0.0.0.0",
            "0.1.2.3",
            "192.0.0.8",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "192.0.2.1",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::a9fe:a9fe",
            "2002:a9fe:a9fe::1",
            "2001:0:4136:e378::1",
        ] {
            assert!(!is_public(ip(internal)), "{internal}");
        }
    }

    #[test]
    fn urls_must_be_plain_https() {
        assert!(check_url("https://upload.wikimedia.org/a.jpg").is_ok());
        assert!(check_url("https://example.org:443/a.jpg").is_ok());
        for bad in [
            "http://example.org/a.jpg",
            "ftp://example.org/a.jpg",
            "file:///etc/passwd",
            "https://user:pw@example.org/",
            "https://example.org:8443/",
            "https://",
            "pas une adresse",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn redirects_are_rechecked() {
        let from = Url::parse("https://example.org/photos/a").unwrap();
        assert_eq!(
            next_hop(&from, "/b.jpg").unwrap().as_str(),
            "https://example.org/b.jpg"
        );
        assert!(next_hop(&from, "http://example.org/b.jpg").is_err());
        assert!(next_hop(&from, "https://example.org:22/").is_err());
    }

    #[tokio::test]
    async fn internal_hosts_are_refused_before_any_connection() {
        let fetcher = SafeFetcher::new("test");
        for url in [
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://169.254.169.254/latest/meta-data/",
            "https://localhost/",
            "https://10.1.2.3/x.jpg",
        ] {
            match fetcher.get(url, 1_000).await {
                Err(FetchError::Refused(_)) => {}
                other => panic!("{url} : {other:?}"),
            }
        }
    }
}
