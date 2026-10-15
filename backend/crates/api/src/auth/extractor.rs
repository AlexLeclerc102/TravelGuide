use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;

use super::session;
use crate::error::AppError;
use crate::state::AppState;
use crate::users::UserRow;

/// Utilisateur authentifié par son cookie de session. Rejette avec 401 sinon.
pub struct CurrentUser {
    pub user: UserRow,
    /// Jeton de la session courante (pour la déconnexion).
    pub session_token: String,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(session::COOKIE_NAME)
            .map(|c| c.value().to_owned())
            .filter(|t| !t.is_empty())
            .ok_or(AppError::Unauthorized)?;
        let user = session::authenticate(&state.db, &token)
            .await?
            .ok_or(AppError::Unauthorized)?;
        Ok(Self {
            user,
            session_token: token,
        })
    }
}

/// `Option<CurrentUser>` : `None` si aucune session valide (routes publiques).
impl axum::extract::OptionalFromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Option<Self>, AppError> {
        match <Self as FromRequestParts<AppState>>::from_request_parts(parts, state).await {
            Ok(user) => Ok(Some(user)),
            Err(AppError::Unauthorized) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
