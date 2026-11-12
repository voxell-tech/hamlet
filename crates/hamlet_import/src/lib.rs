//! Conversations of other programs, found on this machine or in a
//! file they exported, each read as an [`Imported`] for the server to
//! bring in (C35).

use std::fs::File;
use std::io::{BufRead, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use hamlet_protocol::client::{
    ImportSource, Imported, ImportedEntry,
};
use serde_json::Value;

mod claude_code;
mod codex;
mod exports;
mod pi;

/// The characters of a tool call's summary and of a title.
const SUMMARY: usize = 200;
/// The bytes read from the start of a session file to list it.
const HEAD: u64 = 256 << 10;

/// A conversation found on this machine or in an exported file.
#[derive(Clone, Debug)]
pub struct Found {
    pub source: ImportSource,
    /// The source's own id of it.
    pub session: String,
    pub title: String,
    /// The folder it worked in.
    pub folder: Option<String>,
    pub modified: SystemTime,
    /// Its session file, or the exported file that holds it.
    pub path: PathBuf,
}

/// Every conversation of Claude Code, Codex and pi found on this
/// machine, newest first.
pub fn find() -> Vec<Found> {
    let mut found = claude_code::find();
    found.extend(codex::find());
    found.extend(pi::find());
    found.sort_by_key(|found| std::cmp::Reverse(found.modified));
    found
}

/// How far back the conversations of a project reach.
pub const RECENT: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// A project: the folder conversations worked in, or the program of
/// those with none, as an export's are.
#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    /// The root of its git repository, else the folder itself.
    pub folder: Option<String>,
    pub is_git: bool,
    /// Its conversations, newest first.
    pub conversations: Vec<Found>,
}

impl Project {
    /// Whether to bring it in unless the chief says not to: a git
    /// repository with three conversations or more.
    pub fn is_suggested(&self) -> bool {
        self.is_git && self.conversations.len() >= 3
    }

    pub fn modified(&self) -> SystemTime {
        self.conversations
            .first()
            .map_or(SystemTime::UNIX_EPOCH, |found| found.modified)
    }
}

/// The projects of the conversations `found` since `since`, the
/// newest first. A linked git worktree, a folder under Downloads,
/// Codex's scratch folders and the home folder are no project.
pub fn projects(
    found: Vec<Found>,
    since: SystemTime,
) -> Vec<Project> {
    let home = std::env::home_dir();
    let mut projects = Vec::<Project>::new();
    for found in found {
        if found.modified < since {
            continue;
        }
        let (name, folder, is_git) = match &found.folder {
            Some(folder) => {
                let Some((root, is_git)) = root_of(Path::new(folder))
                else {
                    continue;
                };
                let skipped = home.as_deref().is_some_and(|home| {
                    root == home
                        || root.starts_with(home.join("Downloads"))
                        || root
                            .starts_with(home.join("Documents/Codex"))
                });
                if skipped {
                    continue;
                }
                let name = root.file_name().map_or_else(
                    || root.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                (name, Some(root.display().to_string()), is_git)
            }
            None => (found.source.name().to_owned(), None, false),
        };
        match projects.iter_mut().find(|project| {
            project.folder == folder
                && (folder.is_some() || project.name == name)
        }) {
            Some(project) => project.conversations.push(found),
            None => projects.push(Project {
                name,
                folder,
                is_git,
                conversations: vec![found],
            }),
        }
    }
    for project in &mut projects {
        project
            .conversations
            .sort_by_key(|found| std::cmp::Reverse(found.modified));
    }
    projects.sort_by_key(|project| {
        (!project.is_git, std::cmp::Reverse(project.modified()))
    });
    projects
}

/// The root of the git repository `folder` is in, and true; else
/// `folder` and false. `None` for a linked worktree, whose `.git` is
/// a file, or a folder that is gone.
fn root_of(folder: &Path) -> Option<(PathBuf, bool)> {
    if !folder.is_dir() {
        return None;
    }
    for at in folder.ancestors() {
        let git = at.join(".git");
        if git.is_dir() {
            return Some((at.to_owned(), true));
        }
        if git.is_file() {
            return None;
        }
    }
    Some((folder.to_owned(), false))
}

/// Every conversation in a file ChatGPT or Claude exported, as its
/// zip or its `conversations.json`, newest first.
pub fn find_in(path: &Path) -> Result<Vec<Found>> {
    let mut found = exports::find(path)?;
    found.sort_by_key(|found| std::cmp::Reverse(found.modified));
    Ok(found)
}

/// The conversation `found`, as the server brings it in.
pub fn read(found: &Found) -> Result<Imported> {
    let read = match found.source {
        ImportSource::ClaudeCode => claude_code::read(&found.path)?,
        ImportSource::Codex => codex::read(&found.path)?,
        ImportSource::Pi => pi::read(&found.path)?,
        ImportSource::Chatgpt | ImportSource::Claude => {
            exports::read(&found.path, &found.session)?
        }
    };
    Ok(read.finish())
}

/// A conversation as a source reads it, before it is cut.
struct Draft {
    source: ImportSource,
    session: String,
    title: Option<String>,
    model: Option<String>,
    entries: Vec<ImportedEntry>,
}

impl Draft {
    fn new(source: ImportSource, session: String) -> Self {
        Self {
            source,
            session,
            title: None,
            model: None,
            entries: Vec::new(),
        }
    }

    fn user(&mut self, at: u64, text: &str) {
        let text = text.trim();
        if !text.is_empty() {
            self.entries.push(ImportedEntry::User {
                at,
                text: text.to_owned(),
            });
        }
    }

    /// Adds an answer's words, to the answer before them when nothing
    /// came between.
    fn assistant(&mut self, at: u64, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if let Some(ImportedEntry::Assistant {
            text: so_far, ..
        }) = self.entries.last_mut()
        {
            so_far.push_str("\n\n");
            so_far.push_str(text);
        } else {
            self.entries.push(ImportedEntry::Assistant {
                at,
                text: text.to_owned(),
            });
        }
    }

    fn step(&mut self, at: u64, tool: &str, input: &Value) {
        let tool = if tool.is_empty() { "tool" } else { tool };
        self.entries.push(ImportedEntry::Step {
            at,
            summary: call_summary(input),
            tool: summary(tool),
        });
    }

    fn finish(self) -> Imported {
        let first =
            self.entries.iter().find_map(|entry| match entry {
                ImportedEntry::User { text, .. } => {
                    Some(text.as_str())
                }
                _ => None,
            });
        let title = summary(
            self.title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
                .or(first)
                .unwrap_or(self.source.name()),
        );
        let began =
            self.entries.first().map_or(0, |entry| match entry {
                ImportedEntry::User { at, .. }
                | ImportedEntry::Assistant { at, .. }
                | ImportedEntry::Step { at, .. } => *at,
            });
        let (entries, left_out) = keep(self.entries);
        Imported {
            began,
            entries,
            left_out,
            model: self
                .model
                .filter(|model| !model.is_empty())
                .map(|model| summary(&model)),
            session: self.session,
            source: self.source,
            title,
        }
    }
}

/// `entries` cut to their first message and the newest
/// [`Imported::NEWEST`], with the tool calls between those kept,
/// and how many messages were left out.
fn keep(
    mut entries: Vec<ImportedEntry>,
) -> (Vec<ImportedEntry>, u64) {
    let messages = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            !matches!(entry, ImportedEntry::Step { .. })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut left_out = 0;
    if messages.len() > Imported::NEWEST + 1 {
        let first = messages[0];
        let newest = messages[messages.len() - Imported::NEWEST];
        left_out = (messages.len() - Imported::NEWEST - 1) as u64;
        entries.drain(first + 1..newest);
    }
    // Past the most entries, the oldest tool calls go first.
    let mut index = 0;
    while entries.len() > Imported::MOST_ENTRIES
        && index < entries.len()
    {
        if matches!(entries[index], ImportedEntry::Step { .. }) {
            entries.remove(index);
        } else {
            index += 1;
        }
    }
    (entries, left_out)
}

/// `text` cut to [`SUMMARY`] characters, on one line.
fn summary(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(SUMMARY) {
        Some((end, _)) => line[..end].to_owned(),
        None => line,
    }
}

/// A tool call's input in a line: the field that names what it acts
/// on, else the whole input. Input given as JSON text is read first.
fn call_summary(input: &Value) -> String {
    const NAMES: [&str; 8] = [
        "command",
        "cmd",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
    ];
    if let Value::String(text) = input {
        return match serde_json::from_str::<Value>(text) {
            Ok(parsed @ Value::Object(_)) => call_summary(&parsed),
            _ => summary(text),
        };
    }
    let named = NAMES.iter().find_map(|name| match &input[name] {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    });
    summary(&named.unwrap_or_else(|| input.to_string()))
}

/// The JSON records of a file of one per line, the unreadable ones
/// left out; the first `limit` bytes of it when given.
fn records(path: &Path, limit: Option<u64>) -> Result<Vec<Value>> {
    let file = File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    Ok(BufReader::new(file.take(limit.unwrap_or(u64::MAX)))
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .collect())
}

/// An RFC 3339 time in milliseconds since the epoch.
fn time(stamp: &Value) -> Option<u64> {
    let stamp =
        chrono::DateTime::parse_from_rfc3339(stamp.as_str()?).ok()?;
    u64::try_from(stamp.timestamp_millis()).ok()
}

/// The modified time of `path`.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// The folder `variable` names, else `default` under the home folder.
fn folder(variable: &str, default: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::home_dir().map(|home| home.join(default))
        })
}

/// The files with extension `extension` in the folders under
/// `folder`, `depth` levels down.
fn files_under(
    folder: &Path,
    depth: usize,
    extension: &str,
) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .flat_map(|path| {
            if depth == 0 {
                let matches = path
                    .extension()
                    .is_some_and(|ext| ext == extension)
                    && path.is_file();
                matches
                    .then_some(path)
                    .into_iter()
                    .collect::<Vec<_>>()
            } else if path.is_dir() {
                files_under(&path, depth - 1, extension)
            } else {
                Vec::new()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(at: u64) -> ImportedEntry {
        ImportedEntry::User {
            at,
            text: at.to_string(),
        }
    }

    fn step(at: u64) -> ImportedEntry {
        ImportedEntry::Step {
            at,
            summary: String::new(),
            tool: "Bash".into(),
        }
    }

    #[test]
    fn keeps_the_first_message_and_the_newest() {
        let count = Imported::NEWEST as u64 + 5;
        let mut entries = vec![user(0), step(0)];
        entries
            .extend((1..count).flat_map(|at| [step(at), user(at)]));
        let (kept, left_out) = keep(entries);
        assert_eq!(left_out, 4);
        let ats = kept
            .iter()
            .filter_map(|entry| match entry {
                ImportedEntry::User { at, .. } => Some(*at),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(ats.len(), Imported::NEWEST + 1);
        assert_eq!(ats[..2], [0, 5]);
        assert_eq!(ats.last(), Some(&(count - 1)));
        // What answered a message left out goes with it.
        assert!(matches!(kept[1], ImportedEntry::User { at: 5, .. }));
    }
}
