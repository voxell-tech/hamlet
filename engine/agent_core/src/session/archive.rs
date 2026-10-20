//! The sessions kept in a house's `sessions` folder: each log there,
//! summed up for a list of them.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use hamlet_model::log;
use hamlet_rules::vote::BALLOTS;

use super::{Transcript, Usage};

/// A session kept in a log.
#[derive(Clone, PartialEq, Debug)]
pub struct Past {
    pub path: PathBuf,
    /// The first line said in it.
    pub title: String,
    /// The user who said it.
    pub by: String,
    pub modified: SystemTime,
    /// The number it was archived under. `None` for a log in use.
    pub number: Option<String>,
    /// The chat shown can resume it.
    pub ours: bool,
    pub messages: usize,
    pub tokens: u64,
    /// Its text in lowercase, for the search.
    pub said: String,
}

impl Past {
    /// Every word of `query` is in it.
    pub fn matches(&self, query: &str) -> bool {
        query.to_lowercase().split_whitespace().all(|word| {
            self.said.contains(word)
                || self.title.to_lowercase().contains(word)
        })
    }
}

/// The digits between an archived log's name and `.jsonl`.
fn number(path: &Path) -> Option<String> {
    let stem = path.file_name()?.to_str()?.strip_suffix(".jsonl")?;
    let (_, number) = stem.rsplit_once('.')?;
    (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
        .then(|| number.to_string())
}

/// Summary of a log. `None` if nothing was said in it.
pub fn past(path: &Path, live: Option<&Path>) -> Option<Past> {
    let events = log::read_events(path).ok()?;
    // What was spent on a part since rewound was spent all the same.
    let tokens = Usage::from_log(&events).tokens();
    let events = log::standing(events);
    let mut title = None;
    let mut said = String::new();
    let mut messages = 0;
    for e in &events {
        let text = match e["type"].as_str() {
            // The hamlet's own prompts are not talk.
            Some("user") if e["user"] != "hamlet" => {
                let text = e["content"].as_str().unwrap_or_default();
                title.get_or_insert_with(|| {
                    let by = e["user"].as_str().unwrap_or_default();
                    (text.to_string(), by.to_string())
                });
                text
            }
            Some("assistant") => {
                e["message"]["content"].as_str().unwrap_or_default()
            }
            _ => continue,
        };
        if text.trim().is_empty() {
            continue;
        }
        messages += 1;
        said.push_str(&text.to_lowercase());
        said.push('\n');
    }
    let (title, by) = title?;
    let line = title.lines().find(|l| !l.trim().is_empty())?.trim();
    let number = number(path);
    let ours = number.as_ref().is_some_and(|number| {
        live.is_some_and(|live| {
            live.with_extension(format!("{number}.jsonl")) == path
        })
    });
    Some(Past {
        path: path.to_path_buf(),
        title: line.to_string(),
        by,
        modified: std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH),
        number,
        ours,
        messages,
        tokens,
        said,
    })
}

/// The sessions in `dir`, newest first. `live` is the log of the
/// chat shown.
pub fn pasts(dir: &Path, live: Option<&Path>) -> Vec<Past> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut pasts = entries
        .flatten()
        .map(|e| e.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(
                    // A ballot is not a session.
                    |name| {
                        name.ends_with(".jsonl")
                            && !name.starts_with(BALLOTS)
                    },
                )
        })
        .filter_map(|path| past(&path, live))
        .collect::<Vec<_>>();
    pasts.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| b.path.cmp(&a.path))
    });
    pasts
}

/// The transcript of the log at `path`, as it stands after any
/// rewind. Empty when the log cannot be read.
pub fn read(path: &Path) -> Transcript {
    let events = log::read_standing(path).unwrap_or_default();
    Transcript::from_log(&events)
}

/// Deletes the log of a past session.
pub fn delete(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}
