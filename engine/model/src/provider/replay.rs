//! A provider that answers from a list, for tests and replays.

use std::collections::VecDeque;
use std::sync::Mutex;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use serde_json::Value;

use crate::provider::{CallRecord, Completion, Delta, Provider};

/// Returns canned responses in order.
pub struct Replay {
    responses: Mutex<VecDeque<Value>>,
}

impl Replay {
    pub fn new(responses: Vec<Value>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

impl Provider for Replay {
    fn complete<'a>(
        &'a self,
        _messages: &'a [Value],
        _tools: &'a [Value],
        model: Option<&'a str>,
        on_delta: &'a mut (dyn FnMut(Delta<'_>) + Send),
    ) -> BoxFuture<'a, Result<Completion>> {
        Box::pin(async move {
            let next = self
                .responses
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_front();
            let msg =
                next.ok_or_else(|| anyhow!("replay exhausted"))?;
            if let Some(text) =
                msg["content"].as_str().filter(|s| !s.is_empty())
            {
                on_delta(Delta::Text(text));
            }
            let call = CallRecord {
                model: model.unwrap_or("replay").into(),
                endpoint: "replay".into(),
                ..CallRecord::default()
            };
            Ok(Completion { message: msg, call })
        })
    }
}
