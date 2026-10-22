//! Modèles de langage, derrière le trait [`LlmProvider`] (même schéma que
//! `RoutingProvider` et `PlaceProvider`).
//!
//! Le fournisseur réel est OpenRouter ([`super::openrouter::OpenRouter`]) ; les tests
//! utilisent [`ScriptedLlm`], qui rejoue des tours prédéfinis sans réseau.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Mutex;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Message d'une conversation, au format des API compatibles OpenAI.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Appels d'outils demandés par le modèle (rôle `Assistant`).
    pub tool_calls: Vec<ToolCall>,
    /// Appel auquel ce message répond (rôle `Tool`).
    pub tool_call_id: Option<String>,
}

impl Message {
    fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }

    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            tool_calls,
            ..Self::new(Role::Assistant, content)
        }
    }

    pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            tool_call_id: Some(call_id.into()),
            ..Self::new(Role::Tool, content)
        }
    }
}

/// Appel d'outil demandé par le modèle.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Arguments JSON tels qu'émis par le modèle : à valider, ils peuvent être invalides.
    pub arguments: String,
}

/// Outil proposé au modèle (`parameters` : schéma JSON).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Effort de raisonnement demandé au modèle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
    /// Absent : réglage par défaut du modèle.
    pub reasoning: Option<Effort>,
}

/// Consommation d'un appel, telle que facturée par le fournisseur.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Part de `prompt_tokens` lue depuis le cache du fournisseur.
    pub cached_tokens: u32,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FinishReason {
    #[default]
    Stop,
    ToolCalls,
    /// Coupé par `max_tokens`.
    Length,
    Other,
}

/// Événement d'une réponse en flux. Le texte arrive au fil de l'eau ; les appels d'outils,
/// une fois complets ; `Done` termine toujours un flux réussi.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    Text(String),
    ToolCall(ToolCall),
    Done {
        finish: FinishReason,
        usage: Usage,
        /// Modèle ayant réellement répondu (version datée chez OpenRouter).
        model: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("modèle indisponible")]
    Unavailable(#[source] anyhow::Error),
    #[error("trop de requêtes auprès du fournisseur du modèle")]
    RateLimited,
    /// Requête refusée (clé invalide, crédit épuisé, paramètre non pris en charge…).
    #[error("requête refusée par le fournisseur du modèle : {0}")]
    Rejected(String),
}

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatEvent, LlmError>> + Send>>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, req: ChatRequest) -> Result<ChatStream, LlmError>;
}

/// Réponse complète, assemblée à partir d'un flux.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish: FinishReason,
    pub usage: Usage,
    pub model: String,
}

/// Lit un flux jusqu'au bout (sous-appels sans affichage progressif, tests).
pub async fn collect(mut stream: ChatStream) -> Result<Completion, LlmError> {
    let mut out = Completion::default();
    while let Some(event) = stream.next().await {
        match event? {
            ChatEvent::Text(t) => out.text.push_str(&t),
            ChatEvent::ToolCall(c) => out.tool_calls.push(c),
            ChatEvent::Done {
                finish,
                usage,
                model,
            } => {
                out.finish = finish;
                out.usage = usage;
                out.model = model;
                return Ok(out);
            }
        }
    }
    Err(LlmError::Unavailable(anyhow::anyhow!(
        "flux interrompu avant la fin de la réponse"
    )))
}

/// Tour rejoué par [`ScriptedLlm`].
#[derive(Debug, Clone)]
pub enum Turn {
    Text(String),
    /// Appels d'outils `(nom, arguments)` ; les identifiants sont générés.
    Calls(Vec<(String, Value)>),
}

/// Faux modèle pour les tests : rejoue les tours dans l'ordre et garde les requêtes reçues.
#[derive(Debug, Default)]
pub struct ScriptedLlm {
    turns: Mutex<VecDeque<Turn>>,
    requests: Mutex<Vec<ChatRequest>>,
}

/// Consommation fictive de chaque tour rejoué.
pub const SCRIPTED_USAGE: Usage = Usage {
    prompt_tokens: 100,
    completion_tokens: 10,
    cached_tokens: 0,
    cost_usd: 0.0001,
};

impl ScriptedLlm {
    pub fn new(turns: impl IntoIterator<Item = Turn>) -> Self {
        Self {
            turns: Mutex::new(turns.into_iter().collect()),
            requests: Mutex::default(),
        }
    }

    /// Requêtes reçues, dans l'ordre.
    pub fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().expect("mutex empoisonné").clone()
    }

    /// Tours pas encore rejoués.
    pub fn remaining(&self) -> usize {
        self.turns.lock().expect("mutex empoisonné").len()
    }
}

#[async_trait]
impl LlmProvider for std::sync::Arc<ScriptedLlm> {
    async fn complete(&self, req: ChatRequest) -> Result<ChatStream, LlmError> {
        let round = {
            let mut requests = self.requests.lock().expect("mutex empoisonné");
            requests.push(req.clone());
            requests.len()
        };
        let turn = self
            .turns
            .lock()
            .expect("mutex empoisonné")
            .pop_front()
            .ok_or_else(|| LlmError::Unavailable(anyhow::anyhow!("script épuisé")))?;
        let (mut events, finish) = match turn {
            Turn::Text(text) => (vec![ChatEvent::Text(text)], FinishReason::Stop),
            Turn::Calls(calls) => (
                calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, args))| {
                        ChatEvent::ToolCall(ToolCall {
                            id: format!("call_{round}_{i}"),
                            name,
                            arguments: args.to_string(),
                        })
                    })
                    .collect(),
                FinishReason::ToolCalls,
            ),
        };
        events.push(ChatEvent::Done {
            finish,
            usage: SCRIPTED_USAGE,
            model: req.model,
        });
        Ok(Box::pin(futures_util::stream::iter(
            events.into_iter().map(Ok),
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;

    fn request() -> ChatRequest {
        ChatRequest {
            model: "test/model".into(),
            messages: vec![Message::user("Bonjour")],
            tools: vec![],
            max_tokens: 100,
            reasoning: None,
        }
    }

    #[tokio::test]
    async fn scripted_llm_replays_turns_in_order() {
        let llm = Arc::new(ScriptedLlm::new([
            Turn::Calls(vec![("add_poi".into(), json!({"name": "Arenal"}))]),
            Turn::Text("C'est fait.".into()),
        ]));

        let first = collect(llm.complete(request()).await.unwrap())
            .await
            .unwrap();
        assert_eq!(first.finish, FinishReason::ToolCalls);
        assert_eq!(first.tool_calls.len(), 1);
        assert_eq!(first.tool_calls[0].name, "add_poi");
        assert_eq!(first.tool_calls[0].arguments, r#"{"name":"Arenal"}"#);

        let second = collect(llm.complete(request()).await.unwrap())
            .await
            .unwrap();
        assert_eq!(second.text, "C'est fait.");
        assert_eq!(second.usage, SCRIPTED_USAGE);
        assert_eq!(second.model, "test/model");

        assert_eq!(llm.requests().len(), 2);
        assert_eq!(llm.remaining(), 0);
        assert!(matches!(
            llm.complete(request()).await,
            Err(LlmError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn collect_fails_on_truncated_stream() {
        let stream: ChatStream = Box::pin(futures_util::stream::iter([Ok(ChatEvent::Text(
            "début".into(),
        ))]));
        assert!(matches!(
            collect(stream).await,
            Err(LlmError::Unavailable(_))
        ));
    }
}
