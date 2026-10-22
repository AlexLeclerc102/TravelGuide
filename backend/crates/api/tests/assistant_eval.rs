//! Évaluation de l'assistant avec de vrais modèles (OpenRouter, payant) : lancée à la main.
//!
//! ```bash
//! OPENROUTER_API_KEY=sk-or-… cargo test -p api --test assistant_eval -- --ignored --nocapture
//! # Modèles comparés (sinon AI_MODEL puis AI_MODELS_ALLOWED) :
//! AI_EVAL_MODELS=z-ai/glm-5.3-flash,z-ai/glm-5.3 cargo test …
//! ```
//!
//! Incrément A0 : squelette et premier scénario (un appel d'outil). Les scénarios sur un
//! voyage réel (base de test, outils de l'API) arrivent avec la boucle d'agent.

use std::time::Instant;

use serde_json::{json, Value};

use api::assistant::llm::{
    collect, ChatRequest, Completion, Effort, LlmProvider, Message, ToolDef,
};
use api::assistant::openrouter::OpenRouter;
use api::config::AiConfig;

struct Scenario {
    name: &'static str,
    prompt: &'static str,
    tools: fn() -> Vec<ToolDef>,
    /// `Err` : raison de l'échec.
    check: fn(&Completion) -> Result<(), String>,
}

fn add_poi_tool() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "add_poi".into(),
        description: "Ajoute un lieu au voyage.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "category": {
                    "type": "string",
                    "enum": ["nature", "culture", "food", "activity", "lodging", "other"],
                },
            },
            "required": ["name", "category"],
        }),
    }]
}

fn scenarios() -> Vec<Scenario> {
    vec![Scenario {
        name: "appel d'outil simple",
        prompt: "Ajoute le volcan Arenal à nos lieux.",
        tools: add_poi_tool,
        check: |c| {
            let call = c
                .tool_calls
                .iter()
                .find(|t| t.name == "add_poi")
                .ok_or("aucun appel à add_poi")?;
            let args: Value = serde_json::from_str(&call.arguments)
                .map_err(|e| format!("arguments invalides : {e}"))?;
            if !args["name"].as_str().unwrap_or("").contains("Arenal") {
                return Err(format!("nom inattendu : {}", args["name"]));
            }
            if args["category"] != "nature" {
                return Err(format!("catégorie inattendue : {}", args["category"]));
            }
            Ok(())
        },
    }]
}

const SYSTEM: &str = "Tu es l'assistant d'un voyage de groupe. Utilise les outils pour agir. \
    Réponds brièvement en français.";

#[tokio::test]
#[ignore = "appelle OpenRouter (clé et crédit requis)"]
async fn compare_models() {
    // `.env` est écrit pour docker compose : une ligne illisible pour dotenvy (valeur avec
    // espaces non citée) ne doit pas empêcher de lire les suivantes.
    for (key, value) in dotenvy::from_filename_iter("../../../.env")
        .into_iter()
        .flatten()
        .flatten()
    {
        if std::env::var_os(&key).is_none() {
            std::env::set_var(key, value);
        }
    }
    let ai = AiConfig::from_env()
        .expect("configuration IA invalide")
        .expect("OPENROUTER_API_KEY manquante");
    let models: Vec<String> = match std::env::var("AI_EVAL_MODELS") {
        Ok(list) => list.split(',').map(|m| m.trim().to_owned()).collect(),
        Err(_) => std::iter::once(ai.model.clone())
            .chain(ai.models_allowed.iter().cloned())
            .collect(),
    };
    let llm = OpenRouter::new(&ai, "http://localhost:5173").unwrap();

    let mut failures = 0;
    println!(
        "{:<28} {:<24} {:>5} {:>7} {:>7} {:>6} {:>10}  résultat",
        "modèle", "scénario", "ms", "entrée", "cache", "sortie", "coût $"
    );
    for model in &models {
        for scenario in scenarios() {
            let req = ChatRequest {
                model: model.clone(),
                messages: vec![Message::system(SYSTEM), Message::user(scenario.prompt)],
                tools: (scenario.tools)(),
                max_tokens: 800,
                reasoning: Some(Effort::Low),
            };
            let started = Instant::now();
            let outcome = match llm.complete(req).await {
                Ok(stream) => collect(stream).await.map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            let ms = started.elapsed().as_millis();
            match outcome {
                Ok(c) => {
                    let result = (scenario.check)(&c);
                    failures += usize::from(result.is_err());
                    println!(
                        "{:<28} {:<24} {:>5} {:>7} {:>7} {:>6} {:>10.6}  {}",
                        c.model,
                        scenario.name,
                        ms,
                        c.usage.prompt_tokens,
                        c.usage.cached_tokens,
                        c.usage.completion_tokens,
                        c.usage.cost_usd,
                        result.err().unwrap_or_else(|| "ok".into()),
                    );
                }
                Err(e) => {
                    failures += 1;
                    println!("{model:<28} {:<24} {ms:>5}  erreur : {e}", scenario.name);
                }
            }
        }
    }
    assert_eq!(failures, 0, "{failures} scénario(s) en échec");
}
