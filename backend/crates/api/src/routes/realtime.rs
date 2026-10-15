//! Canal temps réel d'un voyage (WebSocket).

use std::time::Duration;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult, ErrorBody};
use crate::realtime;
use crate::state::AppState;
use crate::trips::{self, Role};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(trip_socket))
}

/// Ouvertures de connexion par utilisateur et par minute (reconnexions comprises).
const CONNECTIONS_PER_USER_PER_MINUTE: u32 = 30;
/// Les clients n'envoient rien d'utile : messages entrants minuscules.
const MAX_INCOMING_MESSAGE: usize = 1024;

/// Ouvre le canal temps réel d'un voyage (membres). Messages JSON envoyés par le serveur :
/// `changed` (sujets modifiés, auteur, onglet d'origine), `presence` (membres en ligne),
/// `removed` (accès perdu, la connexion se ferme) et `resync` (tout recharger).
///
/// L'en-tête `Origin` doit être une origine autorisée (protection contre le détournement de
/// WebSocket intersite, le cookie de session étant envoyé automatiquement).
#[utoipa::path(get, path = "/trips/{trip_id}/ws", tag = "temps réel",
    params(("trip_id" = Uuid, Path)),
    responses((status = 101, description = "Connexion WebSocket établie"),
        (status = 401, body = ErrorBody), (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody), (status = 429, body = ErrorBody)))]
async fn trip_socket(
    State(state): State<AppState>,
    current: CurrentUser,
    Path(trip_id): Path<Uuid>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> AppResult<Response> {
    let origin_ok = headers
        .get(header::ORIGIN)
        .and_then(|o| o.to_str().ok())
        .is_some_and(|o| state.config.is_allowed_origin(o));
    if !origin_ok {
        return Err(AppError::Forbidden);
    }
    trips::require_role(&state.db, trip_id, current.user.id, Role::Viewer).await?;
    state.rate_limiter.check(
        &format!("ws:{}", current.user.id),
        CONNECTIONS_PER_USER_PER_MINUTE,
        Duration::from_secs(60),
    )?;
    let CurrentUser {
        user,
        session_token,
    } = current;
    Ok(ws
        .max_message_size(MAX_INCOMING_MESSAGE)
        .on_upgrade(move |socket| realtime::serve(socket, state, trip_id, user, session_token)))
}
