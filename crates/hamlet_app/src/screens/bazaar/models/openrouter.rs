//! OpenRouter as the Models stall reads it: every model it offers, a
//! page at a time, and who serves one. It needs no key.

use serde_json::Value;

use crate::bazaar::models::{Source, Ware};

/// The address OpenRouter lists its models at.
const MODELS: &str = "https://openrouter.ai/api/v1/models";

/// The models asked for at once. A page of them is well within what
/// one fetch may bring, where the whole list is not.
pub(crate) const PAGE: usize = 100;

/// One provider that serves a ware through OpenRouter.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Serving {
    pub provider: String,
    /// The precision it runs the model at, as `fp8`, where told.
    pub quantization: Option<String>,
    /// The share of calls it answered over the last day, out of 100.
    pub uptime: Option<f64>,
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub context: Option<u64>,
}

/// The address of the page of models from `from` on.
pub(crate) fn page(from: usize) -> String {
    format!("{MODELS}?offset={from}&limit={PAGE}")
}

/// The address of who serves the model `id`.
pub(crate) fn servings(id: &str) -> String {
    format!("{MODELS}/{id}/endpoints")
}

/// Dollars for a million tokens, from OpenRouter's price of one
/// token. A price below nothing is one that varies by the call.
fn per_million(v: &Value) -> Option<f64> {
    let each = v
        .as_f64()
        .or_else(|| v.as_str()?.trim().parse::<f64>().ok())?;
    // To a millionth of a dollar, past which a product of floats has
    // only noise.
    (each >= 0.0).then_some((each * 1e12).round() / 1e6)
}

/// The wares in OpenRouter's answer to a request for its models.
pub(crate) fn read(body: &Value) -> Vec<Ware> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            let params = m["supported_parameters"].as_array();
            let can = |what: &str| {
                params.map(|all| all.iter().any(|p| p == what))
            };
            Some(Ware {
                source: Source::OpenRouter,
                title: m["name"].as_str().unwrap_or(&id).to_string(),
                id,
                description: m["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                created: m["created"].as_u64(),
                input: per_million(&m["pricing"]["prompt"]),
                output: per_million(&m["pricing"]["completion"]),
                context: m["context_length"].as_u64(),
                inputs: m["architecture"]["input_modalities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                tools: can("tools"),
                reasoning: can("reasoning"),
                intelligence: m["benchmarks"]["artificial_analysis"]
                    ["intelligence_index"]
                    .as_f64(),
            })
        })
        .collect()
}

/// The providers in OpenRouter's answer to a request for a model's
/// endpoints.
pub(crate) fn read_servings(body: &Value) -> Vec<Serving> {
    body["data"]["endpoints"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Serving {
            provider: e["provider_name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            quantization: e["quantization"]
                .as_str()
                .filter(|q| *q != "unknown")
                .map(str::to_string),
            uptime: e["uptime_last_1d"].as_f64(),
            input: per_million(&e["pricing"]["prompt"]),
            output: per_million(&e["pricing"]["completion"]),
            context: e["context_length"].as_u64(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn openrouter_s_answer_is_read_into_wares() {
        let body = json!({"data": [
            {
                "id": "anthropic/claude-haiku-5.5",
                "name": "Anthropic: Claude Haiku 5.5",
                "created": 1791397883u64,
                "description": "Small and fast.",
                "context_length": 1000000,
                "architecture": {
                    "input_modalities": ["text", "image", "file"]
                },
                "pricing": {"prompt": "0.0000001", "completion": "0.0000005"},
                "supported_parameters": ["reasoning", "tools"],
                "benchmarks": {"artificial_analysis":
                    {"intelligence_index": 43.4}}
            },
            {
                "id": "someone/chatty",
                "pricing": {"prompt": "0", "completion": "0"},
                "supported_parameters": ["max_tokens"]
            },
            {
                "id": "openrouter/auto",
                "pricing": {"prompt": "-1", "completion": "-1"}
            },
            {"name": "no id"}
        ]});
        let wares = read(&body);
        assert_eq!(wares.len(), 3);
        let haiku = &wares[0];
        assert_eq!(haiku.title, "Anthropic: Claude Haiku 5.5");
        assert_eq!(
            (haiku.tools, haiku.reasoning),
            (Some(true), Some(true))
        );
        assert!(haiku.sees());
        assert_eq!(haiku.intelligence, Some(43.4));
        assert!((haiku.input.unwrap() - 0.1).abs() < 1e-9);
        assert!((haiku.output.unwrap() - 0.5).abs() < 1e-9);
        // Free is a price; one that varies by the call is none.
        let chatty = &wares[1];
        assert_eq!(chatty.title, "someone/chatty");
        assert_eq!(chatty.input, Some(0.0));
        assert_eq!(chatty.tools, Some(false));
        assert_eq!(wares[2].input, None);
        assert_eq!(wares[2].tools, None);
    }

    #[test]
    fn a_model_s_providers_are_read_with_their_uptime() {
        let body = json!({"data": {"endpoints": [
            {"provider_name": "Google", "quantization": "unknown",
             "uptime_last_1d": 99.5,
             "pricing": {"prompt": "0.000001", "completion": "0.000002"},
             "context_length": 1000},
            {"provider_name": "Small", "quantization": "fp8"}
        ]}});
        let servings = read_servings(&body);
        assert_eq!(servings[0].quantization, None);
        assert_eq!(servings[0].uptime, Some(99.5));
        assert!((servings[0].input.unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(servings[1].quantization.as_deref(), Some("fp8"));
    }
}
