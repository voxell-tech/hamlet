//! Claude Code's sessions: one JSON line per record in
//! `<config>/projects/<folder>/<session>.jsonl`, where `<config>` is
//! `CLAUDE_CONFIG_DIR` or `~/.claude`.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use hamlet_protocol::client::{
    ImportSource, Imported, ImportedEntry,
};
use serde_json::Value;

use crate::{Found, keep, summary};

/// The bytes read from the start of a session to list it.
const HEAD: u64 = 256 << 10;
/// Words Claude Code writes as the person's that the person did not
/// type.
const NOT_TYPED: [&str; 3] =
    ["<command-", "<local-command-", "[Request interrupted"];

fn projects() -> Option<PathBuf> {
    let config = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|config| !config.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::home_dir().map(|home| home.join(".claude"))
        })?;
    Some(config.join("projects"))
}

pub(crate) fn find() -> Vec<Found> {
    let Some(projects) = projects() else {
        return Vec::new();
    };
    let Ok(folders) = std::fs::read_dir(projects) else {
        return Vec::new();
    };
    folders
        .flatten()
        .filter_map(|folder| std::fs::read_dir(folder.path()).ok())
        .flatten()
        .flatten()
        .map(|file| file.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "jsonl")
        })
        .filter_map(|path| listed(&path))
        .collect()
}

/// The session at `path` as a list shows it, from its first records;
/// `None` for one with no message of the person's there.
fn listed(path: &Path) -> Option<Found> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let head = BufReader::new(File::open(path).ok()?.take(HEAD));
    let mut folder = None;
    let mut title = None;
    let mut first = None;
    for line in head.lines() {
        let Ok(record) = serde_json::from_str::<Value>(&line.ok()?)
        else {
            continue;
        };
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
        modified,
        path: path.to_owned(),
    })
}

pub(crate) fn read(path: &Path) -> Result<Imported> {
    let file = File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let records = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .collect::<Vec<_>>();
    let session = path
        .file_stem()
        .context("a session file has a name")?
        .to_string_lossy()
        .into_owned();
    Ok(imported(session, &records))
}

/// The session `session` of `records`, its lines in order.
fn imported(session: String, records: &[Value]) -> Imported {
    let shown = shown(records);
    let mut entries = Vec::new();
    let mut model = None;
    let mut title = None;
    let mut began = None;
    let mut answered = None::<String>;
    for record in records {
        if let Some(custom) = record["customTitle"].as_str() {
            title = Some(custom.to_owned());
        }
        let Some(uuid) = record["uuid"].as_str() else {
            continue;
        };
        if !is_message(record) || !shown.contains(uuid) {
            continue;
        }
        let at = time(&record["timestamp"]).unwrap_or(0);
        began.get_or_insert(at);
        let message = &record["message"];
        if record["type"] == "user" {
            if let Some(text) = user_text(record) {
                entries.push(ImportedEntry::User { at, text });
            }
            answered = None;
            continue;
        }
        if let Some(name) = message["model"].as_str()
            && !name.starts_with('<')
        {
            model = Some(name.to_owned());
        }
        let id = message["id"].as_str().map(str::to_owned);
        for block in
            message["content"].as_array().into_iter().flatten()
        {
            match block["type"].as_str() {
                Some("text") => {
                    let text =
                        block["text"].as_str().unwrap_or_default();
                    if text.trim().is_empty() {
                        continue;
                    }
                    // Claude Code keeps each block of an answer as a
                    // record of its own.
                    if let (
                        Some(ImportedEntry::Assistant {
                            text: so_far,
                            ..
                        }),
                        true,
                    ) = (
                        entries.last_mut(),
                        id.is_some() && answered == id,
                    ) {
                        so_far.push_str("\n\n");
                        so_far.push_str(text);
                    } else {
                        entries.push(ImportedEntry::Assistant {
                            at,
                            text: text.to_owned(),
                        });
                    }
                    answered.clone_from(&id);
                }
                Some("tool_use") => {
                    entries.push(ImportedEntry::Step {
                        at,
                        summary: call_summary(&block["input"]),
                        tool: block["name"]
                            .as_str()
                            .filter(|name| !name.is_empty())
                            .unwrap_or("tool")
                            .to_owned(),
                    });
                    answered = None;
                }
                _ => {}
            }
        }
    }
    let first = entries.iter().find_map(|entry| match entry {
        ImportedEntry::User { text, .. } => Some(text.clone()),
        _ => None,
    });
    let title =
        summary(title.or(first).as_deref().unwrap_or("Claude Code"));
    let (entries, left_out) = keep(entries);
    Imported {
        began: began.unwrap_or(0),
        entries,
        left_out,
        model: model.map(|model| summary(&model)),
        session,
        source: ImportSource::ClaudeCode,
        title,
    }
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

/// A tool call's input in a line: the field that names what it acts
/// on, else the whole input.
fn call_summary(input: &Value) -> String {
    const NAMES: [&str; 7] = [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
    ];
    match NAMES.iter().find_map(|name| input[name].as_str()) {
        Some(named) => summary(named),
        None => summary(&input.to_string()),
    }
}

fn time(stamp: &Value) -> Option<u64> {
    let stamp =
        chrono::DateTime::parse_from_rfc3339(stamp.as_str()?).ok()?;
    u64::try_from(stamp.timestamp_millis()).ok()
}

#[cfg(test)]
mod tests {
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
        let imported = imported("s1".into(), &records);
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
