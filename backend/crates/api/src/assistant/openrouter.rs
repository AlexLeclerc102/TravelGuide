//! Client OpenRouter : API `chat/completions` compatible OpenAI, réponse en flux SSE.
//!
//! <https://openrouter.ai/docs/api-reference/chat-completion>
//!
//! Chaque requête interdit la collecte des données par l'hébergeur
//! (`provider.data_collection = "deny"`), peut restreindre les hébergeurs à ceux couverts
//! par un accord de traitement (`provider.only`), et demande le coût exact de l'appel.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::StatusCode;
use serde_json::{json, Value};

use super::llm::{
    ChatEvent, ChatRequest, ChatStream, FinishReason, LlmError, LlmProvider, Message, Role,
    ToolCall, Usage,
};
use crate::config::{AiConfig, Secret};

pub struct OpenRouter {
    http: reqwest::Client,
    base_url: String,
    api_key: Secret,
    providers_only: Vec<String>,
}

impl OpenRouter {
    /// `public_origin` identifie l'application auprès d'OpenRouter (`HTTP-Referer`).
    pub fn new(ai: &AiConfig, public_origin: &str) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "HTTP-Referer",
            HeaderValue::from_str(public_origin).context("PUBLIC_ORIGIN invalide")?,
        );
        headers.insert("X-Title", HeaderValue::from_static("TravelGuide"));
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(5))
            // Délai entre deux morceaux du flux : le modèle peut réfléchir avant d'écrire.
            .read_timeout(Duration::from_secs(90))
            .build()
            .context("client HTTP OpenRouter")?;
        Ok(Self {
            http,
            base_url: ai.base_url.trim_end_matches('/').to_owned(),
            api_key: ai.api_key.clone(),
            providers_only: ai.providers_only.clone(),
        })
    }
}

#[async_trait]
impl LlmProvider for OpenRouter {
    async fn complete(&self, req: ChatRequest) -> Result<ChatStream, LlmError> {
        let res = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(self.api_key.expose())
            .json(&request_body(&req, &self.providers_only))
            .send()
            .await
            .map_err(|e| LlmError::Unavailable(e.into()))?;
        let status = res.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(LlmError::RateLimited);
        }
        if status.is_client_error() {
            return Err(LlmError::Rejected(error_message(res).await));
        }
        if !status.is_success() {
            let message = error_message(res).await;
            return Err(LlmError::Unavailable(anyhow::anyhow!(
                "OpenRouter : {message}"
            )));
        }

        let state = (
            res.bytes_stream().boxed(),
            Decoder::default(),
            VecDeque::new(),
        );
        let stream = futures_util::stream::unfold(
            state,
            |(mut bytes, mut decoder, mut pending)| async move {
                loop {
                    if let Some(event) = pending.pop_front() {
                        return Some((event, (bytes, decoder, pending)));
                    }
                    if decoder.is_done() {
                        return None;
                    }
                    match bytes.next().await {
                        Some(Ok(chunk)) => pending.extend(decoder.feed(&chunk)),
                        Some(Err(e)) => {
                            pending.extend(decoder.fail(LlmError::Unavailable(e.into())))
                        }
                        None => pending.extend(decoder.eof()),
                    }
                }
            },
        );
        Ok(Box::pin(stream))
    }
}

/// Message d'erreur renvoyé par OpenRouter (`{"error": {"message": …}}`), ou le statut HTTP.
async fn error_message(res: reqwest::Response) -> String {
    let status = res.status();
    let body: Value = res.json().await.unwrap_or(Value::Null);
    body["error"]["message"]
        .as_str()
        .map(|m| m.chars().take(300).collect())
        .unwrap_or_else(|| status.to_string())
}

/// Corps JSON d'une requête `chat/completions`.
pub fn request_body(req: &ChatRequest, providers_only: &[String]) -> Value {
    let mut provider = json!({ "data_collection": "deny", "require_parameters": true });
    if !providers_only.is_empty() {
        provider["only"] = json!(providers_only);
    }
    let mut body = json!({
        "model": req.model,
        "messages": req.messages.iter().map(message_json).collect::<Vec<_>>(),
        "max_tokens": req.max_tokens,
        "stream": true,
        "usage": { "include": true },
        "provider": provider,
    });
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    },
                })
            })
            .collect();
    }
    if let Some(effort) = req.reasoning {
        body["reasoning"] = json!({ "effort": effort.as_str() });
    }
    body
}

fn message_json(m: &Message) -> Value {
    match m.role {
        Role::System => json!({ "role": "system", "content": m.content }),
        Role::User => json!({ "role": "user", "content": m.content }),
        Role::Tool => json!({
            "role": "tool",
            "tool_call_id": m.tool_call_id,
            "content": m.content,
        }),
        Role::Assistant if m.tool_calls.is_empty() => {
            json!({ "role": "assistant", "content": m.content })
        }
        Role::Assistant => json!({
            "role": "assistant",
            "content": if m.content.is_empty() { Value::Null } else { json!(m.content) },
            "tool_calls": m.tool_calls.iter().map(|c| json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments },
            })).collect::<Vec<_>>(),
        }),
    }
}

/// Découpe un flux Server-Sent Events en données d'événements (champs `data:`).
#[derive(Debug, Default)]
struct SseDecoder {
    buf: Vec<u8>,
    data: Option<String>,
}

impl SseDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = vec![];
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            // Un caractère UTF-8 multi-octets ne contient jamais d'octet `\n` : la coupure
            // ne tombe jamais au milieu d'un caractère.
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            self.line(line.trim_end_matches(['\n', '\r']), &mut out);
        }
        out
    }

    fn eof(&mut self) -> Vec<String> {
        let mut out = vec![];
        if !self.buf.is_empty() {
            let line = String::from_utf8_lossy(&std::mem::take(&mut self.buf)).into_owned();
            self.line(line.trim_end_matches('\r'), &mut out);
        }
        self.line("", &mut out);
        out
    }

    fn line(&mut self, line: &str, out: &mut Vec<String>) {
        if line.is_empty() {
            out.extend(self.data.take());
        } else if let Some(value) = line.strip_prefix("data:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            match &mut self.data {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => self.data = Some(value.to_owned()),
            }
        }
        // Commentaires (`: OPENROUTER PROCESSING`) et autres champs : ignorés.
    }
}

/// Appel d'outil en cours d'assemblage (les arguments arrivent par morceaux).
#[derive(Debug, Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// Transforme les morceaux d'une réponse OpenRouter en [`ChatEvent`].
#[derive(Debug, Default)]
struct Decoder {
    sse: SseDecoder,
    calls: BTreeMap<u64, PartialCall>,
    finish: Option<FinishReason>,
    usage: Usage,
    model: String,
    done: bool,
}

type Events = Vec<Result<ChatEvent, LlmError>>;

impl Decoder {
    fn is_done(&self) -> bool {
        self.done
    }

    fn feed(&mut self, bytes: &[u8]) -> Events {
        let mut out = vec![];
        for data in self.sse.feed(bytes) {
            self.data(&data, &mut out);
        }
        out
    }

    /// Fin du flux HTTP, avec ou sans `[DONE]`.
    fn eof(&mut self) -> Events {
        let mut out = vec![];
        for data in self.sse.eof() {
            self.data(&data, &mut out);
        }
        if !self.done {
            self.end(&mut out);
        }
        out
    }

    fn fail(&mut self, err: LlmError) -> Events {
        self.done = true;
        vec![Err(err)]
    }

    fn data(&mut self, data: &str, out: &mut Events) {
        if self.done {
            return;
        }
        if data == "[DONE]" {
            return self.end(out);
        }
        let chunk: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                return out.extend(self.fail(LlmError::Unavailable(
                    anyhow::Error::new(e).context("morceau de réponse illisible"),
                )))
            }
        };
        // Erreur survenue après le début du flux (statut HTTP déjà envoyé).
        if let Some(err) = chunk.get("error") {
            let message = err["message"].as_str().unwrap_or("erreur inconnue");
            let err = if err["code"].as_u64() == Some(429) {
                LlmError::RateLimited
            } else {
                LlmError::Unavailable(anyhow::anyhow!("OpenRouter : {message}"))
            };
            return out.extend(self.fail(err));
        }
        if let Some(model) = chunk["model"].as_str() {
            model.clone_into(&mut self.model);
        }
        if chunk["usage"].is_object() {
            self.usage = usage(&chunk["usage"]);
        }
        let Some(choice) = chunk["choices"].get(0) else {
            return;
        };
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            out.push(Ok(ChatEvent::Text(text.to_owned())));
        }
        for (position, call) in delta["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let index = call["index"].as_u64().unwrap_or(position as u64);
            let partial = self.calls.entry(index).or_default();
            if let Some(id) = call["id"].as_str().filter(|s| !s.is_empty()) {
                id.clone_into(&mut partial.id);
            }
            if let Some(name) = call["function"]["name"]
                .as_str()
                .filter(|_| partial.name.is_empty())
            {
                name.clone_into(&mut partial.name);
            }
            if let Some(args) = call["function"]["arguments"].as_str() {
                partial.arguments.push_str(args);
            }
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            self.finish = Some(match reason {
                "stop" => FinishReason::Stop,
                "tool_calls" => FinishReason::ToolCalls,
                "length" => FinishReason::Length,
                _ => FinishReason::Other,
            });
        }
    }

    fn end(&mut self, out: &mut Events) {
        self.done = true;
        let Some(mut finish) = self.finish else {
            return out.push(Err(LlmError::Unavailable(anyhow::anyhow!(
                "flux interrompu avant la fin de la réponse"
            ))));
        };
        // Certains hébergeurs terminent par `stop` malgré des appels d'outils.
        if !self.calls.is_empty() && finish == FinishReason::Stop {
            finish = FinishReason::ToolCalls;
        }
        for (index, call) in std::mem::take(&mut self.calls) {
            out.push(Ok(ChatEvent::ToolCall(ToolCall {
                id: if call.id.is_empty() {
                    format!("call_{index}")
                } else {
                    call.id
                },
                name: call.name,
                arguments: call.arguments,
            })));
        }
        out.push(Ok(ChatEvent::Done {
            finish,
            usage: std::mem::take(&mut self.usage),
            model: std::mem::take(&mut self.model),
        }));
    }
}

fn usage(u: &Value) -> Usage {
    let tokens = |v: &Value| v.as_u64().unwrap_or(0).try_into().unwrap_or(u32::MAX);
    Usage {
        prompt_tokens: tokens(&u["prompt_tokens"]),
        completion_tokens: tokens(&u["completion_tokens"]),
        cached_tokens: tokens(&u["prompt_tokens_details"]["cached_tokens"]),
        cost_usd: u["cost"].as_f64().unwrap_or(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::super::llm::{Effort, ToolDef};
    use super::*;

    fn events(decoder: &mut Decoder, chunks: &[&str]) -> Vec<ChatEvent> {
        let mut out: Events = chunks
            .iter()
            .flat_map(|c| decoder.feed(c.as_bytes()))
            .collect();
        out.extend(decoder.eof());
        out.into_iter()
            .map(|e| e.expect("événement valide"))
            .collect()
    }

    #[test]
    fn builds_request_body() {
        let req = ChatRequest {
            model: "z-ai/glm-5.2".into(),
            messages: vec![
                Message::system("Tu es un assistant."),
                Message::user("Ajoute Arenal"),
                Message::assistant(
                    "",
                    vec![ToolCall {
                        id: "c1".into(),
                        name: "add_poi".into(),
                        arguments: r#"{"name":"Arenal"}"#.into(),
                    }],
                ),
                Message::tool("c1", "ok P1"),
            ],
            tools: vec![ToolDef {
                name: "add_poi".into(),
                description: "Ajoute un lieu".into(),
                parameters: json!({"type": "object"}),
            }],
            max_tokens: 800,
            reasoning: Some(Effort::Low),
        };
        let body = request_body(&req, &["mistral/eu".to_owned()]);

        assert_eq!(body["stream"], true);
        assert_eq!(body["usage"]["include"], true);
        assert_eq!(body["max_tokens"], 800);
        assert_eq!(body["reasoning"]["effort"], "low");
        assert_eq!(body["provider"]["data_collection"], "deny");
        assert_eq!(body["provider"]["require_parameters"], true);
        assert_eq!(body["provider"]["only"], json!(["mistral/eu"]));
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "add_poi");

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[2]["content"], Value::Null);
        assert_eq!(messages[2]["tool_calls"][0]["id"], "c1");
        assert_eq!(
            messages[2]["tool_calls"][0]["function"]["arguments"],
            r#"{"name":"Arenal"}"#
        );
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "c1");
    }

    #[test]
    fn omits_optional_fields() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![Message::user("x")],
            tools: vec![],
            max_tokens: 10,
            reasoning: None,
        };
        let body = request_body(&req, &[]);
        assert!(body.get("tools").is_none());
        assert!(body.get("reasoning").is_none());
        assert!(body["provider"].get("only").is_none());
    }

    #[test]
    fn sse_handles_split_lines_comments_and_crlf() {
        let mut sse = SseDecoder::default();
        assert!(sse.feed(b": OPENROUTER PROCESSING\r\n\r\nda").is_empty());
        assert!(sse.feed("ta: {\"a\":\"é\"}\r\n".as_bytes()).is_empty());
        assert_eq!(
            sse.feed(b"\r\ndata: x\ndata: y\n\n"),
            vec!["{\"a\":\"é\"}".to_owned(), "x\ny".to_owned(),]
        );
        // Dernier événement sans ligne vide finale.
        assert!(sse.feed(b"data: [DONE]").is_empty());
        assert_eq!(sse.eof(), vec!["[DONE]".to_owned()]);
    }

    #[test]
    fn assembles_text_tool_calls_and_usage() {
        let mut decoder = Decoder::default();
        let out = events(&mut decoder, &[
            "data: {\"model\":\"z-ai/glm-5.2-20260616\",\"choices\":[{\"delta\":{\"content\":\"Je \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"cherche.\"}}]}\n\n",
            // Deux appels parallèles, arguments découpés et entrelacés.
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"add_poi\",\"arguments\":\"{\\\"na\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"b\",\"function\":{\"name\":\"route\",\"arguments\":\"{}\"}}]}}]}\n",
            "\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"me\\\":\\\"Arenal\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            ": commentaire\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1200,\"completion_tokens\":40,\"prompt_tokens_details\":{\"cached_tokens\":900},\"cost\":0.00042}}\n\n",
            "data: [DONE]\n\n",
        ]);

        assert_eq!(
            out,
            vec![
                ChatEvent::Text("Je ".into()),
                ChatEvent::Text("cherche.".into()),
                ChatEvent::ToolCall(ToolCall {
                    id: "a".into(),
                    name: "add_poi".into(),
                    arguments: r#"{"name":"Arenal"}"#.into(),
                }),
                ChatEvent::ToolCall(ToolCall {
                    id: "b".into(),
                    name: "route".into(),
                    arguments: "{}".into(),
                }),
                ChatEvent::Done {
                    finish: FinishReason::ToolCalls,
                    usage: Usage {
                        prompt_tokens: 1200,
                        completion_tokens: 40,
                        cached_tokens: 900,
                        cost_usd: 0.00042,
                    },
                    model: "z-ai/glm-5.2-20260616".into(),
                },
            ]
        );
        assert!(decoder.is_done());
    }

    #[test]
    fn tool_calls_with_stop_and_without_id_are_normalised() {
        let mut decoder = Decoder::default();
        let out = events(&mut decoder, &[
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"name\":\"x\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"stop\"}]}\n\n",
        ]);
        assert_eq!(
            out[0],
            ChatEvent::ToolCall(ToolCall {
                id: "call_0".into(),
                name: "x".into(),
                arguments: "{}".into(),
            })
        );
        assert!(matches!(
            out[1],
            ChatEvent::Done {
                finish: FinishReason::ToolCalls,
                ..
            }
        ));
    }

    #[test]
    fn eof_after_finish_without_done_marker_is_a_success() {
        let mut decoder = Decoder::default();
        let out = events(&mut decoder, &[
            "data: {\"choices\":[{\"delta\":{\"content\":\"Oui\"},\"finish_reason\":\"stop\"}]}\n\n",
        ]);
        assert!(matches!(
            out.last(),
            Some(ChatEvent::Done {
                finish: FinishReason::Stop,
                ..
            })
        ));
    }

    #[test]
    fn truncated_stream_is_an_error() {
        let mut decoder = Decoder::default();
        let mut out = decoder.feed(b"data: {\"choices\":[{\"delta\":{\"content\":\"Oui\"}}]}\n\n");
        out.extend(decoder.eof());
        assert!(matches!(out[0], Ok(ChatEvent::Text(_))));
        assert!(matches!(out[1], Err(LlmError::Unavailable(_))));
    }

    #[test]
    fn mid_stream_errors_stop_the_stream() {
        let mut decoder = Decoder::default();
        let out = decoder.feed(
            b"data: {\"error\":{\"code\":429,\"message\":\"slow down\"}}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
        );
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Err(LlmError::RateLimited)));
        assert!(decoder.is_done());
        assert!(decoder.eof().is_empty());

        let mut decoder = Decoder::default();
        let out = decoder.feed(b"data: {\"error\":{\"code\":502,\"message\":\"upstream\"}}\n\n");
        assert!(matches!(out[0], Err(LlmError::Unavailable(_))));
    }

    #[test]
    fn invalid_json_is_an_error() {
        let mut decoder = Decoder::default();
        let out = decoder.feed(b"data: {pas du json\n\n");
        assert!(matches!(out[0], Err(LlmError::Unavailable(_))));
    }
}
