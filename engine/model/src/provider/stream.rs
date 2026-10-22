//! A streamed reply, put together from its server-sent lines.

use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::provider::Delta;

/// Splits a byte stream into lines. Buffers bytes so a line (or a
/// UTF-8 character) split across chunks is reassembled before
/// decoding.
#[derive(Default)]
pub(super) struct LineSplitter {
    buf: Vec<u8>,
}

impl LineSplitter {
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) =
            self.buf.iter().position(|&b| b == b'\n')
        {
            let line = self.buf.drain(..=pos).collect::<Vec<_>>();
            out.push(
                String::from_utf8_lossy(&line).trim().to_string(),
            );
        }
        out
    }

    /// Returns any trailing line that had no newline.
    pub(super) fn finish(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let line =
            String::from_utf8_lossy(&self.buf).trim().to_string();
        self.buf.clear();
        Some(line)
    }
}

/// Accumulates SSE lines into one assistant message.
#[derive(Default)]
pub(super) struct StreamAcc {
    content: String,
    reasoning: String,
    // Whichever key the server used, so it is written back
    // unchanged.
    reasoning_key: Option<&'static str>,
    calls: Vec<ToolCall>,
    /// Arrives in a final chunk whose `choices` may be empty.
    pub(super) usage: Option<Value>,
    pub(super) finish_reason: Option<String>,
    started: Option<Instant>,
    pub(super) first_token_ms: Option<u64>,
}

#[derive(Default)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl StreamAcc {
    /// An empty reply whose first token is timed from `started`.
    pub(super) fn since(started: Instant) -> Self {
        StreamAcc {
            started: Some(started),
            ..StreamAcc::default()
        }
    }

    /// True when the reply finished: the stream said so, or a finish
    /// reason came.
    pub(super) fn ended(&self, done: bool) -> bool {
        done || self.finish_reason.is_some()
    }

    /// Feeds one line. True once `[DONE]` is seen.
    pub(super) fn feed_line(
        &mut self,
        line: &str,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<bool> {
        let Some(data) = line.trim().strip_prefix("data:") else {
            return Ok(false);
        };
        let data = data.trim();
        if data == "[DONE]" {
            return Ok(true);
        }
        if data.is_empty() {
            return Ok(false);
        }
        let v = serde_json::from_str::<Value>(data)
            .map_err(|e| anyhow!("bad SSE chunk ({e}): {data}"))?;
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            bail!("provider error: {err}");
        }
        if let Some(usage) = v.get("usage").filter(|u| u.is_object())
        {
            self.usage = Some(usage.clone());
        }
        if let Some(reason) =
            v["choices"][0]["finish_reason"].as_str()
        {
            self.finish_reason = Some(reason.to_string());
        }
        let d = &v["choices"][0]["delta"];
        let produced = ["reasoning", "reasoning_content", "content"]
            .iter()
            .any(|k| d[*k].as_str().is_some_and(|s| !s.is_empty()))
            || d["tool_calls"].is_array();
        if produced && self.first_token_ms.is_none() {
            self.first_token_ms =
                self.started.map(|t| t.elapsed().as_millis() as u64);
        }
        for key in ["reasoning", "reasoning_content"] {
            if let Some(s) = d[key].as_str() {
                self.reasoning_key = Some(key);
                if !s.is_empty() {
                    self.reasoning.push_str(s);
                    on_delta(Delta::Reasoning(s));
                }
            }
        }
        if let Some(s) =
            d["content"].as_str().filter(|s| !s.is_empty())
        {
            self.content.push_str(s);
            on_delta(Delta::Text(s));
        }
        for tc in d["tool_calls"].as_array().into_iter().flatten() {
            let i = tc["index"].as_u64().unwrap_or(0) as usize;
            while self.calls.len() <= i {
                self.calls.push(ToolCall::default());
            }
            let c = &mut self.calls[i];
            if let Some(id) = tc["id"].as_str() {
                c.id = id.to_string();
            }
            if let Some(s) = tc["function"]["name"].as_str() {
                c.name.push_str(s);
            }
            if let Some(s) = tc["function"]["arguments"].as_str() {
                c.arguments.push_str(s);
            }
        }
        Ok(false)
    }

    pub(super) fn into_message(self) -> Value {
        let mut msg =
            json!({"role": "assistant", "content": self.content});
        if let Some(k) = self.reasoning_key {
            msg[k] = json!(self.reasoning);
        }
        if !self.calls.is_empty() {
            let calls = self
                .calls
                .into_iter()
                .enumerate()
                .map(|(n, c)| {
                    let id = if c.id.is_empty() { format!("call_{n}") } else { c.id };
                    json!({"id": id, "type": "function",
                           "function": {"name": c.name, "arguments": c.arguments}})
                })
                .collect::<Vec<_>>();
            msg["tool_calls"] = json!(calls);
        }
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::CallRecord;

    fn chunk(delta: Value) -> String {
        format!("data: {}", json!({"choices": [{"delta": delta}]}))
    }

    /// Feeds lines, returning the message and the collected deltas as
    /// (kind, text).
    fn run(lines: &[String]) -> Result<(Value, Vec<(char, String)>)> {
        let mut acc = StreamAcc::default();
        let mut seen = Vec::new();
        let mut cb = |d: Delta<'_>| match d {
            Delta::Text(s) => seen.push(('t', s.to_string())),
            Delta::Reasoning(s) => seen.push(('r', s.to_string())),
        };
        for l in lines {
            if acc.feed_line(l, &mut cb)? {
                break;
            }
        }
        Ok((acc.into_message(), seen))
    }

    #[test]
    fn content_only() {
        let lines = [
            ": keepalive".to_string(),
            String::new(),
            chunk(json!({"role": "assistant", "content": "Hel"})),
            chunk(json!({"content": "lo"})),
            "data: [DONE]".to_string(),
        ];
        let (msg, seen) = run(&lines).unwrap();
        assert_eq!(
            msg,
            json!({"role": "assistant", "content": "Hello"})
        );
        assert_eq!(
            seen,
            vec![('t', "Hel".into()), ('t', "lo".into())]
        );
    }

    #[test]
    fn reasoning_key_is_preserved() {
        for key in ["reasoning", "reasoning_content"] {
            let lines = [
                chunk(json!({key: "think "})),
                chunk(json!({key: "hard"})),
                chunk(json!({"content": "ok"})),
            ];
            let (msg, seen) = run(&lines).unwrap();
            assert_eq!(msg[key], "think hard");
            let other = if key == "reasoning" {
                "reasoning_content"
            } else {
                "reasoning"
            };
            assert!(msg.get(other).is_none());
            assert_eq!(msg["content"], "ok");
            assert_eq!(seen[0], ('r', "think ".to_string()));
        }
    }

    #[test]
    fn parallel_tool_calls_merge_by_index() {
        let lines = [
            chunk(json!({"tool_calls": [
                {"index": 0, "id": "a", "function": {"name": "bash", "arguments": ""}},
                {"index": 1, "id": "b", "function": {"name": "edit", "arguments": "{\"p"}}]})),
            chunk(json!({"tool_calls": [
                {"index": 0, "function": {"arguments": "{\"command\":"}}]})),
            chunk(json!({"tool_calls": [
                {"index": 1, "function": {"arguments": "ath\":1}"}}]})),
            chunk(json!({"tool_calls": [
                {"index": 0, "function": {"arguments": "\"ls\"}"}}]})),
            "data: [DONE]".to_string(),
        ];
        let (msg, _) = run(&lines).unwrap();
        assert_eq!(msg["content"], "");
        assert_eq!(
            msg["tool_calls"],
            json!([
                {"id": "a", "type": "function",
                 "function": {"name": "bash", "arguments": "{\"command\":\"ls\"}"}},
                {"id": "b", "type": "function",
                 "function": {"name": "edit", "arguments": "{\"path\":1}"}},
            ])
        );
    }

    #[test]
    fn missing_tool_call_id_is_synthesized() {
        let lines = [chunk(json!({"tool_calls": [
            {"index": 0, "function": {"name": "bash", "arguments": "{}"}}]}))];
        let (msg, _) = run(&lines).unwrap();
        assert_eq!(msg["tool_calls"][0]["id"], "call_0");
    }

    #[test]
    fn line_split_across_byte_chunks() {
        let full =
            format!("{}\n", chunk(json!({"content": "héllo"})));
        let bytes = full.as_bytes();
        // Split inside the two-byte e-acute.
        let cut = full.find('é').unwrap() + 1;
        let mut splitter = LineSplitter::default();
        assert!(splitter.push(&bytes[..cut]).is_empty());
        let lines = splitter.push(&bytes[cut..]);
        assert_eq!(lines.len(), 1);
        let (msg, _) = run(&lines).unwrap();
        assert_eq!(msg["content"], "héllo");
        assert!(splitter.finish().is_none());
        // A last line with no newline is kept for the end.
        assert!(splitter.push(b"data: [DONE]").is_empty());
        assert_eq!(
            splitter.finish().as_deref(),
            Some("data: [DONE]")
        );
    }

    #[test]
    fn error_chunk_fails() {
        let lines = [
            chunk(json!({"content": "x"})),
            format!(
                "data: {}",
                json!({"error": {"message": "boom"}})
            ),
        ];
        let err = run(&lines).unwrap_err().to_string();
        assert!(err.contains("boom"), "{err}");
    }

    #[test]
    fn usage_arrives_in_a_final_chunk_with_no_choices() {
        let mut acc = StreamAcc::default();
        let mut cb = |_: Delta<'_>| {};
        for line in [
            chunk(json!({"content": "hi"})),
            r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#.to_string(),
            r#"data: {"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":30,"total_tokens":150}}"#.to_string(),
            "data: [DONE]".to_string(),
        ] {
            acc.feed_line(&line, &mut cb).unwrap();
        }
        assert_eq!(acc.finish_reason.as_deref(), Some("stop"));
        let mut call = CallRecord::default();
        call.read_usage(&acc.usage.unwrap());
        assert_eq!(
            (call.input_tokens, call.output_tokens),
            (Some(120), Some(30))
        );
        assert_eq!(call.reasoning_tokens, None);

        // The shapes of other providers.
        let mut openai = CallRecord::default();
        openai.read_usage(&json!({"prompt_tokens": 100, "completion_tokens": 40,
            "prompt_tokens_details": {"cached_tokens": 64},
            "completion_tokens_details": {"reasoning_tokens": 25}, "cost": 0.0012}));
        assert_eq!(openai.cached_input_tokens, Some(64));
        assert_eq!(openai.reasoning_tokens, Some(25));
        assert_eq!(openai.billed_cost, Some(0.0012));

        let mut deepseek = CallRecord::default();
        deepseek.read_usage(&json!({"prompt_tokens": 100, "completion_tokens": 40,
            "prompt_cache_hit_tokens": 80, "prompt_cache_miss_tokens": 20}));
        assert_eq!(deepseek.cached_input_tokens, Some(80));
        assert_eq!(deepseek.billed_cost, None);
    }

    #[test]
    fn a_stream_cut_short_has_not_ended() {
        let mut acc = StreamAcc::default();
        let mut cb = |_: Delta<'_>| {};
        acc.feed_line(
            "data: {\"choices\":[{\"delta\":{\"content\":\"half \
             a\"}}]}",
            &mut cb,
        )
        .unwrap();
        assert!(!acc.ended(false));
        // Either sign of an end is enough.
        assert!(acc.ended(true));
        acc.feed_line(
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"\
             stop\"}]}",
            &mut cb,
        )
        .unwrap();
        assert!(acc.ended(false));
        assert!(acc.feed_line("data:[DONE]", &mut cb).unwrap());
    }
}
