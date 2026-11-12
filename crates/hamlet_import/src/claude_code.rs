//! Claude Code's sessions: one JSON line per record in
//! `<config>/projects/<folder>/<session>.jsonl`, where `<config>` is
//! `CLAUDE_CONFIG_DIR` or `~/.claude`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use hamlet_protocol::client::ImportSource;
use serde_json::Value;

use crate::{
    Draft, Found, HEAD, files_under, folder, modified, records,
    summary, time,
};

/// Words Claude Code writes as the person's that the person did not
/// type.
const NOT_TYPED: [&str; 3] =
    ["<command-", "<local-command-", "[Request interrupted"];

pub(crate) fn find() -> Vec<Found> {
    let Some(config) = folder("CLAUDE_CONFIG_DIR", ".claude") else {
        return Vec::new();
    };
    files_under(&config.join("projects"), 1, "jsonl")
        .iter()
        .filter_map(|path| listed(path))
        .collect()
}

/// The session at `path` as a list shows it, from its first records;
/// `None` for one with no message of the person's there.
fn listed(path: &Path) -> Option<Found> {
    let mut folder = None;
    let mut title = None;
    let mut first = None;
    for record in records(path, Some(HEAD)).ok()? {
        if folder.is_none() {
            folder = record["cwd"].as_str().map(str::to_owned);
        }
        if let Some(custom) = record["customTitle"].as_str() {
            title = Some(custom.to_owned());
        }
        if first.is_none() && is_message(&record) {
            first = user_text(&record);
        }
    }
    let first = first?;
    Some(Found {
        source: ImportSource::ClaudeCode,
        session: path.file_stem()?.to_string_lossy().into_owned(),
        title: summary(title.as_deref().unwrap_or(&first)),
        folder,
        modified: modified(path)?,
        path: path.to_owned(),
    })
}

pub(crate) fn read(path: &Path) -> Result<Draft> {
    let session = path
        .file_stem()
        .context("a session file has a name")?
        .to_string_lossy()
        .into_owned();
    Ok(read_records(session, &records(path, None)?))
}

/// The session `session` of `records`, its lines in order.
fn read_records(session: String, records: &[Value]) -> Draft {
    let shown = shown(records);
    let mut read = Draft::new(ImportSource::ClaudeCode, session);
    for record in records {
        if let Some(custom) = record["customTitle"].as_str() {
            read.title = Some(custom.to_owned());
        }
        let Some(uuid) = record["uuid"].as_str() else {
            continue;
        };
        if !is_message(record) || !shown.contains(uuid) {
            continue;
        }
        let at = time(&record["timestamp"]).unwrap_or(0);
        if record["type"] == "user" {
            if let Some(text) = user_text(record) {
                read.user(at, &text);
            }
            continue;
        }
        let message = &record["message"];
        if let Some(model) = message["model"].as_str()
            && !model.starts_with('<')
        {
            read.model = Some(model.to_owned());
        }
        for block in
            message["content"].as_array().into_iter().flatten()
        {
            match block["type"].as_str() {
                Some("text") => {
                    read.assistant(
                        at,
                        block["text"].as_str().unwrap_or_default(),
                    );
                }
                Some("tool_use") => read.step(
                    at,
                    block["name"].as_str().unwrap_or_default(),
                    &block["input"],
                ),
                _ => {}
            }
        }
    }
    read
}

fn is_message(record: &Value) -> bool {
    matches!(record["type"].as_str(), Some("user" | "assistant"))
        && record["isSidechain"] != true
        && record["isMeta"] != true
        && record["isCompactSummary"] != true
}

/// The uuids of the branch last shown: from the last message back to
/// the first, across compactions.
fn shown(records: &[Value]) -> HashSet<&str> {
    let parents = records
        .iter()
        .filter_map(|record| {
            let uuid = record["uuid"].as_str()?;
            let parent = record["parentUuid"]
                .as_str()
                .or_else(|| record["logicalParentUuid"].as_str());
            Some((uuid, parent))
        })
        .collect::<HashMap<_, _>>();
    let mut shown = HashSet::new();
    let mut at = records
        .iter()
        .rev()
        .find(|record| is_message(record))
        .and_then(|record| record["uuid"].as_str());
    while let Some(uuid) = at
        && shown.insert(uuid)
    {
        at = parents.get(uuid).copied().flatten();
    }
    shown
}

/// The words the person typed in a user record, without what Claude
/// Code added to them.
fn user_text(record: &Value) -> Option<String> {
    let content = &record["message"]["content"];
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
    let text = without_reminders(&text);
    let text = text.trim();
    if text.is_empty()
        || NOT_TYPED.iter().any(|not| text.starts_with(not))
    {
        return None;
    }
    Some(text.to_owned())
}

fn without_reminders(text: &str) -> String {
    const OPEN: &str = "<system-reminder>";
    const CLOSE: &str = "</system-reminder>";
    let mut kept = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        kept.push_str(&rest[..start]);
        match rest[start..].find(CLOSE) {
            Some(end) => rest = &rest[start + end + CLOSE.len()..],
            None => rest = "",
        }
    }
    kept.push_str(rest);
    kept
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client::ImportedEntry;
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_the_branch_last_shown() {
        let records = [
            json!({"type": "custom-title", "customTitle": "Fix the build"}),
            json!({"type": "user", "uuid": "a", "parentUuid": null, "timestamp": "2026-11-01T10:00:00Z",
                   "message": {"role": "user", "content": "<system-reminder>x</system-reminder>Why does it fail?"}}),
            json!({"type": "assistant", "uuid": "b", "parentUuid": "a", "timestamp": "2026-11-01T10:00:01Z",
                   "message": {"id": "m1", "model": "claude-x", "content": [{"type": "text", "text": "Looking."}]}}),
            json!({"type": "assistant", "uuid": "c", "parentUuid": "b", "timestamp": "2026-11-01T10:00:02Z",
                   "message": {"id": "m1", "content": [{"type": "tool_use", "name": "Bash", "input": {"command": "cargo build"}}]}}),
            json!({"type": "user", "uuid": "d", "parentUuid": "c", "isSidechain": true,
                   "message": {"content": "from a subagent"}}),
            json!({"type": "user", "uuid": "e", "parentUuid": "c", "timestamp": "2026-11-01T10:01:00Z",
                   "message": {"content": "a line taken back"}}),
            json!({"type": "user", "uuid": "f", "parentUuid": "c", "timestamp": "2026-11-01T10:02:00Z",
                   "message": {"content": [{"type": "text", "text": "Try again."}]}}),
            json!({"type": "assistant", "uuid": "g", "parentUuid": "f", "timestamp": "2026-11-01T10:02:01Z",
                   "message": {"id": "m2", "content": [{"type": "text", "text": "Done."}]}}),
            json!({"type": "assistant", "uuid": "h", "parentUuid": "g", "timestamp": "2026-11-01T10:02:02Z",
                   "message": {"id": "m2", "content": [{"type": "text", "text": "It builds."}]}}),
        ];
        let imported = read_records("s1".into(), &records).finish();
        assert_eq!(imported.title, "Fix the build");
        assert_eq!(imported.model.as_deref(), Some("claude-x"));
        let at = |minute: u64, second: u64| {
            1_793_527_200_000 + minute * 60_000 + second * 1000
        };
        assert_eq!(
            imported.entries,
            [
                ImportedEntry::User {
                    at: at(0, 0),
                    text: "Why does it fail?".into()
                },
                ImportedEntry::Assistant {
                    at: at(0, 1),
                    text: "Looking.".into()
                },
                ImportedEntry::Step {
                    at: at(0, 2),
                    summary: "cargo build".into(),
                    tool: "Bash".into()
                },
                ImportedEntry::User {
                    at: at(2, 0),
                    text: "Try again.".into()
                },
                ImportedEntry::Assistant {
                    at: at(2, 1),
                    text: "Done.\n\nIt builds.".into()
                },
            ]
        );
    }
}
