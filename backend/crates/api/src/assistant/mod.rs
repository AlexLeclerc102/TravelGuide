//! Assistant IA d'un voyage (voir `docs/AI_ASSISTANT.md`).
//!
//! Incrément A0 : accès aux modèles de langage ([`llm`]) et client OpenRouter
//! ([`openrouter`]). Conversations, outils et boucle d'agent viennent ensuite.

pub mod llm;
pub mod openrouter;

use std::sync::Arc;

use crate::config::Config;
use llm::LlmProvider;

/// Fournisseur de modèles configuré, ou `None` sans clé OpenRouter (assistant désactivé).
pub fn provider_from_config(config: &Config) -> anyhow::Result<Option<Arc<dyn LlmProvider>>> {
    let Some(ai) = &config.ai else {
        return Ok(None);
    };
    let provider = openrouter::OpenRouter::new(ai, &config.public_origin)?;
    Ok(Some(Arc::new(provider)))
}
