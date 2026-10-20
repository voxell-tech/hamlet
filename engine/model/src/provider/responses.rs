//! The client for a ChatGPT plan: the Responses API, called with the
//! access token of the chief's sign-in, as
//! <https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference>
//! asks.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use futures::StreamExt;
use serde_json::{Value, json};

use super::openai::{
    CONNECT_TIMEOUT, ERROR_SHOWN, Failed, RETRY_LONGEST, RETRY_WAITS,
};
use super::stream::LineSplitter;
use super::{CallRecord, Completion, Delta};
use crate::config::host;

/// The namespace the tools are offered in, which the plan asks for.
const NAMESPACE: &str = "hamlet";

/// Client for the models of a ChatGPT plan.
pub struct ChatGpt {
    http: reqwest::Client,
    base_url: String,
    /// The home the sign-in is kept in.
    home: PathBuf,
}

impl ChatGpt {
    pub fn new(base_url: String, home: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("reqwest client config is static");
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            home,
        }
    }

    pub(super) async fn run(
        &self,
        messages: &[Value],
        tools: &[Value],
        model: &str,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<Completion> {
        let body = request(model, messages, tools);
        let mut waits = RETRY_WAITS.iter();
        loop {
            let failed = match self.attempt(&body, on_delta).await {
                Ok(done) => return Ok(done),
                Err(failed) => failed,
            };
            let Some((asked, wait)) = failed.again.zip(waits.next())
            else {
                return Err(failed.error);
            };
            let wait = asked.unwrap_or(*wait).min(RETRY_LONGEST);
            tokio::time::sleep(wait).await;
        }
    }

    async fn attempt(
        &self,
        body: &Value,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<Completion, Failed> {
        let token = crate::chatgpt::access_token(&self.home)
            .await
            .map_err(Failed::last)?;
        let started = Instant::now();
        let resp = self
            .http
            .post(format!("{}/responses", self.base_url))
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .with_context(|| {
                format!("could not reach {}", self.base_url)
            })
            .map_err(Failed::passing)?;
        let status = resp.status();
        if !status.is_success() {
            let request = resp
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(|id| format!(", request {id}"))
                .unwrap_or_default();
            let text = resp.text().await.unwrap_or_default();
            let error =
                anyhow!("HTTP {status}{request}: {}", refused(&text));
            // A spent plan is not asked again; a busy server is.
            return Err(match status.is_server_error() {
                true => Failed::passing(error),
                false => Failed::last(error),
            });
        }
        let mut stream = resp.bytes_stream();
        let mut lines = LineSplitter::default();
        let mut acc = ResponseAcc::since(started);
        let mut done = false;
        let mut begun = false;
        let mut hear = |delta: Delta<'_>| {
            begun = true;
            on_delta(delta)
        };
        'outer: while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(e) if begun => {
                    return Err(Failed::last(e.into()));
                }
                Err(e) => return Err(Failed::passing(e.into())),
            };
            for line in lines.push(&chunk) {
                if acc
                    .feed_line(&line, &mut hear)
                    .map_err(Failed::last)?
                {
                    done = true;
                    break 'outer;
                }
            }
        }
        if !done && let Some(line) = lines.finish() {
            done = acc
                .feed_line(&line, &mut hear)
                .map_err(Failed::last)?;
        }
        if !done {
            let error = anyhow!(
                "the model's reply was cut off before it finished; \
                 send the message again"
            );
            return Err(match begun {
                true => Failed::last(error),
                false => Failed::passing(error),
            });
        }
        let mut call = CallRecord {
            model: body["model"].as_str().unwrap_or_default().into(),
            endpoint: host(&self.base_url).to_string(),
            first_token_ms: acc.first_token_ms,
            duration_ms: started.elapsed().as_millis() as u64,
            finish_reason: acc.finish_reason.take(),
            ..CallRecord::default()
        };
        if let Some(usage) = acc.usage.take() {
            call.read_usage(&usage);
        }
        Ok(Completion {
            message: acc.into_message(),
            call,
        })
    }
}

/// The words for an error of the plan, with what to do where there is
/// something to do.
fn plan_error(code: &str, message: &str) -> String {
    let todo = match code {
        "subscription_sharing_usage_limit_exceeded" => {
            " The usage of your ChatGPT plan, or its limit for Hamlet, \
             is spent for now. See https://chatgpt.com/settings/usage."
        }
        "subscription_sharing_user_not_eligible" => {
            " This ChatGPT account or workspace cannot lend its plan \
             to Hamlet."
        }
        "subscription_sharing_invalid_user" => {
            " Sign in with ChatGPT again."
        }
        _ => "",
    };
    format!("ChatGPT: {message} ({code}).{todo}")
}

/// The error in the body of a refused request, in whichever shape it
/// came.
fn refused(text: &str) -> String {
    let body =
        serde_json::from_str::<Value>(text).unwrap_or_default();
    let error = &body["error"];
    if let Some(message) = error["message"].as_str() {
        return plan_error(
            error["code"].as_str().unwrap_or("error"),
            message,
        );
    }
    if let Some(detail) = body["detail"].as_str() {
        return format!("ChatGPT: {detail}");
    }
    let shown = text.chars().take(ERROR_SHOWN).collect::<String>();
    format!("ChatGPT: {shown}")
}

/// The body of a request for `model` to answer `messages`, which are
/// in the shape of the chat protocol, with `tools` of that protocol.
fn request(
    model: &str,
    messages: &[Value],
    tools: &[Value],
) -> Value {
    let mut body = json!({
        "model": model,
        "input": input(messages),
        "store": false,
        "stream": true,
        "reasoning": {"summary": "auto"},
    });
    if !tools.is_empty() {
        let functions = tools
            .iter()
            .map(|tool| {
                let function = &tool["function"];
                json!({
                    "type": "function",
                    "name": function["name"],
                    "description": function["description"].as_str().unwrap_or_default(),
                    "parameters": function["parameters"],
                    "strict": false,
                })
            })
            .collect::<Vec<_>>();
        body["tools"] = json!([{
            "type": "namespace",
            "name": NAMESPACE,
            "description": "The tools of a hamlet.",
            "tools": functions,
        }]);
    }
    body
}

/// The text of a message's content: itself, or its parts' texts.
fn text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The input items of a conversation in the shape of the chat
/// protocol. Reasoning is not sent back, since nothing is stored.
fn input(messages: &[Value]) -> Vec<Value> {
    let mut items = Vec::new();
    for message in messages {
        let content = text(&message["content"]);
        match message["role"].as_str().unwrap_or_default() {
            // The plan refuses system messages.
            "system" | "developer" => items.push(
                json!({"role": "developer", "content": content}),
            ),
            "user" => items
                .push(json!({"role": "user", "content": content})),
            "assistant" => {
                if !content.is_empty() {
                    items.push(
                        json!({"role": "assistant", "content": content}),
                    );
                }
                let calls = message["tool_calls"].as_array();
                for call in calls.into_iter().flatten() {
                    items.push(json!({
                        "type": "function_call",
                        "call_id": call["id"],
                        "name": call["function"]["name"],
                        "arguments": call["function"]["arguments"],
                        "namespace": NAMESPACE,
                    }));
                }
            }
            "tool" => items.push(json!({
                "type": "function_call_output",
                "call_id": message["tool_call_id"],
                "output": content,
            })),
            _ => {}
        }
    }
    items
}

/// Accumulates the events of a streamed response into one assistant
/// message in the shape of the chat protocol.
#[derive(Default)]
struct ResponseAcc {
    content: String,
    reasoning: String,
    calls: Vec<ToolCall>,
    usage: Option<Value>,
    finish_reason: Option<String>,
    started: Option<Instant>,
    first_token_ms: Option<u64>,
}

#[derive(Default)]
struct ToolCall {
    /// The id of its output item, which its argument deltas name.
    item: String,
    call_id: String,
    name: String,
    arguments: String,
}

impl ResponseAcc {
    fn since(started: Instant) -> Self {
        ResponseAcc {
            started: Some(started),
            ..ResponseAcc::default()
        }
    }

    fn produced(&mut self) {
        if self.first_token_ms.is_none() {
            self.first_token_ms =
                self.started.map(|t| t.elapsed().as_millis() as u64);
        }
    }

    /// The call whose output item is `item`, added if it is new.
    fn call(&mut self, item: &str) -> &mut ToolCall {
        let at = match self.calls.iter().position(|c| c.item == item)
        {
            Some(at) => at,
            None => {
                self.calls.push(ToolCall {
                    item: item.to_string(),
                    ..ToolCall::default()
                });
                self.calls.len() - 1
            }
        };
        &mut self.calls[at]
    }

    /// Feeds one line. True once the response has ended.
    fn feed_line(
        &mut self,
        line: &str,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<bool> {
        let Some(data) = line.trim().strip_prefix("data:") else {
            return Ok(false);
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(false);
        }
        let event = serde_json::from_str::<Value>(data)
            .map_err(|e| anyhow!("bad SSE event ({e}): {data}"))?;
        let delta = event["delta"].as_str().unwrap_or_default();
        let item = &event["item"];
        match event["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" if !delta.is_empty() => {
                self.produced();
                self.content.push_str(delta);
                on_delta(Delta::Text(delta));
            }
            "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta"
                if !delta.is_empty() =>
            {
                self.produced();
                self.reasoning.push_str(delta);
                on_delta(Delta::Reasoning(delta));
            }
            // Parts of a summary are paragraphs.
            "response.reasoning_summary_part.added"
                if !self.reasoning.is_empty() =>
            {
                self.reasoning.push_str("\n\n");
                on_delta(Delta::Reasoning("\n\n"));
            }
            "response.output_item.added"
            | "response.output_item.done"
                if item["type"] == "function_call" =>
            {
                self.produced();
                let call = self
                    .call(item["id"].as_str().unwrap_or_default());
                let text = |key: &str| {
                    item[key].as_str().unwrap_or_default()
                };
                call.call_id = text("call_id").into();
                call.name = text("name").into();
                // Whole once the item is done.
                if !text("arguments").is_empty() {
                    call.arguments = text("arguments").into();
                }
            }
            "response.function_call_arguments.delta" => {
                let item =
                    event["item_id"].as_str().unwrap_or_default();
                self.call(item).arguments.push_str(delta);
            }
            "response.completed" => {
                self.usage = Some(event["response"]["usage"].clone());
                let reason = match self.calls.is_empty() {
                    true => "stop",
                    false => "tool_calls",
                };
                self.finish_reason = Some(reason.into());
                return Ok(true);
            }
            "response.incomplete" => {
                let response = &event["response"];
                self.usage = Some(response["usage"].clone());
                let why = response["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("incomplete");
                // As the chat protocol says it, which the agents
                // read.
                let reason = match why {
                    "max_output_tokens" => "length",
                    other => other,
                };
                self.finish_reason = Some(reason.into());
                return Ok(true);
            }
            "response.failed" => {
                let error = &event["response"]["error"];
                bail!(plan_error(
                    error["code"].as_str().unwrap_or("failed"),
                    error["message"]
                        .as_str()
                        .unwrap_or("the response failed"),
                ));
            }
            "error" => {
                let error = match event["error"].is_object() {
                    true => &event["error"],
                    false => &event,
                };
                bail!(plan_error(
                    error["code"].as_str().unwrap_or("error"),
                    error["message"].as_str().unwrap_or_default(),
                ));
            }
            _ => {}
        }
        Ok(false)
    }

    fn into_message(self) -> Value {
        let mut message =
            json!({"role": "assistant", "content": self.content});
        if !self.reasoning.is_empty() {
            message["reasoning"] = json!(self.reasoning);
        }
        if !self.calls.is_empty() {
            let calls = self
                .calls
                .into_iter()
                .map(|c| {
                    json!({"id": c.call_id, "type": "function",
                           "function": {"name": c.name, "arguments": c.arguments}})
                })
                .collect::<Vec<_>>();
            message["tool_calls"] = json!(calls);
        }
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conversation_is_sent_as_the_plan_takes_it() {
        let messages = [
            json!({"role": "system", "content": "Be brief."}),
            json!({"role": "user", "content": [{"type": "text", "text": "ls"}]}),
            json!({"role": "assistant", "content": "", "reasoning": "hm",
                   "tool_calls": [{"id": "c1", "type": "function",
                   "function": {"name": "bash", "arguments": "{}"}}]}),
            json!({"role": "tool", "tool_call_id": "c1", "content": "a.txt"}),
        ];
        let tools = [json!({"type": "function", "function": {
            "name": "bash", "parameters": {"type": "object"}}})];
        let body = request("m", &messages, &tools);
        assert_eq!(
            (&body["store"], &body["stream"]),
            (&json!(false), &json!(true))
        );
        assert_eq!(
            body["input"],
            json!([
                {"role": "developer", "content": "Be brief."},
                {"role": "user", "content": "ls"},
                {"type": "function_call", "call_id": "c1", "name": "bash",
                 "arguments": "{}", "namespace": "hamlet"},
                {"type": "function_call_output", "call_id": "c1", "output": "a.txt"},
            ])
        );
        let namespace = &body["tools"][0];
        assert_eq!(namespace["type"], "namespace");
        assert_eq!(namespace["tools"][0]["name"], "bash");
        assert_eq!(namespace["tools"][0]["strict"], false);
    }

    #[test]
    fn a_streamed_response_becomes_one_message() {
        let events = [
            json!({"type": "response.created"}),
            json!({"type": "response.reasoning_summary_part.added"}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "Look."}),
            json!({"type": "response.reasoning_summary_part.added"}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "Run."}),
            json!({"type": "response.output_text.delta", "delta": "On it"}),
            json!({"type": "response.output_item.added", "item": {"type": "function_call",
                   "id": "fc_1", "call_id": "c1", "name": "bash", "arguments": ""}}),
            json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1",
                   "delta": "{\"command\":"}),
            json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1",
                   "delta": "\"ls\"}"}),
            json!({"type": "response.completed", "response": {"usage": {
                   "input_tokens": 50, "input_tokens_details": {"cached_tokens": 10},
                   "output_tokens": 20, "output_tokens_details": {"reasoning_tokens": 5}}}}),
        ];
        let mut acc = ResponseAcc::default();
        let mut seen = Vec::new();
        let mut hear = |d: Delta<'_>| match d {
            Delta::Text(s) => seen.push(('t', s.to_string())),
            Delta::Reasoning(s) => seen.push(('r', s.to_string())),
        };
        let ended = events
            .iter()
            .map(|e| {
                acc.feed_line(&format!("data: {e}"), &mut hear)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(ended.iter().filter(|e| **e).count(), 1);
        assert!(ended.last().unwrap());
        assert_eq!(seen[0], ('r', "Look.".into()));
        assert_eq!(seen.last().unwrap(), &('t', "On it".into()));
        assert_eq!(acc.finish_reason.as_deref(), Some("tool_calls"));
        let mut call = CallRecord::default();
        call.read_usage(&acc.usage.take().unwrap());
        assert_eq!(
            (
                call.input_tokens,
                call.cached_input_tokens,
                call.output_tokens,
                call.reasoning_tokens
            ),
            (Some(50), Some(10), Some(20), Some(5))
        );
        assert_eq!(
            acc.into_message(),
            json!({"role": "assistant", "content": "On it", "reasoning": "Look.\n\nRun.",
                   "tool_calls": [{"id": "c1", "type": "function",
                   "function": {"name": "bash", "arguments": "{\"command\":\"ls\"}"}}]})
        );

        // A spent plan fails the call, with where to look.
        let spent = json!({"type": "response.failed", "response": {"error": {
            "code": "subscription_sharing_usage_limit_exceeded", "message": "Limit reached"}}});
        let why = ResponseAcc::default()
            .feed_line(&format!("data: {spent}"), &mut |_| {})
            .unwrap_err()
            .to_string();
        assert!(
            why.contains("Limit reached")
                && why.contains("settings/usage"),
            "{why}"
        );
    }
}
