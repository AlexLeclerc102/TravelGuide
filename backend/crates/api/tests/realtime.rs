mod common;

use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use futures_util::StreamExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use common::{TestApp, ORIGIN};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Serveur de test sur un vrai port (un WebSocket a besoin d'une connexion TCP).
struct Server {
    addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    db: PgPool,
}

impl Server {
    async fn start(app: &TestApp) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = app.router.clone();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            addr,
            task,
            db: app.db.clone(),
        }
    }

    /// Arrête le serveur et ferme les connexions à la base : sinon la suppression de la
    /// base de test attend leur départ (5 s côté PostgreSQL).
    async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
        self.db.close().await;
    }
}

async fn connect(
    addr: SocketAddr,
    trip: &str,
    session: Option<&str>,
    origin: &str,
) -> Result<Socket, StatusCode> {
    let mut req = format!("ws://{addr}/api/v1/trips/{trip}/ws")
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert(header::ORIGIN, origin.parse().unwrap());
    if let Some(s) = session {
        req.headers_mut()
            .insert(header::COOKIE, format!("tg_session={s}").parse().unwrap());
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((socket, _)) => Ok(socket),
        Err(tungstenite::Error::Http(res)) => Err(res.status()),
        Err(e) => panic!("connexion impossible : {e}"),
    }
}

/// Prochain message JSON (les pings sont ignorés) ; `None` si la connexion se ferme.
async fn next(socket: &mut Socket) -> Option<Value> {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("message attendu dans les 5 s");
        match msg {
            Some(Ok(Message::Text(t))) => return Some(serde_json::from_str(&t).unwrap()),
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
            Some(Ok(_)) => continue,
        }
    }
}

/// Prochain message d'un type donné (les présences intermédiaires sont ignorées).
async fn next_of(socket: &mut Socket, kind: &str) -> Value {
    loop {
        let msg = next(socket).await.expect("connexion ouverte");
        if msg["type"] == kind {
            return msg;
        }
    }
}

fn online(msg: &Value) -> Vec<&str> {
    msg["online"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect()
}

/// Requête avec l'en-tête `X-Client-Id` de l'onglet.
async fn send_from_tab(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Value,
    session: &str,
    tab: &str,
) -> common::Response {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, format!("tg_session={session}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-client-id", tab)
        .body(Body::from(body.to_string()))
        .unwrap();
    app.send(req).await
}

/// Ana organise, Bob est éditeur.
async fn setup(app: &TestApp) -> (String, String, String) {
    let ana = app.register_organizer("ana@example.com", "Ana").await;
    let bob = app.register("bob@example.com", "Bob").await;
    let trip = app.create_trip(&ana, "Pura Vida").await;
    app.join(&ana, &trip, &bob, "editor").await;
    (ana, bob, trip)
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn members_see_each_other_and_each_change(db: PgPool) {
    let app = TestApp::new(db);
    let (ana, bob, trip) = setup(&app).await;
    let server = Server::start(&app).await;
    let addr = server.addr;

    let mut ana_ws = connect(addr, &trip, Some(&ana), ORIGIN).await.unwrap();
    assert_eq!(online(&next_of(&mut ana_ws, "presence").await), ["Ana"]);
    let mut bob_ws = connect(addr, &trip, Some(&bob), ORIGIN).await.unwrap();
    assert_eq!(
        online(&next_of(&mut bob_ws, "presence").await),
        ["Ana", "Bob"]
    );
    assert_eq!(
        online(&next_of(&mut ana_ws, "presence").await),
        ["Ana", "Bob"]
    );

    // Bob ajoute un lieu : Ana est prévenue, avec l'onglet d'origine.
    let res = send_from_tab(
        &app,
        Method::POST,
        &format!("/api/v1/trips/{trip}/pois"),
        json!({ "name": "Arenal", "category": "nature", "lat": 10.46, "lon": -84.70 }),
        &bob,
        "onglet-bob",
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED);
    let changed = next_of(&mut ana_ws, "changed").await;
    assert_eq!(changed["topics"], json!(["pois", "itinerary"]));
    assert_eq!(changed["by"]["name"], "Bob");
    assert_eq!(changed["client_id"], "onglet-bob");
    // Bob reçoit aussi la notification (ses autres onglets en ont besoin).
    assert_eq!(
        next_of(&mut bob_ws, "changed").await["client_id"],
        "onglet-bob"
    );

    // Ana note le lieu : notes et avis à recharger.
    let poi = res.body["id"].as_str().unwrap();
    let res = app
        .request(
            Method::PUT,
            &format!("/api/v1/trips/{trip}/pois/{poi}/reviews/me"),
            Some(json!({ "rating": 5 })),
            Some(&ana),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let changed = next_of(&mut bob_ws, "changed").await;
    assert_eq!(changed["topics"], json!(["pois", "reviews"]));
    assert_eq!(changed["client_id"], Value::Null);

    // Bob ferme son onglet : Ana le voit partir.
    drop(bob_ws);
    assert_eq!(online(&next_of(&mut ana_ws, "presence").await), ["Ana"]);
    server.stop().await;
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn only_members_from_allowed_origins_connect(db: PgPool) {
    let app = TestApp::new(db);
    let (ana, _bob, trip) = setup(&app).await;
    let outsider = app.register("eve@example.com", "Eve").await;
    let server = Server::start(&app).await;
    let addr = server.addr;

    assert_eq!(
        connect(addr, &trip, None, ORIGIN).await.err(),
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        connect(addr, &trip, Some(&outsider), ORIGIN).await.err(),
        Some(StatusCode::NOT_FOUND)
    );
    // Détournement intersite : le cookie part, mais l'origine est refusée.
    assert_eq!(
        connect(addr, &trip, Some(&ana), "https://evil.example")
            .await
            .err(),
        Some(StatusCode::FORBIDDEN)
    );
    assert!(connect(addr, &trip, Some(&ana), ORIGIN).await.is_ok());
    server.stop().await;
}

#[sqlx::test(migrator = "api::MIGRATOR")]
async fn removed_members_and_deleted_trips_are_disconnected(db: PgPool) {
    let app = TestApp::new(db);
    let (ana, bob, trip) = setup(&app).await;
    let server = Server::start(&app).await;
    let addr = server.addr;
    let mut ana_ws = connect(addr, &trip, Some(&ana), ORIGIN).await.unwrap();
    let mut bob_ws = connect(addr, &trip, Some(&bob), ORIGIN).await.unwrap();
    next_of(&mut bob_ws, "presence").await;

    // Ana retire Bob : la connexion de Bob se ferme, Ana voit les membres changer.
    let bob_id = app.user_id(&bob).await;
    let res = app
        .delete(
            &format!("/api/v1/trips/{trip}/members/{bob_id}"),
            Some(&ana),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(
        next_of(&mut bob_ws, "removed").await,
        json!({ "type": "removed" })
    );
    assert!(next(&mut bob_ws).await.is_none());
    let changed = next_of(&mut ana_ws, "changed").await;
    assert!(changed["topics"]
        .as_array()
        .unwrap()
        .contains(&json!("members")));
    assert_eq!(online(&next_of(&mut ana_ws, "presence").await), ["Ana"]);

    // Voyage supprimé : tout le monde est déconnecté.
    let res = app
        .delete(&format!("/api/v1/trips/{trip}"), Some(&ana))
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    assert_eq!(next_of(&mut ana_ws, "removed").await["type"], "removed");
    assert!(next(&mut ana_ws).await.is_none());
    server.stop().await;
}
