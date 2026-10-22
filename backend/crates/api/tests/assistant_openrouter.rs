//! Client OpenRouter contre un faux serveur local : en-têtes, statuts d'erreur et flux
//! découpé en petits morceaux, sans réseau.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use api::assistant::llm::{collect, ChatRequest, FinishReason, LlmError, LlmProvider, Message};
use api::assistant::openrouter::OpenRouter;
use api::config::{AiConfig, Secret};

const ORIGIN: &str = "http://localhost:5173";

/// Requête reçue par le faux serveur : `(en-têtes, corps)`.
type Seen = Arc<Mutex<Option<(HeaderMap, Value)>>>;

struct Mock {
    status: StatusCode,
    body: &'static str,
    seen: Seen,
}

async fn handler(
    State(mock): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    *mock.seen.lock().unwrap() = Some((headers, body));
    if !mock.status.is_success() {
        return (mock.status, mock.body).into_response();
    }
    // Morceaux de 7 octets : les lignes SSE et les caractères UTF-8 sont coupés en route.
    let chunks: Vec<Result<Vec<u8>, std::io::Error>> = mock
        .body
        .as_bytes()
        .chunks(7)
        .map(|c| Ok(c.to_vec()))
        .collect();
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(futures_util::stream::iter(chunks)))
        .unwrap()
}

async fn serve(status: StatusCode, body: &'static str) -> (OpenRouter, Seen) {
    let seen = Seen::default();
    let mock = Arc::new(Mock {
        status,
        body,
        seen: seen.clone(),
    });
    let app = Router::new()
        .route("/api/v1/chat/completions", post(handler))
        .with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let ai = AiConfig {
        api_key: Secret::new("sk-or-test"),
        base_url: format!("http://{addr}/api/v1/"),
        model: "z-ai/glm-5.3-flash".into(),
        model_small: "z-ai/glm-5.3-flash".into(),
        models_allowed: vec![],
        providers_only: vec!["mistral/eu".into()],
        daily_budget_usd: 0.5,
        max_rounds: 8,
    };
    (OpenRouter::new(&ai, ORIGIN).unwrap(), seen)
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "z-ai/glm-5.3-flash".into(),
        messages: vec![Message::user("Où dormir près d'Arenal ?")],
        tools: vec![],
        max_tokens: 200,
        reasoning: None,
    }
}

const STREAM: &str = concat!(
    ": OPENROUTER PROCESSING\n\n",
    "data: {\"model\":\"z-ai/glm-5.3-flash-20260826\",\"choices\":[{\"delta\":{\"content\":\"À La \"}}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"Fortuna.\"},\"finish_reason\":\"stop\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":50,\"completion_tokens\":5,\"cost\":0.00001}}\n\n",
    "data: [DONE]\n\n",
);

#[tokio::test]
async fn streams_a_reply_with_attribution_headers() {
    let (llm, seen) = serve(StatusCode::OK, STREAM).await;

    let reply = collect(llm.complete(request()).await.unwrap())
        .await
        .unwrap();
    assert_eq!(reply.text, "À La Fortuna.");
    assert_eq!(reply.finish, FinishReason::Stop);
    assert_eq!(reply.model, "z-ai/glm-5.3-flash-20260826");
    assert_eq!(reply.usage.prompt_tokens, 50);
    assert_eq!(reply.usage.cost_usd, 0.00001);

    let (headers, body) = seen.lock().unwrap().take().unwrap();
    assert_eq!(headers["authorization"], "Bearer sk-or-test");
    assert_eq!(headers["http-referer"], ORIGIN);
    assert_eq!(headers["x-title"], "TravelGuide");
    assert_eq!(body["model"], "z-ai/glm-5.3-flash");
    assert_eq!(body["provider"]["data_collection"], "deny");
    assert_eq!(body["provider"]["only"], json!(["mistral/eu"]));
}

#[tokio::test]
async fn maps_error_statuses() {
    let (llm, _) = serve(StatusCode::TOO_MANY_REQUESTS, "{}").await;
    assert!(matches!(
        llm.complete(request()).await,
        Err(LlmError::RateLimited)
    ));

    let (llm, _) = serve(
        StatusCode::PAYMENT_REQUIRED,
        r#"{"error":{"code":402,"message":"Insufficient credits"}}"#,
    )
    .await;
    match llm.complete(request()).await {
        Err(LlmError::Rejected(message)) => assert_eq!(message, "Insufficient credits"),
        Err(e) => panic!("erreur inattendue : {e}"),
        Ok(_) => panic!("la requête aurait dû être refusée"),
    }

    let (llm, _) = serve(StatusCode::BAD_GATEWAY, "").await;
    assert!(matches!(
        llm.complete(request()).await,
        Err(LlmError::Unavailable(_))
    ));
}

#[test]
fn api_key_is_never_printed() {
    let secret = Secret::new("sk-or-v1-abcdef");
    assert_eq!(format!("{secret:?}"), "Secret(***)");
}
