//! pi's sessions: one JSON line per entry in
//! `<time>_<id>.jsonl` files, in `PI_CODING_AGENT_SESSION_DIR`, else
//! in a folder for each project under `sessions` in
//! `PI_CODING_AGENT_DIR` or `~/.pi/agent`. Entries form a tree; the
//! last one written ends the branch pi opens.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use hamlet_protocol::client::ImportSource;
use serde_json::Value;

use crate::{
    Draft, Found, files_under, folder, listing, modified, records,
    summary, time,
};

/// The folder of pi's sessions, and how many folders down they are in
/// it: none in the one `PI_CODING_AGENT_SESSION_DIR` names, one for
/// each project in its own.
fn sessions() -> Option<(PathBuf, usize)> {
    std::env::var_os("PI_CODING_AGENT_SESSION_DIR")
        .filter(|sessions| !sessions.is_empty())
        .map(|sessions| (PathBuf::from(sessions), 0))
        .or_else(|| {
            folder("PI_CODING_AGENT_DIR", ".pi/agent")
                .map(|agent| (agent.join("sessions"), 1))
        })
}

pub(crate) fn find() -> Vec<Found> {
    let Some((sessions, depth)) = sessions() else {
        return Vec::new();
    };
    files_under(&sessions, depth, "jsonl")
        .iter()
        .filter_map(|path| listed(path))
        .collect()
}

fn listed(path: &Path) -> Option<Found> {
    let (session, folder, first) = listing(path, |records| {
        let mut session = None;
        let mut folder = None;
        let mut first = None;
        for record in records {
            if record["type"] == "session" {
                session = record["id"].as_str().map(str::to_owned);
                folder = record["cwd"].as_str().map(str::to_owned);
            }
            if first.is_none() && record["message"]["role"] == "user"
            {
                first = text_of(&record["message"]["content"]);
            }
        }
        Some((session?, folder, first?))
    })?;
    Some(Found {
        source: ImportSource::Pi,
        session,
        title: summary(&first),
        folder,
        modified: modified(path)?,
        path: path.to_owned(),
    })
}

pub(crate) fn read(path: &Path) -> Result<Draft> {
    let records = records(path, None)?;
    let session = records
        .iter()
        .find(|record| record["type"] == "session")
        .and_then(|record| record["id"].as_str())
        .unwrap_or_default()
        .to_owned();
    // Sessions of pi's first format give their entries no id and keep
    // them in a line, all of them open.
    let lined = !records.iter().any(|record| {
        record["type"] == "message" && record["id"].is_string()
    });
    let open = open_branch(&records);
    let mut read = Draft::new(ImportSource::Pi, session);
    for record in &records {
        let is_open = lined
            || record["id"]
                .as_str()
                .is_some_and(|id| open.contains(id));
        if record["type"] != "message" || !is_open {
            continue;
        }
        let message = &record["message"];
        let at = message["timestamp"]
            .as_u64()
            .or_else(|| time(&record["timestamp"]))
            .unwrap_or(0);
        match message["role"].as_str() {
            Some("user") => {
                if let Some(text) = text_of(&message["content"]) {
                    read.user(at, &text);
                }
            }
            Some("assistant") => {
                if let Some(model) = message["model"].as_str() {
                    read.model = Some(model.to_owned());
                }
                for block in message["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    match block["type"].as_str() {
                        Some("text") => {
                            read.assistant(
                                at,
                                block["text"]
                                    .as_str()
                                    .unwrap_or_default(),
                            );
                        }
                        Some("toolCall") => read.step(
                            at,
                            block["name"]
                                .as_str()
                                .unwrap_or_default(),
                            &block["arguments"],
                        ),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(read)
}

/// The ids from the last entry written back to the first.
fn open_branch(records: &[Value]) -> HashSet<&str> {
    let parents = records
        .iter()
        .filter_map(|record| {
            Some((
                record["id"].as_str()?,
                record["parentId"].as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut open = HashSet::new();
    let mut at = records
        .iter()
        .rev()
        .find(|record| record["type"] != "session")
        .and_then(|record| record["id"].as_str());
    while let Some(id) = at
        && open.insert(id)
    {
        at = parents.get(id).copied().flatten();
    }
    open
}

fn text_of(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
