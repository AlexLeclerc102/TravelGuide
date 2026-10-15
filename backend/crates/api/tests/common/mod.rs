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

pub const ORIGIN: &str = "http://localhost:5173";
pub const PASSWORD: &str = "motdepasse-solide";

pub struct TestApp {
    pub router: Router,
    pub db: PgPool,
    pub mailer: Arc<MemoryMailer>,
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
        let router = api::build_app(db.clone(), config, mailer.clone());
        Self { router, db, mailer }
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
