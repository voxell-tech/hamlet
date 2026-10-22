//! A session's use of the model.

use bevy_ecs::prelude::*;
use hamlet_model::log;
use hamlet_model::provider::CallRecord;
use hamlet_rules::budget::{
    format_cost, format_duration, format_tokens,
};
use serde_json::Value;

/// A session's use of the model so far. Counts cover only the calls
/// whose provider reported them.
#[derive(Component, Default, Clone, Debug, PartialEq)]
pub struct Usage {
    pub calls: u64,
    /// Calls whose provider reported no tokens.
    pub unreported: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    /// Time spent waiting on the model, in milliseconds.
    pub model_ms: u64,
    /// The sum of what providers said they charged, where they said.
    pub billed_cost: f64,
    pub model: String,
    /// Input tokens of the latest call: how full the context is.
    pub last_input_tokens: u64,
}

impl Usage {
    /// Input and output tokens together.
    pub fn tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        model: &str,
        input: Option<u64>,
        cached: Option<u64>,
        output: Option<u64>,
        reasoning: Option<u64>,
        cost: Option<f64>,
        ms: u64,
    ) {
        self.calls += 1;
        if input.is_none() && output.is_none() {
            self.unreported += 1;
        }
        self.input_tokens += input.unwrap_or(0);
        self.cached_input_tokens += cached.unwrap_or(0);
        self.output_tokens += output.unwrap_or(0);
        self.reasoning_tokens += reasoning.unwrap_or(0);
        self.billed_cost += cost.unwrap_or(0.0);
        self.model_ms += ms;
        self.model = model.to_string();
        if let Some(i) = input {
            self.last_input_tokens = i;
        }
    }

    /// One line for a status bar. A local model has no price, so its
    /// cost is shown as time, never as `$0.00`.
    pub fn summary(&self) -> String {
        if self.calls == 0 {
            return "No model calls yet".into();
        }
        let tokens = if self.unreported == self.calls {
            "Tokens not reported".to_string()
        } else {
            format_tokens(self.tokens())
        };
        let cost = if self.billed_cost > 0.0 {
            format_cost(self.billed_cost)
        } else {
            format!("{} model time", format_duration(self.model_ms))
        };
        format!("{tokens}, {cost}")
    }

    pub(super) fn record(&mut self, c: &CallRecord) {
        self.add(
            &c.model,
            c.input_tokens,
            c.cached_input_tokens,
            c.output_tokens,
            c.reasoning_tokens,
            c.billed_cost,
            c.duration_ms,
        );
    }

    /// Totals for a resumed session, from every `model_call` event
    /// in its log: a call a rewind took out was paid for all the
    /// same.
    pub fn from_log(events: &[Value]) -> Self {
        let mut u = Usage::default();
        for e in events.iter().filter(|e| e["type"] == "model_call") {
            u.add(
                e["model"].as_str().unwrap_or_default(),
                e["input_tokens"].as_u64(),
                e["cached_input_tokens"].as_u64(),
                e["output_tokens"].as_u64(),
                e["reasoning_tokens"].as_u64(),
                e["billed_cost"].as_f64(),
                e["duration_ms"].as_u64().unwrap_or(0),
            );
        }
        u.last_input_tokens = carried(events);
        u
    }
}

/// The input tokens of the latest model call in `events` that
/// stands: the size of the conversation as it is after its rewinds.
pub(super) fn carried(events: &[Value]) -> u64 {
    let out = log::taken_out(events);
    events
        .iter()
        .zip(out)
        .rev()
        .filter(|(event, out)| !out && event["type"] == "model_call")
        .find_map(|(event, _)| event["input_tokens"].as_u64())
        .unwrap_or(0)
}
