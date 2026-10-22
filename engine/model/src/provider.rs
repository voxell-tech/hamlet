//! The seam to a model: what a provider is asked and what it answers.

mod openai;
mod replay;
mod responses;
mod stream;

use anyhow::Result;
use futures::future::BoxFuture;
use serde_json::Value;

pub use crate::provider::openai::{Endpoints, OpenAiCompat};
pub use crate::provider::replay::Replay;
pub use crate::provider::responses::ChatGpt;

/// A piece of streamed output.
pub enum Delta<'a> {
    Text(&'a str),
    Reasoning(&'a str),
}

/// Usage of one model call. A count the provider left out is `None`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallRecord {
    pub model: String,
    /// Host of the endpoint.
    pub endpoint: String,
    pub input_tokens: Option<u64>,
    /// The part of `input_tokens` served from the provider's cache.
    pub cached_input_tokens: Option<u64>,
    /// Includes reasoning tokens where the provider counts them as
    /// output.
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    /// The provider's own charge, in its account currency.
    pub billed_cost: Option<f64>,
    /// The provider's usage object, unchanged.
    pub usage_raw: Option<Value>,
    pub first_token_ms: Option<u64>,
    pub duration_ms: u64,
    pub finish_reason: Option<String>,
}

impl CallRecord {
    /// Fills the counts from a usage object in any of the shapes
    /// providers use, of the chat protocol or of the Responses API.
    fn read_usage(&mut self, usage: &Value) {
        let n = |v: &Value| v.as_u64();
        self.input_tokens = n(&usage["prompt_tokens"])
            .or_else(|| n(&usage["input_tokens"]));
        self.output_tokens = n(&usage["completion_tokens"])
            .or_else(|| n(&usage["output_tokens"]));
        self.cached_input_tokens =
            n(&usage["prompt_tokens_details"]["cached_tokens"])
                .or_else(|| n(&usage["prompt_cache_hit_tokens"]))
                .or_else(|| {
                    n(&usage["input_tokens_details"]["cached_tokens"])
                });
        self.reasoning_tokens =
            n(&usage["completion_tokens_details"]
                ["reasoning_tokens"])
            .or_else(|| {
                n(&usage["output_tokens_details"]["reasoning_tokens"])
            });
        self.billed_cost = usage["cost"].as_f64();
        self.usage_raw = Some(usage.clone());
    }
}

/// A model's reply and what the call used.
#[derive(Debug)]
pub struct Completion {
    /// The assistant message exactly as it should be appended to
    /// history.
    pub message: Value,
    pub call: CallRecord,
}

/// Something that completes a chat.
pub trait Provider: Send + Sync {
    /// Streams one completion, calling `on_delta` as text arrives.
    /// `model` names the model to run; `None` is the one that runs
    /// where nobody chose.
    fn complete<'a>(
        &'a self,
        messages: &'a [Value],
        tools: &'a [Value],
        model: Option<&'a str>,
        on_delta: &'a mut (dyn FnMut(Delta<'_>) + Send),
    ) -> BoxFuture<'a, Result<Completion>>;
}
