//! Présentation d'un lieu tirée de Wikipédia, avec sa photo Wikimedia Commons.
//!
//! À partir des tags OSM `wikidata` / `wikipedia` d'un lieu : l'élément Wikidata donne les
//! articles dans chaque langue et la photo (propriété P18) ; l'API REST de Wikipédia donne le
//! résumé ; Commons donne la miniature, l'auteur et la licence (attribution obligatoire).
//! Résultats en cache ; le fournisseur réel est [`Wikimedia`], les tests utilisent [`StaticWiki`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::AppError;

/// Langues des articles, par préférence (interface en français, puis langues les plus
/// fournies pour le Costa Rica).
const LANGS: [&str; 3] = ["fr", "en", "es"];

/// Présentation d'un lieu.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct WikiSummary {
    pub title: String,
    /// Description courte (ex. « musée national situé à San José »).
    pub description: Option<String>,
    /// Premier paragraphe de l'article, en texte brut.
    pub extract: String,
    /// Langue de l'article (`fr`, `en`, `es`…).
    pub lang: String,
    /// Article complet sur Wikipédia.
    pub url: String,
    pub image: Option<WikiImage>,
    /// Guide Wikivoyage du lieu, s'il existe.
    pub wikivoyage_url: Option<String>,
}

/// Photo Wikimedia Commons, avec de quoi la créditer.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct WikiImage {
    /// Miniature (environ 640 px de large).
    pub url: String,
    pub width: u32,
    pub height: u32,
    /// Page de la photo sur Commons (auteur, licence, original).
    pub page_url: String,
    pub artist: Option<String>,
    pub license: Option<String>,
}

/// Élément Wikidata : titres des articles par site (`frwiki`, `enwikivoyage`…) et photo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entity {
    pub sitelinks: HashMap<String, String>,
    /// Nom du fichier Commons (sans le préfixe `File:`).
    pub image_file: Option<String>,
}

/// Résumé d'un article Wikipédia.
#[derive(Debug, Clone, PartialEq)]
pub struct Article {
    pub title: String,
    pub description: Option<String>,
    pub extract: String,
    pub lang: String,
    pub url: String,
    /// Élément Wikidata de l'article.
    pub wikibase_item: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum WikiError {
    #[error("Wikipédia est momentanément indisponible")]
    Unavailable(#[source] anyhow::Error),
}

impl From<WikiError> for AppError {
    fn from(e: WikiError) -> Self {
        let WikiError::Unavailable(ref source) = e;
        tracing::warn!(error = ?source, "Wikipédia / Wikidata en échec");
        AppError::ServiceUnavailable(e.to_string())
    }
}

#[async_trait]
pub trait WikiProvider: Send + Sync {
    /// Élément Wikidata (`None` s'il n'existe pas).
    async fn entity(&self, qid: &str) -> Result<Option<Entity>, WikiError>;
    /// Résumé d'un article (`None` s'il n'existe pas ou n'est qu'une page d'homonymie).
    async fn article(&self, lang: &str, title: &str) -> Result<Option<Article>, WikiError>;
    /// Miniature et crédits d'une photo Commons.
    async fn image(&self, file: &str) -> Result<Option<WikiImage>, WikiError>;
    /// Nombre de langues de l'article Wikipédia de chaque élément (au plus 50 à la fois ;
    /// un élément absent de la réponse n'existe pas).
    async fn languages(&self, qids: &[String]) -> Result<HashMap<String, u32>, WikiError>;
}

/// Projets Wikimedia dont le code de site finit par `wiki` sans être une Wikipédia.
const NOT_WIKIPEDIA: [&str; 8] = [
    "commonswiki",
    "specieswiki",
    "metawiki",
    "mediawikiwiki",
    "wikidatawiki",
    "sourceswiki",
    "incubatorwiki",
    "wikimaniawiki",
];

/// Langues de Wikipédia parmi les codes de site d'un élément (`frwiki`, `enwiki`…).
fn count_wikipedias<'a>(sites: impl Iterator<Item = &'a str>) -> u32 {
    sites
        .filter(|s| s.ends_with("wiki") && !NOT_WIKIPEDIA.contains(s))
        .count() as u32
}

const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const CACHE_MAX: usize = 2_000;

/// Fournisseur + cache en mémoire (une instance ; TTL 7 jours, absences comprises).
pub struct WikiService {
    provider: Box<dyn WikiProvider>,
    cache: Mutex<HashMap<String, (Instant, Option<WikiSummary>)>>,
    languages: Mutex<HashMap<String, (Instant, u32)>>,
}

impl WikiService {
    pub fn new(provider: impl WikiProvider + 'static) -> Self {
        Self {
            provider: Box::new(provider),
            cache: Mutex::new(HashMap::new()),
            languages: Mutex::new(HashMap::new()),
        }
    }

    /// Nombre de langues Wikipédia de chaque élément (0 s'il n'existe pas). En cache 7 jours ;
    /// Wikidata ne répondant pas, les éléments non connus comptent pour 0 (rien n'échoue).
    pub async fn languages(&self, qids: &[String]) -> HashMap<String, u32> {
        let mut result = HashMap::with_capacity(qids.len());
        let mut missing = vec![];
        {
            let cache = self.languages.lock().expect("mutex empoisonné");
            for q in qids {
                match cache.get(q).filter(|(at, _)| at.elapsed() < CACHE_TTL) {
                    Some(&(_, n)) => {
                        result.insert(q.clone(), n);
                    }
                    None => missing.push(q.clone()),
                }
            }
        }
        missing.sort();
        missing.dedup();
        for chunk in missing.chunks(50) {
            match self.provider.languages(chunk).await {
                Ok(found) => {
                    let mut cache = self.languages.lock().expect("mutex empoisonné");
                    if cache.len() >= CACHE_MAX * 10 {
                        cache.clear();
                    }
                    for q in chunk {
                        let n = found.get(q).copied().unwrap_or(0);
                        cache.insert(q.clone(), (Instant::now(), n));
                        result.insert(q.clone(), n);
                    }
                }
                Err(WikiError::Unavailable(e)) => {
                    tracing::warn!(error = ?e, "notoriété Wikidata indisponible");
                    break;
                }
            }
        }
        result
    }

    /// Présentation d'un lieu d'après ses tags (`wikipedia` au format OSM `langue:Titre`).
    pub async fn summary(
        &self,
        wikidata: Option<&str>,
        wikipedia: Option<&str>,
    ) -> Result<Option<WikiSummary>, WikiError> {
        let key = format!("{}|{}", wikidata.unwrap_or(""), wikipedia.unwrap_or(""));
        if let Some(hit) = self.cached(&key) {
            return Ok(hit);
        }
        let summary = self.fetch(wikidata, wikipedia).await?;
        self.remember(key, summary.clone());
        Ok(summary)
    }

    fn cached(&self, key: &str) -> Option<Option<WikiSummary>> {
        let cache = self.cache.lock().expect("mutex empoisonné");
        cache
            .get(key)
            .filter(|(at, _)| at.elapsed() < CACHE_TTL)
            .map(|(_, v)| v.clone())
    }

    fn remember(&self, key: String, value: Option<WikiSummary>) {
        let mut cache = self.cache.lock().expect("mutex empoisonné");
        if cache.len() >= CACHE_MAX {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
            if cache.len() >= CACHE_MAX {
                cache.clear();
            }
        }
        cache.insert(key, (Instant::now(), value));
    }

    async fn fetch(
        &self,
        wikidata: Option<&str>,
        wikipedia: Option<&str>,
    ) -> Result<Option<WikiSummary>, WikiError> {
        let tag = wikipedia.and_then(|t| t.split_once(':'));
        // Sans Wikidata, l'article du tag donne l'élément (et sert de repli).
        let mut tagged = None;
        let qid = match wikidata {
            Some(q) => Some(q.to_owned()),
            None => match tag {
                Some((lang, title)) => {
                    tagged = self.provider.article(lang, title).await?;
                    tagged.as_ref().and_then(|a| a.wikibase_item.clone())
                }
                None => None,
            },
        };
        let entity = match &qid {
            Some(q) => self.provider.entity(q).await?.unwrap_or_default(),
            None => Entity::default(),
        };

        // Article dans la langue préférée, sinon celui du tag ; photo en parallèle.
        let mut candidates: Vec<(&str, &str)> = LANGS
            .iter()
            .filter_map(|&lang| {
                Some((lang, entity.sitelinks.get(&format!("{lang}wiki"))?.as_str()))
            })
            .collect();
        candidates.extend(tag);
        let pick = async {
            for (lang, title) in candidates {
                if let Some(a) = tagged
                    .as_ref()
                    .filter(|a| a.lang == lang && a.title == title)
                {
                    return Ok(Some(a.clone()));
                }
                if let Some(a) = self.provider.article(lang, title).await? {
                    return Ok(Some(a));
                }
            }
            Ok::<_, WikiError>(None)
        };
        let photo = async {
            let file = entity.image_file.as_deref()?;
            // Une photo indisponible n'empêche pas d'afficher le texte.
            match self.provider.image(file).await {
                Ok(image) => image,
                Err(WikiError::Unavailable(e)) => {
                    tracing::warn!(error = ?e, file, "photo Commons indisponible");
                    None
                }
            }
        };
        let (article, image) = tokio::join!(pick, photo);
        let Some(article) = article? else {
            return Ok(None);
        };
        let wikivoyage_url = LANGS.iter().find_map(|&lang| {
            let title = entity.sitelinks.get(&format!("{lang}wikivoyage"))?;
            article_url(&format!("https://{lang}.wikivoyage.org/wiki/"), title)
        });
        Ok(Some(WikiSummary {
            title: article.title,
            description: article.description,
            extract: article.extract,
            lang: article.lang,
            url: article.url,
            image,
            wikivoyage_url,
        }))
    }
}

/// `https://fr.wikivoyage.org/wiki/` + `San José` → `…/wiki/San_Jos%C3%A9`.
fn article_url(base: &str, title: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(base).ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .push(&title.replace(' ', "_"));
    Some(url.into())
}

/// Lien affichable : https vers un domaine Wikimedia uniquement.
fn wikimedia_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    let trusted = ["wikipedia.org", "wikimedia.org", "wikivoyage.org"]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")));
    (url.scheme() == "https" && trusted).then(|| url.into())
}

/// Texte brut d'un fragment HTML de Commons (ex. auteur avec lien), borné.
fn plain_text(html: &str) -> Option<String> {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| text.chars().take(200).collect())
}

// --- Fournisseur réel ----------------------------------------------------------------------

/// API publiques de Wikimedia (User-Agent identifiant l'application, comme l'exige leur
/// politique d'usage).
pub struct Wikimedia {
    http: reqwest::Client,
}

impl Wikimedia {
    pub fn new(user_agent: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(8))
            .build()
            .context("client HTTP Wikimedia")?;
        Ok(Self { http })
    }

    /// Corps de la réponse, `None` pour un 404.
    async fn get(&self, url: reqwest::Url) -> Result<Option<String>, WikiError> {
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| WikiError::Unavailable(e.into()))?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let res = res
            .error_for_status()
            .map_err(|e| WikiError::Unavailable(e.into()))?;
        res.text()
            .await
            .map(Some)
            .map_err(|e| WikiError::Unavailable(e.into()))
    }
}

fn unreadable(e: impl Into<anyhow::Error>) -> WikiError {
    WikiError::Unavailable(e.into().context("réponse Wikimedia illisible"))
}

#[async_trait]
impl WikiProvider for Wikimedia {
    async fn entity(&self, qid: &str) -> Result<Option<Entity>, WikiError> {
        let sites: Vec<String> = LANGS
            .iter()
            .flat_map(|l| [format!("{l}wiki"), format!("{l}wikivoyage")])
            .collect();
        let mut url = reqwest::Url::parse("https://www.wikidata.org/w/api.php").expect("URL");
        url.query_pairs_mut()
            .append_pair("action", "wbgetentities")
            .append_pair("format", "json")
            .append_pair("ids", qid)
            .append_pair("props", "sitelinks|claims")
            .append_pair("sitefilter", &sites.join("|"));
        match self.get(url).await? {
            Some(body) => parse_entity(&body, qid).map_err(unreadable),
            None => Ok(None),
        }
    }

    async fn article(&self, lang: &str, title: &str) -> Result<Option<Article>, WikiError> {
        let mut url = reqwest::Url::parse(&format!(
            "https://{lang}.wikipedia.org/api/rest_v1/page/summary/"
        ))
        .map_err(unreadable)?;
        url.path_segments_mut()
            .map_err(|()| unreadable(anyhow::anyhow!("URL")))?
            .pop_if_empty()
            .push(&title.replace(' ', "_"));
        match self.get(url).await? {
            Some(body) => parse_article(&body).map_err(unreadable),
            None => Ok(None),
        }
    }

    async fn image(&self, file: &str) -> Result<Option<WikiImage>, WikiError> {
        let mut url = reqwest::Url::parse("https://commons.wikimedia.org/w/api.php").expect("URL");
        url.query_pairs_mut()
            .append_pair("action", "query")
            .append_pair("format", "json")
            .append_pair("formatversion", "2")
            .append_pair("prop", "imageinfo")
            .append_pair("iiprop", "url|extmetadata")
            .append_pair("iiurlwidth", "640")
            .append_pair("iiextmetadatafilter", "Artist|LicenseShortName")
            .append_pair("titles", &format!("File:{file}"));
        match self.get(url).await? {
            Some(body) => parse_image(&body).map_err(unreadable),
            None => Ok(None),
        }
    }

    async fn languages(&self, qids: &[String]) -> Result<HashMap<String, u32>, WikiError> {
        if qids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut url = reqwest::Url::parse("https://www.wikidata.org/w/api.php").expect("URL");
        url.query_pairs_mut()
            .append_pair("action", "wbgetentities")
            .append_pair("format", "json")
            .append_pair("ids", &qids.join("|"))
            .append_pair("props", "sitelinks");
        match self.get(url).await? {
            Some(body) => parse_languages(&body).map_err(unreadable),
            None => Ok(HashMap::new()),
        }
    }
}

fn parse_languages(body: &str) -> anyhow::Result<HashMap<String, u32>> {
    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        entities: HashMap<String, serde_json::Value>,
    }
    let res: Response = serde_json::from_str(body)?;
    Ok(res
        .entities
        .into_iter()
        .filter(|(_, e)| e.get("missing").is_none())
        .map(|(q, e)| {
            let n = e["sitelinks"]
                .as_object()
                .map(|links| count_wikipedias(links.keys().map(String::as_str)))
                .unwrap_or(0);
            (q, n)
        })
        .collect())
}

fn parse_entity(body: &str, qid: &str) -> anyhow::Result<Option<Entity>> {
    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        entities: HashMap<String, serde_json::Value>,
    }
    let res: Response = serde_json::from_str(body)?;
    let Some(entity) = res.entities.get(qid) else {
        return Ok(None);
    };
    // Élément inexistant : `{"id": "Q…", "missing": ""}`.
    if entity.get("missing").is_some() {
        return Ok(None);
    }
    let sitelinks = entity["sitelinks"]
        .as_object()
        .map(|links| {
            links
                .iter()
                .filter_map(|(site, v)| Some((site.clone(), v["title"].as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let image_file = entity["claims"]["P18"]
        .as_array()
        .and_then(|claims| {
            // Photo « préférée » d'abord, sinon la première non dépréciée.
            claims
                .iter()
                .filter(|c| c["rank"] != "deprecated")
                .max_by_key(|c| c["rank"] == "preferred")
        })
        .and_then(|c| c["mainsnak"]["datavalue"]["value"].as_str())
        .map(str::to_owned);
    Ok(Some(Entity {
        sitelinks,
        image_file,
    }))
}

fn parse_article(body: &str) -> anyhow::Result<Option<Article>> {
    #[derive(Deserialize)]
    struct Urls {
        page: String,
    }
    #[derive(Deserialize)]
    struct ContentUrls {
        desktop: Urls,
    }
    #[derive(Deserialize)]
    struct Summary {
        #[serde(rename = "type", default)]
        kind: String,
        title: String,
        description: Option<String>,
        #[serde(default)]
        extract: String,
        lang: String,
        wikibase_item: Option<String>,
        content_urls: ContentUrls,
    }
    let s: Summary = serde_json::from_str(body)?;
    let url = wikimedia_url(&s.content_urls.desktop.page);
    // Une page d'homonymie ou vide ne présente pas le lieu.
    if s.kind != "standard" || s.extract.trim().is_empty() {
        return Ok(None);
    }
    let Some(url) = url else {
        anyhow::bail!("lien d'article inattendu : {}", s.content_urls.desktop.page);
    };
    Ok(Some(Article {
        title: s.title,
        description: s.description.filter(|d| !d.trim().is_empty()),
        extract: s.extract,
        lang: s.lang,
        url,
        wikibase_item: s.wikibase_item,
    }))
}

fn parse_image(body: &str) -> anyhow::Result<Option<WikiImage>> {
    let v: serde_json::Value = serde_json::from_str(body)?;
    let Some(info) = v["query"]["pages"][0]["imageinfo"][0].as_object() else {
        return Ok(None);
    };
    let text = |key: &str| info.get(key).and_then(|v| v.as_str());
    let number = |key: &str| info.get(key).and_then(|v| v.as_u64()).map(|n| n as u32);
    let (Some(url), Some(page_url)) = (
        text("thumburl").and_then(wikimedia_url),
        text("descriptionurl").and_then(wikimedia_url),
    ) else {
        return Ok(None);
    };
    let meta = |key: &str| {
        info.get("extmetadata")?.get(key)?["value"]
            .as_str()
            .and_then(plain_text)
    };
    Ok(Some(WikiImage {
        url,
        width: number("thumbwidth").unwrap_or(0),
        height: number("thumbheight").unwrap_or(0),
        page_url,
        artist: meta("Artist"),
        license: meta("LicenseShortName"),
    }))
}

// --- Fournisseur de test -------------------------------------------------------------------

/// Faux fournisseur pour les tests : données fixées, appels comptés, panne simulable.
#[derive(Default)]
pub struct StaticWiki {
    pub entities: HashMap<String, Entity>,
    /// Clé `langue:Titre`.
    pub articles: HashMap<String, Article>,
    pub images: HashMap<String, WikiImage>,
    /// Langues Wikipédia par élément.
    pub language_counts: HashMap<String, u32>,
    pub calls: AtomicUsize,
    pub down: std::sync::atomic::AtomicBool,
}

impl StaticWiki {
    fn enter(&self) -> Result<(), WikiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.down.load(Ordering::SeqCst) {
            Err(WikiError::Unavailable(anyhow::anyhow!("panne simulée")))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl WikiProvider for std::sync::Arc<StaticWiki> {
    async fn entity(&self, qid: &str) -> Result<Option<Entity>, WikiError> {
        self.enter()?;
        Ok(self.entities.get(qid).cloned())
    }

    async fn article(&self, lang: &str, title: &str) -> Result<Option<Article>, WikiError> {
        self.enter()?;
        Ok(self.articles.get(&format!("{lang}:{title}")).cloned())
    }

    async fn image(&self, file: &str) -> Result<Option<WikiImage>, WikiError> {
        self.enter()?;
        Ok(self.images.get(file).cloned())
    }

    async fn languages(&self, qids: &[String]) -> Result<HashMap<String, u32>, WikiError> {
        self.enter()?;
        Ok(qids
            .iter()
            .filter_map(|q| Some((q.clone(), *self.language_counts.get(q)?)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn article(lang: &str, title: &str, qid: &str) -> Article {
        Article {
            title: title.into(),
            description: None,
            extract: format!("{title} ({lang})"),
            lang: lang.into(),
            url: format!(
                "https://{lang}.wikipedia.org/wiki/{}",
                title.replace(' ', "_")
            ),
            wikibase_item: Some(qid.into()),
        }
    }

    fn photo() -> WikiImage {
        WikiImage {
            url: "https://upload.wikimedia.org/arenal.jpg".into(),
            width: 640,
            height: 480,
            page_url: "https://commons.wikimedia.org/wiki/File:Arenal.jpg".into(),
            artist: Some("Ana".into()),
            license: Some("CC BY-SA 4.0".into()),
        }
    }

    fn arenal() -> Arc<StaticWiki> {
        let mut wiki = StaticWiki::default();
        wiki.entities.insert(
            "Q641588".into(),
            Entity {
                sitelinks: [
                    ("enwiki", "Arenal Volcano"),
                    ("eswiki", "Volcán Arenal"),
                    ("frwiki", "Arenal (volcan)"),
                    ("enwikivoyage", "Arenal"),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
                image_file: Some("Arenal.jpg".into()),
            },
        );
        for (lang, title) in [("en", "Arenal Volcano"), ("es", "Volcán Arenal")] {
            wiki.articles
                .insert(format!("{lang}:{title}"), article(lang, title, "Q641588"));
        }
        wiki.images.insert("Arenal.jpg".into(), photo());
        Arc::new(wiki)
    }

    #[tokio::test]
    async fn prefers_french_then_english_and_adds_photo_and_guide() {
        let wiki = arenal();
        let svc = WikiService::new(wiki.clone());
        // L'article français est absent du faux fournisseur : repli sur l'anglais.
        let s = svc.summary(Some("Q641588"), None).await.unwrap().unwrap();
        assert_eq!(s.lang, "en");
        assert_eq!(s.title, "Arenal Volcano");
        assert_eq!(s.image, Some(photo()));
        assert_eq!(
            s.wikivoyage_url.as_deref(),
            Some("https://en.wikivoyage.org/wiki/Arenal")
        );
        // Deuxième demande : depuis le cache.
        let calls = wiki.calls.load(Ordering::SeqCst);
        svc.summary(Some("Q641588"), None).await.unwrap();
        assert_eq!(wiki.calls.load(Ordering::SeqCst), calls);
    }

    #[tokio::test]
    async fn wikipedia_tag_alone_resolves_the_item() {
        let svc = WikiService::new(arenal());
        let s = svc
            .summary(None, Some("es:Volcán Arenal"))
            .await
            .unwrap()
            .unwrap();
        // L'élément trouvé via l'article espagnol mène à l'article anglais, préféré.
        assert_eq!(s.lang, "en");
        assert!(s.image.is_some());
    }

    #[tokio::test]
    async fn unknown_references_give_nothing_and_outages_are_errors() {
        let wiki = arenal();
        let svc = WikiService::new(wiki.clone());
        assert_eq!(svc.summary(Some("Q1"), None).await.unwrap(), None);
        assert_eq!(svc.summary(None, Some("fr:Inconnu")).await.unwrap(), None);
        wiki.down.store(true, Ordering::SeqCst);
        assert!(svc.summary(Some("Q2"), None).await.is_err());
        // Les erreurs ne sont pas mises en cache.
        wiki.down.store(false, Ordering::SeqCst);
        assert_eq!(svc.summary(Some("Q2"), None).await.unwrap(), None);
    }

    // Extraits de réponses réelles (Musée national du Costa Rica, Q1968698).
    const ENTITY: &str = r#"{"entities":{"Q1968698":{"type":"item","id":"Q1968698",
        "sitelinks":{"enwiki":{"site":"enwiki","title":"Museo Nacional de Costa Rica","badges":[]},
                     "frwiki":{"site":"frwiki","title":"Musée national du Costa Rica","badges":[]}},
        "claims":{"P18":[{"mainsnak":{"snaktype":"value","property":"P18",
            "datavalue":{"value":"Fachada del Museo Nacional de Costa Rica.JPG","type":"string"},
            "datatype":"commonsMedia"},"type":"statement","rank":"normal"}]}}},"success":1}"#;

    const SUMMARY: &str = r#"{"type":"standard","title":"Musée national du Costa Rica",
        "description":"musée national situé à San José (Costa Rica)",
        "extract":"Le Musée national du Costa Rica, sis devant la Plaza de la Democracia…",
        "lang":"fr","wikibase_item":"Q1968698",
        "content_urls":{"desktop":{"page":"https://fr.wikipedia.org/wiki/Mus%C3%A9e_national_du_Costa_Rica"}}}"#;

    const IMAGE: &str = r#"{"batchcomplete":true,"query":{"pages":[{"pageid":45869280,"ns":6,
        "title":"File:Fachada del Museo Nacional de Costa Rica.JPG","imageinfo":[{
        "thumburl":"https://thumb.wikimedia.org/wikipedia/commons/thumb/6/6c/Fachada.JPG/960px-Fachada.JPG",
        "thumbwidth":640,"thumbheight":480,
        "descriptionurl":"https://commons.wikimedia.org/wiki/File:Fachada_del_Museo_Nacional_de_Costa_Rica.JPG",
        "extmetadata":{
          "Artist":{"value":"<a href=\"//commons.wikimedia.org/w/index.php?title=User:Fespinozalemistre&amp;action=edit\" class=\"new\">Fespinozalemistre</a>"},
          "LicenseShortName":{"value":"CC BY-SA 4.0"}}}]}]}}"#;

    #[test]
    fn parses_real_responses() {
        let e = parse_entity(ENTITY, "Q1968698").unwrap().unwrap();
        assert_eq!(e.sitelinks["frwiki"], "Musée national du Costa Rica");
        assert_eq!(
            e.image_file.as_deref(),
            Some("Fachada del Museo Nacional de Costa Rica.JPG")
        );
        assert_eq!(
            parse_entity(r#"{"entities":{"Q1":{"id":"Q1","missing":""}}}"#, "Q1").unwrap(),
            None
        );

        let a = parse_article(SUMMARY).unwrap().unwrap();
        assert_eq!(a.lang, "fr");
        assert_eq!(a.wikibase_item.as_deref(), Some("Q1968698"));
        assert_eq!(
            a.description.as_deref(),
            Some("musée national situé à San José (Costa Rica)")
        );
        let homonymie = SUMMARY.replace(r#""type":"standard""#, r#""type":"disambiguation""#);
        assert_eq!(parse_article(&homonymie).unwrap(), None);

        let i = parse_image(IMAGE).unwrap().unwrap();
        assert_eq!(i.artist.as_deref(), Some("Fespinozalemistre"));
        assert_eq!(i.license.as_deref(), Some("CC BY-SA 4.0"));
        assert_eq!((i.width, i.height), (640, 480));
        assert!(i
            .page_url
            .starts_with("https://commons.wikimedia.org/wiki/File:"));
    }

    #[test]
    fn counts_wikipedia_languages() {
        let body = r#"{"entities":{
            "Q641588":{"id":"Q641588","sitelinks":{"enwiki":{},"eswiki":{},"frwiki":{},
                "commonswiki":{},"enwikivoyage":{},"zh_min_nanwiki":{}}},
            "Q1":{"id":"Q1","missing":""}}}"#;
        let counts = parse_languages(body).unwrap();
        assert_eq!(counts.get("Q641588"), Some(&4));
        assert_eq!(counts.get("Q1"), None);
    }

    #[tokio::test]
    async fn language_counts_are_cached_and_never_fail() {
        let mut wiki = StaticWiki::default();
        wiki.language_counts.insert("Q641588".into(), 40);
        let wiki = Arc::new(wiki);
        let svc = WikiService::new(wiki.clone());
        let qids = vec!["Q641588".to_owned(), "Q1".to_owned()];
        let counts = svc.languages(&qids).await;
        assert_eq!(counts["Q641588"], 40);
        assert_eq!(counts["Q1"], 0);
        wiki.down.store(true, Ordering::SeqCst);
        // Depuis le cache ; un élément inconnu sans réponse : absent (compté 0 par l'appelant).
        let again = svc.languages(&[qids[0].clone(), "Q2".to_owned()]).await;
        assert_eq!(again.get("Q641588"), Some(&40));
        assert_eq!(again.get("Q2"), None);
    }

    #[test]
    fn only_wikimedia_links_are_kept() {
        assert!(wikimedia_url("https://fr.wikipedia.org/wiki/X").is_some());
        assert!(wikimedia_url("https://upload.wikimedia.org/a.jpg").is_some());
        for bad in [
            "http://fr.wikipedia.org/wiki/X",
            "https://evil.example/wikipedia.org",
            "https://wikipedia.org.evil.example/x",
            "javascript:alert(1)",
        ] {
            assert_eq!(wikimedia_url(bad), None, "{bad}");
        }
        assert_eq!(
            plain_text("<b>Ana</b> &amp; <i>Bob</i>").as_deref(),
            Some("Ana & Bob")
        );
        assert_eq!(plain_text("<br>"), None);
        assert_eq!(
            article_url("https://fr.wikivoyage.org/wiki/", "San José").as_deref(),
            Some("https://fr.wikivoyage.org/wiki/San_Jos%C3%A9")
        );
    }
}
