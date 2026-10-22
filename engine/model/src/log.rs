//! Append-only JSONL session log, which model context, replay and
//! usage counts are read from.

mod template;

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::log::template::args_template;

const SCHEMA_VERSION: u32 = 1;

/// A session's log, open for appending.
pub struct Log {
    path: PathBuf,
    file: File,
    secrets: Vec<String>,
}

/// One tool call, as the log keeps it.
pub struct ToolRecord<'a> {
    pub call_id: &'a str,
    pub name: &'a str,
    pub args: &'a str,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub policy: &'a str,
    pub degraded: bool,
    pub output: &'a str,
}

impl Log {
    /// Opens (creating parent dirs) for append. `secrets` are
    /// scrubbed from every string written.
    pub fn open(path: &Path, secrets: Vec<String>) -> Result<Self> {
        if let Some(dir) =
            path.parent().filter(|d| !d.as_os_str().is_empty())
        {
            fs::create_dir_all(dir).with_context(|| {
                format!("creating {}", dir.display())
            })?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        // A crash can leave a partial last line; start fresh on a new
        // line.
        if let Ok(bytes) = fs::read(path)
            && bytes.last().is_some_and(|b| *b != b'\n')
        {
            file.write_all(b"\n")?;
            file.flush()?;
        }
        let secrets =
            secrets.into_iter().filter(|s| !s.is_empty()).collect();
        Ok(Self {
            path: path.to_path_buf(),
            file,
            secrets,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn user(&mut self, user: &str, content: &str) -> Result<()> {
        self.write(
            "user",
            json!({ "user": user, "content": content }),
        )
    }

    /// The full assistant message as returned by the provider,
    /// written verbatim (after scrubbing).
    pub fn assistant(&mut self, message: &Value) -> Result<()> {
        self.write("assistant", json!({ "message": message }))
    }

    pub fn tool(&mut self, rec: &ToolRecord) -> Result<()> {
        let hash = Sha256::digest(rec.args.as_bytes());
        let args_hash = hash
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        self.write(
            "tool",
            json!({
                "call_id": rec.call_id,
                "name": rec.name,
                "args": rec.args,
                "args_template": args_template(rec.args),
                "args_hash": args_hash,
                "exit_code": rec.exit_code,
                "duration_ms": rec.duration_ms,
                "policy": rec.policy,
                "degraded": rec.degraded,
                "output": rec.output,
            }),
        )
    }

    /// One model call: its usage and timing.
    pub fn model_call(
        &mut self,
        call: &crate::provider::CallRecord,
    ) -> Result<()> {
        self.write(
            "model_call",
            json!({
                "model": call.model,
                "endpoint": call.endpoint,
                "input_tokens": call.input_tokens,
                "cached_input_tokens": call.cached_input_tokens,
                "output_tokens": call.output_tokens,
                "reasoning_tokens": call.reasoning_tokens,
                "billed_cost": call.billed_cost,
                "usage": call.usage_raw,
                "first_token_ms": call.first_token_ms,
                "duration_ms": call.duration_ms,
                "finish_reason": call.finish_reason,
            }),
        )
    }

    /// Words for whoever reads the conversation. No model is shown
    /// them.
    pub fn notice(&mut self, text: &str) -> Result<()> {
        self.write("notice", json!({ "text": text }))
    }

    /// A summary that stands for everything said before it.
    pub fn compacted(&mut self, summary: &str) -> Result<()> {
        self.write("compacted", json!({ "summary": summary }))
    }

    pub fn turn_aborted(&mut self, reason: &str) -> Result<()> {
        self.write("turn_aborted", json!({ "reason": reason }))
    }

    /// Takes out the message that is line `to` of the log, counted
    /// as [`read_events`] yields them, and every line after it.
    pub fn rewind(&mut self, to: usize) -> Result<()> {
        self.write("rewind", json!({ "to": to }))
    }

    fn write(&mut self, kind: &str, fields: Value) -> Result<()> {
        let mut obj = Map::new();
        obj.insert("v".into(), json!(SCHEMA_VERSION));
        obj.insert("ts".into(), json!(now_millis()));
        obj.insert("type".into(), json!(kind));
        if let Value::Object(f) = fields {
            obj.extend(f);
        }
        let mut value = Value::Object(obj);
        scrub(&mut value, &self.secrets);
        let mut line = serde_json::to_string(&value)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.flush()?;
        Ok(())
    }
}

/// The time now, in milliseconds since 1970.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn scrub(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(s) => {
            for secret in secrets {
                if s.contains(secret.as_str()) {
                    *s = s.replace(secret.as_str(), "[REDACTED]");
                }
            }
        }
        Value::Array(items) => {
            items.iter_mut().for_each(|v| scrub(v, secrets))
        }
        Value::Object(map) => {
            map.values_mut().for_each(|v| scrub(v, secrets))
        }
        _ => {}
    }
}

/// `text` as a log opened with `secrets` writes it.
pub fn scrubbed(text: &str, secrets: &[String]) -> String {
    secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .fold(text.to_string(), |text, secret| {
            text.replace(secret.as_str(), "[REDACTED]")
        })
}

/// Reads every line written, those a rewind took out too. A missing
/// file is empty; unparseable lines (such as a truncated last line)
/// are skipped.
pub fn read_events(path: &Path) -> Result<Vec<Value>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!("reading {}", path.display())
            });
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .filter(Value::is_object)
        .collect())
}

/// True for each of `events` that a rewind took out, and for the
/// rewinds themselves. A rewind takes out the lines from the message
/// it names up to itself, and one that names no message that stands
/// takes out nothing.
pub fn taken_out(events: &[Value]) -> Vec<bool> {
    let mut out = vec![false; events.len()];
    for (at, event) in events.iter().enumerate() {
        if event["type"] != "rewind" {
            continue;
        }
        out[at] = true;
        let to = event["to"]
            .as_u64()
            .and_then(|to| usize::try_from(to).ok())
            .filter(|to| {
                *to < at && !out[*to] && events[*to]["type"] == "user"
            });
        if let Some(to) = to {
            out[to..at].fill(true);
        }
    }
    out
}

/// The lines of `events` that stand: the conversation as it is after
/// its rewinds.
pub fn standing(events: Vec<Value>) -> Vec<Value> {
    let out = taken_out(&events);
    events
        .into_iter()
        .zip(out)
        .filter(|(_, out)| !out)
        .map(|(event, _)| event)
        .collect()
}

/// Reads the lines of a log that stand. A conversation is read with
/// this, and only what counts every line written with
/// [`read_events`].
pub fn read_standing(path: &Path) -> Result<Vec<Value>> {
    read_events(path).map(standing)
}

/// Rebuilds OpenAI chat history (no system message) from events.
pub fn derive_messages(events: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    // Tool call ids from the latest assistant message still lacking a
    // result.
    let mut pending = Vec::<String>::new();
    let flush = |pending: &mut Vec<String>, out: &mut Vec<Value>| {
        for id in pending.drain(..) {
            out.push(json!({ "role": "tool", "tool_call_id": id, "content": "Error: aborted" }));
        }
    };
    for ev in events {
        match ev.get("type").and_then(Value::as_str) {
            Some("user") => {
                flush(&mut pending, &mut out);
                let content =
                    ev.get("content").cloned().unwrap_or(Value::Null);
                out.push(
                    json!({ "role": "user", "content": content }),
                );
            }
            Some("assistant") => {
                flush(&mut pending, &mut out);
                let Some(msg) = ev.get("message") else {
                    continue;
                };
                pending = msg
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| {
                        c.get("id")
                            .and_then(Value::as_str)
                            .map(String::from)
                    })
                    .collect();
                out.push(msg.clone());
            }
            Some("compacted") => {
                pending.clear();
                out.clear();
                let summary =
                    ev.get("summary").cloned().unwrap_or(Value::Null);
                out.push(
                    json!({ "role": "user", "content": summary }),
                );
            }
            Some("tool") => {
                let Some(id) =
                    ev.get("call_id").and_then(Value::as_str)
                else {
                    continue;
                };
                // Drop results with no open call, the API rejects
                // them.
                let Some(pos) = pending.iter().position(|p| p == id)
                else {
                    continue;
                };
                pending.remove(pos);
                let content =
                    ev.get("output").cloned().unwrap_or(Value::Null);
                out.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
            }
            _ => {}
        }
    }
    flush(&mut pending, &mut out);
    out
}

/// Every assistant message in `events`, in order.
pub fn assistant_messages(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter(|e| {
            e.get("type").and_then(Value::as_str) == Some("assistant")
        })
        .filter_map(|e| e.get("message").cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    /// A log path in a directory of its own, which does not exist
    /// yet.
    fn tmp_path(name: &str) -> (Scratch, PathBuf) {
        let dir = Scratch::new(&format!("log-{name}"));
        let path = dir.join("sub").join("session.jsonl");
        (dir, path)
    }

    fn rec<'a>(
        id: &'a str,
        args: &'a str,
        output: &'a str,
    ) -> ToolRecord<'a> {
        ToolRecord {
            call_id: id,
            name: "bash",
            args,
            exit_code: 0,
            duration_ms: 12,
            policy: "p1",
            degraded: false,
            output,
        }
    }

    fn call_msg(ids: &[&str]) -> Value {
        let calls = ids
            .iter()
            .map(|id| json!({"id": id, "type": "function", "function": {"name": "bash", "arguments": "{}"}}))
            .collect::<Vec<_>>();
        json!({"role": "assistant", "content": null, "tool_calls": calls})
    }

    #[test]
    fn scrubs_nested_and_tool_output() {
        let (_dir, path) = tmp_path("scrub");
        let mut log =
            Log::open(&path, vec!["s3cr3t".into()]).unwrap();
        let msg = json!({
            "role": "assistant",
            "content": "key s3cr3t",
            "tool_calls": [{"id": "c1", "function": {"name": "bash", "arguments": "{\"k\":\"s3cr3t\"}"}}],
        });
        log.assistant(&msg).unwrap();
        log.tool(&rec("c1", "{}", "token=s3cr3t")).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("s3cr3t"));
        let ev = read_events(&path).unwrap();
        assert_eq!(
            ev[0]["message"]["tool_calls"][0]["function"]
                ["arguments"],
            "{\"k\":\"[REDACTED]\"}"
        );
        assert_eq!(ev[1]["output"], "token=[REDACTED]");
    }

    #[test]
    fn crash_resume_aborts_missing_results() {
        let (_dir, path) = tmp_path("crash");
        {
            let mut log = Log::open(&path, vec![]).unwrap();
            log.user("u", "go").unwrap();
            log.assistant(&call_msg(&["a", "b"])).unwrap();
            log.tool(&rec("a", "{}", "ra")).unwrap();
            // crash before b's result
        }
        // The call left open is closed at the end of the log too.
        let cut = derive_messages(&read_events(&path).unwrap());
        assert_eq!(cut[3]["content"], "Error: aborted");
        let mut log = Log::open(&path, vec![]).unwrap();
        log.user("u", "again").unwrap();
        let msgs = derive_messages(&read_events(&path).unwrap());
        let roles = msgs
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            roles,
            ["user", "assistant", "tool", "tool", "user"]
        );
        assert_eq!(msgs[2]["tool_call_id"], "a");
        assert_eq!(msgs[2]["content"], "ra");
        assert_eq!(msgs[3]["tool_call_id"], "b");
        assert_eq!(msgs[3]["content"], "Error: aborted");
    }

    #[test]
    fn truncated_last_line_skipped_and_append_recovers() {
        let (_dir, path) = tmp_path("trunc");
        assert!(read_events(&path).unwrap().is_empty());
        {
            let mut log = Log::open(&path, vec![]).unwrap();
            log.user("u", "one").unwrap();
        }
        let mut f =
            OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(br#"{"v":1,"ts":1,"type":"us"#).unwrap();
        drop(f);
        assert_eq!(read_events(&path).unwrap().len(), 1);
        let mut log = Log::open(&path, vec![]).unwrap();
        log.user("u", "two").unwrap();
        let ev = read_events(&path).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[1]["content"], "two");
    }

    #[test]
    fn a_rewind_takes_out_its_message_and_what_followed() {
        let user =
            |text: &str| json!({"type": "user", "content": text});
        let reply = |text: &str| {
            json!({"type": "assistant",
                "message": {"role": "assistant", "content": text}})
        };
        let rewind = |to: usize| json!({"type": "rewind", "to": to});
        let said = |events: &[Value]| {
            derive_messages(&standing(events.to_vec()))
                .iter()
                .map(|m| m["content"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        let mut log = vec![
            user("one"),
            reply("1"),
            user("two"),
            reply("2"),
            rewind(2),
        ];
        assert_eq!(said(&log), ["one", "1"]);
        // A message that is gone, a line that is no message and a
        // line that is not there are no targets.
        log.extend([rewind(2), rewind(1), rewind(40)]);
        assert_eq!(said(&log), ["one", "1"]);
        log.extend([user("three"), reply("3"), rewind(8)]);
        assert_eq!(said(&log), ["one", "1"]);
        // Back past the target of the first.
        log.extend([user("four"), rewind(0)]);
        assert!(said(&log).is_empty());

        // A summary made after the message goes with it.
        let log = [
            user("one"),
            reply("1"),
            user("two"),
            json!({"type": "compacted", "summary": "short"}),
            reply("2"),
            user("three"),
        ];
        assert_eq!(said(&log), ["short", "2", "three"]);
        let mut back = log.to_vec();
        back.push(rewind(5));
        assert_eq!(said(&back), ["short", "2"]);
        back.push(rewind(2));
        assert_eq!(said(&back), ["one", "1"]);
    }

    #[test]
    fn unknown_types_and_fields_ignored() {
        let ev = vec![
            json!({"v": 2, "type": "future", "x": 1}),
            json!({"v": 2, "type": "user", "content": "hi", "extra": [1]}),
        ];
        assert_eq!(
            derive_messages(&ev),
            vec![json!({"role": "user", "content": "hi"})]
        );
    }
}
