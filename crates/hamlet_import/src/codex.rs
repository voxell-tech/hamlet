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
    Draft, Found, files_under, folder, listing, modified, records,
    summary, time,
};

/// How the parts Codex adds to a user message begin, each in a part
/// of its own: the context it gives the model and what its window
/// attached.
const GENERATED: [&str; 14] = [
    "<environment_context>",
    "<user_instructions>",
    "<permissions instructions>",
    "<collaboration_mode>",
    "<turn_aborted>",
    "<user_shell_command>",
    "<subagent_notification>",
    "<skill>",
    "<image ",
    "</image>",
    "<external_",
    "<in-app-browser-context",
    "# AGENTS.md instructions",
    "# Files mentioned by the user",
];

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
    let (session, folder, title) = listing(path, |records| {
        let mut session = None;
        let mut folder = None;
        let mut first = None;
        for record in records {
            if record["type"] == "session_meta" {
                session = record["payload"]["id"]
                    .as_str()
                    .map(str::to_owned);
                folder = record["payload"]["cwd"]
                    .as_str()
                    .map(str::to_owned);
            }
            if first.is_none() && is_message(record, "user") {
                first = user_text(&record["payload"]);
            }
        }
        let session = session?;
        let title = names.get(&session).cloned().or(first)?;
        Some((session, folder, title))
    })?;
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
        // Codex takes back the newest turns when it replays them.
        if record["type"] == "event_msg"
            && payload["type"] == "thread_rolled_back"
        {
            let turns = payload["num_turns"].as_u64().unwrap_or(0);
            read.take_back(
                usize::try_from(turns).unwrap_or(usize::MAX),
            );
        }
        if record["type"] != "response_item" {
            continue;
        }
        match payload["type"].as_str() {
            Some("message") if payload["role"] == "user" => {
                let text = user_text(payload);
                // An image alone is a turn too, which a rollback
                // counts.
                if text.is_some() || has_image(payload) {
                    read.turn();
                }
                if let Some(text) = text {
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

/// Whether a user message attached an image.
fn has_image(payload: &Value) -> bool {
    payload["content"].as_array().is_some_and(|parts| {
        parts.iter().any(|part| part["type"] == "input_image")
    })
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
                && !GENERATED
                    .iter()
                    .any(|made| text.starts_with(made))
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    (!text.is_empty()).then_some(text)
}
