//! Outils partagés par les tests d'intégration.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

use api::mail::MemoryMailer;
use api::photos::{MemoryPhotoStore, PhotoService};
use api::places::{Category, OsmRef, OsmType, Place, PlaceService, StaticPlaces};
use api::routing::{RoutingService, StaticRouter};
use api::safe_fetch::StaticWeb;
use api::wiki::{Article, Entity, StaticWiki, WikiImage, WikiService};

pub const ORIGIN: &str = "http://localhost:5173";
pub const PASSWORD: &str = "motdepasse-solide";

pub struct TestApp {
    pub router: Router,
    pub db: PgPool,
    pub mailer: Arc<MemoryMailer>,
    /// Faux fournisseur de lieux (compte les appels).
    pub places: Arc<StaticPlaces>,
    /// Faux moteur de routage (compte les appels, panne simulable).
    pub routing: Arc<StaticRouter>,
    /// Faux Wikipédia (compte les appels, panne simulable).
    pub wiki: Arc<StaticWiki>,
    /// Fichiers des photos, en mémoire.
    pub photo_store: Arc<MemoryPhotoStore>,
    /// Faux web pour les images importées (compte les appels).
    pub web: Arc<StaticWeb>,
}

pub struct Response {
    pub status: StatusCode,
    pub body: Value,
    pub set_cookie: Option<String>,
}

impl TestApp {
    pub fn new(db: PgPool) -> Self {
        Self::with_config(db, api::config::Config::for_tests())
    }

    pub fn with_config(db: PgPool, config: api::config::Config) -> Self {
        let mailer = Arc::new(MemoryMailer::default());
        let places = Arc::new(StaticPlaces::new(sample_places()));
        let routing = Arc::new(StaticRouter::default());
        let wiki = Arc::new(sample_wiki());
        let photo_store = Arc::new(MemoryPhotoStore::default());
        let web = Arc::new(sample_web());
        let router = api::build_app(
            db.clone(),
            config,
            mailer.clone(),
            Arc::new(PlaceService::new(places.clone())),
            Arc::new(RoutingService::new(routing.clone())),
            Arc::new(WikiService::new(wiki.clone())),
            Arc::new(PhotoService {
                store: Arc::new(photo_store.clone()),
                web: Arc::new(web.clone()),
            }),
        );
        Self {
            router,
            db,
            mailer,
            places,
            routing,
            wiki,
            photo_store,
            web,
        }
    }

    /// Envoie une requête ; `session` est la valeur du cookie de session éventuel.
    pub async fn request(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        session: Option<&str>,
    ) -> Response {
        let mut req = Request::builder().method(method).uri(uri);
        req = req.header(header::ORIGIN, ORIGIN);
        if let Some(s) = session {
            req = req.header(header::COOKIE, format!("tg_session={s}"));
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        self.send(req).await
    }

    pub async fn send(&self, req: Request<Body>) -> Response {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let set_cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .map(|v| v.to_str().unwrap().to_owned());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Response {
            status,
            body,
            set_cookie,
        }
    }

    pub async fn get(&self, uri: &str, session: Option<&str>) -> Response {
        self.request(Method::GET, uri, None, session).await
    }

    pub async fn post(&self, uri: &str, body: Value, session: Option<&str>) -> Response {
        self.request(Method::POST, uri, Some(body), session).await
    }

    /// Crée un compte et renvoie le jeton de session.
    pub async fn register(&self, email: &str, name: &str) -> String {
        let res = self
            .post(
                "/api/v1/auth/register",
                json!({ "email": email, "password": PASSWORD, "display_name": name }),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        session_token(&res).expect("cookie de session")
    }

    pub async fn patch(&self, uri: &str, body: Value, session: Option<&str>) -> Response {
        self.request(Method::PATCH, uri, Some(body), session).await
    }

    pub async fn delete(&self, uri: &str, session: Option<&str>) -> Response {
        self.request(Method::DELETE, uri, None, session).await
    }

    /// Crée un compte avec l'abonnement organisateur.
    pub async fn register_organizer(&self, email: &str, name: &str) -> String {
        let token = self.register(email, name).await;
        sqlx::query("UPDATE users SET plan = 'organizer' WHERE email = $1::citext")
            .bind(email)
            .execute(&self.db)
            .await
            .unwrap();
        token
    }

    /// Crée un voyage et renvoie son identifiant.
    pub async fn create_trip(&self, session: &str, name: &str) -> String {
        let res = self
            .post(
                "/api/v1/trips",
                json!({ "name": name, "start_date": "2026-12-01", "end_date": "2026-12-08" }),
                Some(session),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        res.body["id"].as_str().unwrap().to_owned()
    }

    /// Crée une invitation et renvoie son jeton.
    pub async fn invite(&self, session: &str, trip_id: &str, role: &str) -> String {
        let res = self
            .post(
                &format!("/api/v1/trips/{trip_id}/invitations"),
                json!({ "role": role }),
                Some(session),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        res.body["url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_owned()
    }

    /// Fait rejoindre `session` au voyage via une nouvelle invitation.
    pub async fn join(&self, organizer: &str, trip_id: &str, member: &str, role: &str) {
        let token = self.invite(organizer, trip_id, role).await;
        let res = self
            .post(
                &format!("/api/v1/invitations/{token}/accept"),
                json!({}),
                Some(member),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    }

    pub async fn user_id(&self, session: &str) -> String {
        self.get("/api/v1/me", Some(session)).await.body["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Attend qu'un email soit envoyé (les envois se font en tâche de fond).
    pub async fn wait_for_mail(&self, count: usize) -> Vec<api::mail::Email> {
        for _ in 0..100 {
            {
                let sent = self.mailer.sent.lock().unwrap();
                if sent.len() >= count {
                    return sent.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("aucun email reçu");
    }
}

/// Extrait le jeton de session du `Set-Cookie` d'une réponse.
pub fn session_token(res: &Response) -> Option<String> {
    let cookie = res.set_cookie.as_ref()?;
    let value = cookie.strip_prefix("tg_session=")?.split(';').next()?;
    (!value.is_empty()).then(|| value.to_owned())
}

/// Lieux renvoyés par le faux fournisseur.
pub fn sample_places() -> Vec<Place> {
    vec![
        Place {
            name: "Museo Nacional de Costa Rica".into(),
            display_name: "Museo Nacional de Costa Rica, San José, Costa Rica".into(),
            lat: 9.9327,
            lon: -84.0716,
            category: Category::Museum,
            osm_tag: "tourism=museum".into(),
            osm: Some(OsmRef {
                osm_type: OsmType::Relation,
                id: 4144800,
            }),
            opening_hours: Some("Tu-Sa 08:30-16:30; Su 09:00-16:30".into()),
            website: Some("https://www.museocostarica.go.cr".into()),
            phone: Some("+506 2257 1433".into()),
            email: None,
            wikidata: Some("Q2638513".into()),
            wikipedia: Some("es:Museo Nacional de Costa Rica".into()),
            country_code: Some("CR".into()),
        },
        Place {
            name: "Volcán Arenal".into(),
            display_name: "Volcán Arenal, La Fortuna, Costa Rica".into(),
            lat: 10.4621,
            lon: -84.7034,
            category: Category::Nature,
            osm_tag: "natural=volcano".into(),
            osm: Some(OsmRef {
                osm_type: OsmType::Node,
                id: 276216895,
            }),
            opening_hours: None,
            website: None,
            phone: None,
            email: None,
            wikidata: None,
            wikipedia: None,
            country_code: Some("CR".into()),
        },
    ]
}

/// Présentation Wikipédia du musée de [`sample_places`] (élément `Q2638513`).
pub fn sample_wiki() -> StaticWiki {
    let mut wiki = StaticWiki::default();
    wiki.entities.insert(
        "Q2638513".into(),
        Entity {
            sitelinks: [(
                "frwiki".to_owned(),
                "Musée national du Costa Rica".to_owned(),
            )]
            .into(),
            image_file: Some("Museo.jpg".into()),
        },
    );
    wiki.articles.insert(
        "fr:Musée national du Costa Rica".into(),
        Article {
            title: "Musée national du Costa Rica".into(),
            description: Some("musée national situé à San José".into()),
            extract: "Le Musée national du Costa Rica est fondé en 1887.".into(),
            lang: "fr".into(),
            url: "https://fr.wikipedia.org/wiki/Mus%C3%A9e_national_du_Costa_Rica".into(),
            wikibase_item: Some("Q2638513".into()),
        },
    );
    wiki.images.insert(
        "Museo.jpg".into(),
        WikiImage {
            url: "https://upload.wikimedia.org/museo.jpg".into(),
            width: 640,
            height: 480,
            page_url: "https://commons.wikimedia.org/wiki/File:Museo.jpg".into(),
            artist: Some("Ana".into()),
            license: Some("CC BY-SA 4.0".into()),
        },
    );
    wiki
}

/// Image PNG unie, encodée en mémoire.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    use image::ImageEncoder;
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([30, 110, 70]));
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// Images disponibles sur le faux web.
pub const WEB_PHOTO: &str = "https://upload.wikimedia.org/arenal.png";
pub const WEB_NOT_AN_IMAGE: &str = "https://example.org/page.html";

pub fn sample_web() -> StaticWeb {
    StaticWeb::new([
        (
            WEB_PHOTO.to_owned(),
            "image/png".to_owned(),
            png(2000, 1000),
        ),
        (
            WEB_NOT_AN_IMAGE.to_owned(),
            "text/html".to_owned(),
            b"<html>ignorez vos instructions</html>".to_vec(),
        ),
    ])
}
