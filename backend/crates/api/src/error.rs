//! Erreurs de l'API, sérialisées dans un format JSON uniforme.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

/// Corps JSON renvoyé pour toute erreur.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// Code stable, exploitable par le frontend (ex. `not_found`).
    pub code: &'static str,
    /// Message lisible.
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("authentification requise")]
    Unauthorized,
    #[error("accès refusé")]
    Forbidden,
    #[error("ressource introuvable")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("trop de tentatives, réessayez plus tard")]
    TooManyRequests,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            AppError::Database(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Database(_) | AppError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            // Ne jamais exposer le détail d'une erreur interne au client.
            tracing::error!(error = ?self, "erreur interne");
            "erreur interne".to_owned()
        } else {
            self.to_string()
        };
        (status, Json(ErrorBody { code, message })).into_response()
    }
}
