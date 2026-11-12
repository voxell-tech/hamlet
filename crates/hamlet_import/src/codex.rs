//! Codex's sessions: one JSON line per record in
//! `<home>/sessions/<year>/<month>/<day>/rollout-*.jsonl`, where
//! `<home>` is `CODEX_HOME` or `~/.codex`, with their names in
//! `<home>/session_index.jsonl`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use hamlet_protocol::client::ImportSource;
use serde_json::Value;

use crate::{
    Draft, Found, HEAD, files_under, folder, modified, records,
    summary, time,
};

pub(crate) fn find() -> Vec<Found> {
    let Some(home) = folder("CODEX_HOME", ".codex") else {
        return Vec::new();
    };
    let names = records(&home.join("session_index.jsonl"), None)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|record| {
            Some((
                record["id"].as_str()?.to_owned(),
                record["thread_name"].as_str()?.to_owned(),
            ))
        })
        .collect::<HashMap<_, _>>();
    files_under(&home.join("sessions"), 3, "jsonl")
        .iter()
        .filter_map(|path| listed(path, &names))
        .collect()
}

fn listed(
    path: &Path,
    names: &HashMap<String, String>,
) -> Option<Found> {
    let mut session = None;
    let mut folder = None;
    let mut first = None;
    for record in records(path, Some(HEAD)).ok()? {
        if record["type"] == "session_meta" {
            session =
                record["payload"]["id"].as_str().map(str::to_owned);
            folder =
                record["payload"]["cwd"].as_str().map(str::to_owned);
        }
        if first.is_none() && is_message(&record, "user") {
            first = user_text(&record["payload"]);
        }
    }
    let session = session?;
    let title = names.get(&session).cloned().or(first)?;
    Some(Found {
        source: ImportSource::Codex,
        session,
        title: summary(&title),
        folder,
        modified: modified(path)?,
        path: path.to_owned(),
    })
}

pub(crate) fn read(path: &Path) -> Result<Draft> {
    let records = records(path, None)?;
    let session = records
        .iter()
        .find(|record| record["type"] == "session_meta")
        .and_then(|record| record["payload"]["id"].as_str())
        .unwrap_or_default()
        .to_owned();
    let mut read = Draft::new(ImportSource::Codex, session);
    for record in &records {
        let at = time(&record["timestamp"]).unwrap_or(0);
        let payload = &record["payload"];
        if record["type"] == "turn_context"
            && let Some(model) = payload["model"].as_str()
        {
            read.model = Some(model.to_owned());
        }
        if record["type"] != "response_item" {
            continue;
        }
        match payload["type"].as_str() {
            Some("message") if payload["role"] == "user" => {
                if let Some(text) = user_text(payload) {
                    read.user(at, &text);
                }
            }
            Some("message") if payload["role"] == "assistant" => {
                for part in payload["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    read.assistant(
                        at,
                        part["text"].as_str().unwrap_or_default(),
                    );
                }
            }
            Some("function_call") => read.step(
                at,
                payload["name"].as_str().unwrap_or_default(),
                &payload["arguments"],
            ),
            Some("custom_tool_call") => read.step(
                at,
                payload["name"].as_str().unwrap_or_default(),
                &payload["input"],
            ),
            Some("local_shell_call") => {
                read.step(at, "shell", &payload["action"]);
            }
            _ => {}
        }
    }
    Ok(read)
}

fn is_message(record: &Value, role: &str) -> bool {
    record["type"] == "response_item"
        && record["payload"]["type"] == "message"
        && record["payload"]["role"] == role
}

/// The words the person typed in a user message, without the context
/// Codex puts in parts of their own.
fn user_text(payload: &Value) -> Option<String> {
    let text = payload["content"]
        .as_array()?
        .iter()
        .filter_map(|part| part["text"].as_str())
        .map(str::trim)
        .filter(|text| {
            !text.is_empty()
                && !text.starts_with('<')
                && !text.starts_with("# AGENTS.md instructions")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    (!text.is_empty()).then_some(text)
}
