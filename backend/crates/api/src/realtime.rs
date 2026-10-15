//! Collaboration en temps réel : un canal WebSocket par voyage.
//!
//! Le serveur ne pousse que des **notifications** (« les lieux ont changé ») : chaque client
//! recharge ensuite ce qui le concerne par l'API REST, avec ses propres droits. Aucune donnée
//! ne transite par le canal, ce qui évite toute fuite (emails des membres, par exemple).
//!
//! Une seule instance de l'API : le [`Hub`] est en mémoire. Pour plusieurs instances, il
//! faudrait relayer les notifications (PostgreSQL `LISTEN/NOTIFY`, par exemple).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::Serialize;
use tokio::sync::broadcast;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::session;
use crate::state::AppState;
use crate::trips;
use crate::users::UserRow;

/// Ce qui a changé dans un voyage ; le client recharge les données correspondantes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// Paramètres du voyage (nom, dates, modes par défaut) ou rôle d'un membre.
    Trip,
    Members,
    Invitations,
    Pois,
    Reviews,
    Days,
    Itinerary,
}

/// Auteur d'une modification.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Actor {
    pub user_id: Uuid,
    pub name: String,
}

impl From<&UserRow> for Actor {
    fn from(u: &UserRow) -> Self {
        Self {
            user_id: u.id,
            name: u.display_name.clone(),
        }
    }
}

/// Message envoyé aux clients (JSON, champ `type`).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Des données du voyage ont changé.
    Changed {
        topics: Vec<Topic>,
        by: Actor,
        /// Onglet à l'origine de la modification (en-tête `X-Client-Id`) : il l'ignore.
        client_id: Option<String>,
    },
    /// Membres connectés (un par personne, quel que soit le nombre d'onglets).
    Presence { online: Vec<Actor> },
    /// Vous n'avez plus accès à ce voyage (retiré, voyage supprimé, session terminée).
    Removed,
    /// Des notifications ont été perdues : rechargez tout.
    Resync,
}

/// Événement interne d'un voyage, filtré connexion par connexion.
#[derive(Debug, Clone)]
enum Event {
    Message(ClientMessage),
    /// Un membre perd l'accès au voyage : ses connexions se ferment.
    Kick(Uuid),
    /// Le voyage est supprimé : toutes les connexions se ferment.
    Deleted,
}

/// Notifications en attente par voyage avant qu'un client trop lent ne décroche.
const CHANNEL_CAPACITY: usize = 64;
/// Connexions simultanées maximales par voyage.
pub const MAX_CONNECTIONS_PER_TRIP: usize = 200;

struct TripChannel {
    tx: broadcast::Sender<Event>,
    online: HashMap<u64, Actor>,
}

/// Canaux des voyages ouverts (en mémoire, une instance).
#[derive(Default)]
pub struct Hub {
    trips: Mutex<HashMap<Uuid, TripChannel>>,
    next_id: AtomicU64,
}

/// Abonnement d'une connexion ; se désinscrit (et met à jour la présence) en disparaissant.
pub struct Subscription {
    hub: Arc<Hub>,
    trip_id: Uuid,
    conn_id: u64,
    rx: broadcast::Receiver<Event>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.hub.leave(self.trip_id, self.conn_id);
    }
}

fn presence(online: &HashMap<u64, Actor>) -> ClientMessage {
    let mut people: Vec<Actor> = Vec::new();
    for actor in online.values() {
        if !people.iter().any(|p| p.user_id == actor.user_id) {
            people.push(actor.clone());
        }
    }
    people.sort_by(|a, b| a.name.cmp(&b.name).then(a.user_id.cmp(&b.user_id)));
    ClientMessage::Presence { online: people }
}

impl Hub {
    /// Inscrit une connexion ; `None` si le voyage a atteint sa limite de connexions.
    pub fn join(self: &Arc<Self>, trip_id: Uuid, actor: Actor) -> Option<Subscription> {
        let mut trips = self.trips.lock().expect("mutex empoisonné");
        let channel = trips.entry(trip_id).or_insert_with(|| TripChannel {
            tx: broadcast::channel(CHANNEL_CAPACITY).0,
            online: HashMap::new(),
        });
        if channel.online.len() >= MAX_CONNECTIONS_PER_TRIP {
            return None;
        }
        let conn_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // Abonné avant l'annonce : la nouvelle connexion reçoit la présence à jour.
        let rx = channel.tx.subscribe();
        channel.online.insert(conn_id, actor);
        let _ = channel.tx.send(Event::Message(presence(&channel.online)));
        Some(Subscription {
            hub: Arc::clone(self),
            trip_id,
            conn_id,
            rx,
        })
    }

    fn leave(&self, trip_id: Uuid, conn_id: u64) {
        let mut trips = self.trips.lock().expect("mutex empoisonné");
        let Some(channel) = trips.get_mut(&trip_id) else {
            return;
        };
        channel.online.remove(&conn_id);
        if channel.online.is_empty() {
            trips.remove(&trip_id);
        } else {
            let _ = channel.tx.send(Event::Message(presence(&channel.online)));
        }
    }

    fn send(&self, trip_id: Uuid, event: Event) {
        let trips = self.trips.lock().expect("mutex empoisonné");
        if let Some(channel) = trips.get(&trip_id) {
            // Personne n'écoute : rien à faire.
            let _ = channel.tx.send(event);
        }
    }

    /// Signale une modification aux membres connectés.
    pub fn changed(&self, trip_id: Uuid, topics: &[Topic], by: &UserRow, client: &ClientId) {
        self.send(
            trip_id,
            Event::Message(ClientMessage::Changed {
                topics: topics.to_vec(),
                by: by.into(),
                client_id: client.0.clone(),
            }),
        );
    }

    /// Ferme les connexions d'un membre qui perd l'accès au voyage.
    pub fn kick(&self, trip_id: Uuid, user_id: Uuid) {
        self.send(trip_id, Event::Kick(user_id));
    }

    /// Ferme toutes les connexions d'un voyage supprimé.
    pub fn deleted(&self, trip_id: Uuid) {
        self.send(trip_id, Event::Deleted);
    }

    /// Nombre de connexions ouvertes sur un voyage (tests, supervision).
    pub fn connections(&self, trip_id: Uuid) -> usize {
        let trips = self.trips.lock().expect("mutex empoisonné");
        trips.get(&trip_id).map_or(0, |c| c.online.len())
    }
}

/// Identifiant de l'onglet à l'origine d'une requête (en-tête `X-Client-Id`), renvoyé
/// dans les notifications pour qu'il ignore les siennes.
#[derive(Debug, Clone, Default)]
pub struct ClientId(pub Option<String>);

pub const CLIENT_ID_HEADER: &str = "x-client-id";

impl<S: Send + Sync> FromRequestParts<S> for ClientId {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let id = parts
            .headers
            .get(CLIENT_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .filter(|v| {
                (1..=64).contains(&v.len())
                    && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
            .map(str::to_owned);
        Ok(Self(id))
    }
}

/// Intervalle des pings (garde la connexion ouverte à travers les proxys) et des
/// revérifications de la session et de l'appartenance au voyage.
const HEARTBEAT: Duration = Duration::from_secs(30);

/// Fait vivre une connexion : relaie les notifications, vérifie régulièrement que
/// l'utilisateur a toujours accès au voyage, et se termine à la fermeture du client.
pub async fn serve(
    mut socket: WebSocket,
    state: AppState,
    trip_id: Uuid,
    user: UserRow,
    session_token: String,
) {
    let Some(mut sub) = state.realtime.join(trip_id, Actor::from(&user)) else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    tracing::debug!(%trip_id, user_id = %user.id, "connexion temps réel ouverte");
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.tick().await;
    loop {
        let outgoing = tokio::select! {
            incoming = socket.recv() => match incoming {
                // Le client n'envoie rien d'utile : on ne lit que la fermeture.
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => continue,
            },
            event = sub.rx.recv() => match event {
                Ok(Event::Message(m)) => Some(m),
                Ok(Event::Kick(id)) if id == user.id => {
                    let _ = send_json(&mut socket, &ClientMessage::Removed).await;
                    break;
                }
                Ok(Event::Kick(_)) => None,
                Ok(Event::Deleted) => {
                    let _ = send_json(&mut socket, &ClientMessage::Removed).await;
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => Some(ClientMessage::Resync),
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = heartbeat.tick() => {
                if !still_allowed(&state, trip_id, user.id, &session_token).await {
                    let _ = send_json(&mut socket, &ClientMessage::Removed).await;
                    break;
                }
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
                None
            }
        };
        if let Some(message) = outgoing {
            if send_json(&mut socket, &message).await.is_err() {
                break;
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
    tracing::debug!(%trip_id, user_id = %user.id, "connexion temps réel fermée");
}

async fn send_json(socket: &mut WebSocket, message: &ClientMessage) -> Result<(), axum::Error> {
    let text = serde_json::to_string(message).expect("message sérialisable");
    socket.send(Message::Text(text.into())).await
}

/// Session toujours valide (pas de déconnexion ni de suppression du compte) et toujours
/// membre du voyage ? Une erreur de base de données ne ferme pas la connexion. La session
/// n'est pas prolongée : un onglet resté ouvert n'est pas une activité.
async fn still_allowed(state: &AppState, trip_id: Uuid, user_id: Uuid, token: &str) -> bool {
    let session = session::peek(&state.db, token).await;
    let role = trips::role_of(&state.db, trip_id, user_id).await;
    match (session, role) {
        (Ok(Some(u)), Ok(Some(_))) => u.id == user_id,
        (Ok(None), _) | (_, Ok(None)) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(n: u128, name: &str) -> Actor {
        Actor {
            user_id: Uuid::from_u128(n),
            name: name.into(),
        }
    }

    fn next(sub: &mut Subscription) -> Event {
        sub.rx.try_recv().expect("événement attendu")
    }

    fn online(event: Event) -> Vec<String> {
        match event {
            Event::Message(ClientMessage::Presence { online }) => {
                online.into_iter().map(|a| a.name).collect()
            }
            other => panic!("présence attendue : {other:?}"),
        }
    }

    #[test]
    fn presence_follows_connections() {
        let hub = Arc::new(Hub::default());
        let trip = Uuid::from_u128(1);
        let mut ana = hub.join(trip, actor(1, "Ana")).unwrap();
        assert_eq!(online(next(&mut ana)), ["Ana"]);

        let mut bob = hub.join(trip, actor(2, "Bob")).unwrap();
        assert_eq!(online(next(&mut ana)), ["Ana", "Bob"]);
        assert_eq!(online(next(&mut bob)), ["Ana", "Bob"]);

        // Deux onglets de Bob : il n'apparaît qu'une fois.
        let bob2 = hub.join(trip, actor(2, "Bob")).unwrap();
        assert_eq!(online(next(&mut ana)), ["Ana", "Bob"]);
        assert_eq!(hub.connections(trip), 3);
        drop(bob2);
        drop(bob);
        let _ = next(&mut ana);
        assert_eq!(online(next(&mut ana)), ["Ana"]);
        drop(ana);
        assert_eq!(hub.connections(trip), 0);
    }

    #[test]
    fn notifications_stay_within_their_trip() {
        let hub = Arc::new(Hub::default());
        let (t1, t2) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut a = hub.join(t1, actor(1, "Ana")).unwrap();
        let mut b = hub.join(t2, actor(2, "Bob")).unwrap();
        let _ = (next(&mut a), next(&mut b));

        hub.send(t1, Event::Kick(Uuid::from_u128(9)));
        assert!(matches!(next(&mut a), Event::Kick(_)));
        assert!(b.rx.try_recv().is_err());
        // Voyage sans connexion : rien ne se passe.
        hub.deleted(Uuid::from_u128(3));
    }

    #[test]
    fn client_id_is_validated() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let parse = |v: &str| {
            let req = axum::http::Request::builder()
                .header(CLIENT_ID_HEADER, v)
                .body(())
                .unwrap();
            let (mut parts, _) = req.into_parts();
            let Ok(ClientId(id)) = runtime.block_on(ClientId::from_request_parts(&mut parts, &()));
            id
        };
        assert_eq!(parse("abc-123").as_deref(), Some("abc-123"));
        assert_eq!(parse("pas valide !"), None);
        assert_eq!(parse(&"x".repeat(65)), None);
    }
}
