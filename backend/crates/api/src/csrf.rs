//! Protection CSRF : toute requête d'écriture doit provenir d'une origine autorisée.
//!
//! Complète le cookie `SameSite=Lax`. Les navigateurs envoient toujours l'en-tête
//! `Origin` sur les requêtes POST/PUT/PATCH/DELETE émises par `fetch`.

use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::error::ErrorBody;
use crate::state::AppState;

pub async fn verify_origin(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }
    let allowed = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|o| o.to_str().ok())
        .is_some_and(|o| state.config.is_allowed_origin(o));
    if allowed {
        next.run(req).await
    } else {
        (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                code: "bad_origin",
                message: "origine de la requête non autorisée".into(),
            }),
        )
            .into_response()
    }
}
