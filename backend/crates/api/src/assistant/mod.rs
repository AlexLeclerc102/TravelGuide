//! Assistant IA d'un voyage (voir `docs/AI_ASSISTANT.md`).
//!
//! Incrément A0 : accès aux modèles de langage ([`llm`]) et client OpenRouter
//! ([`openrouter`]). Conversations, outils et boucle d'agent viennent ensuite.

pub mod llm;
pub mod openrouter;

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::config::Config;
use llm::LlmProvider;

/// Requête envoyée par l'assistant au routeur de l'API, pour le compte de l'utilisateur.
///
/// C'est une *extension* de requête, posée par le répartiteur interne des outils : un
/// client externe ne peut pas la fournir (contrairement à un en-tête).
#[derive(Debug, Clone, Copy)]
pub struct AssistantCall {
    pub conversation_id: Uuid,
}

/// La requête vient-elle de l'assistant ? (attribution « via l'assistant »)
#[derive(Debug, Clone, Copy, Default)]
pub struct ViaAssistant(pub bool);

impl<S: Send + Sync> FromRequestParts<S> for ViaAssistant {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(Self(parts.extensions.get::<AssistantCall>().is_some()))
    }
}

/// Fournisseur de modèles configuré, ou `None` sans clé OpenRouter (assistant désactivé).
pub fn provider_from_config(config: &Config) -> anyhow::Result<Option<Arc<dyn LlmProvider>>> {
    let Some(ai) = &config.ai else {
        return Ok(None);
    };
    let provider = openrouter::OpenRouter::new(ai, &config.public_origin)?;
    Ok(Some(Arc::new(provider)))
}
