//! Conversations of other programs, found on this machine or in a
//! file they exported, each read as an [`Imported`] for the server to
//! bring in (C35), and the MCP servers they list (C36).

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use hamlet_protocol::client::{
    ImportSource, Imported, ImportedEntry,
};
use serde_json::Value;

mod claude_code;
mod codex;
mod exports;
mod mcp;
mod pi;

pub use mcp::{FoundServer, McpSource, Unrunnable, mcp_servers};

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
/// The most conversations of a project, the newest, listed.
pub const MOST_LISTED: usize = 50;
/// The most conversations of a project, the newest, brought in unless
/// the chief picks others.
pub const MOST_PICKED: usize = 20;

/// A project: the folder conversations worked in, or the program of
/// those with none, as an export's are.
#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    /// The root of its git repository, else the folder itself.
    pub folder: Option<String>,
    pub is_git: bool,
    /// Its newest conversations, at most [`MOST_LISTED`], newest
    /// first.
    pub conversations: Vec<Found>,
}

impl Project {
    /// Whether to bring it in unless the chief says not to: a git
    /// repository with three conversations or more.
    pub fn is_suggested(&self) -> bool {
        self.is_git && self.conversations.len() >= 3
    }

    /// Its newest [`MOST_PICKED`] conversations, brought in unless
    /// the chief picks others.
    pub fn picked(&self) -> &[Found] {
        &self.conversations
            [..self.conversations.len().min(MOST_PICKED)]
    }

    pub fn modified(&self) -> SystemTime {
        self.conversations
            .first()
            .map_or(SystemTime::UNIX_EPOCH, |found| found.modified)
    }
}

/// The projects of the conversations `found` since `since`, the
/// newest first, each with its newest [`MOST_LISTED`] conversations.
/// A linked git worktree, a folder under Downloads, Codex's scratch
/// folders and the home folder are no project.
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
        project.conversations.truncate(MOST_LISTED);
    }
    projects.sort_by_key(|project| {
        (!project.is_git, std::cmp::Reverse(project.modified()))
    });
    projects
}

/// The root of the git repository `folder` is in, and true; else
/// `folder` and false. `None` for a linked worktree or a folder that
/// is gone.
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
            return (is_worktree(&git) == Some(false))
                .then(|| (at.to_owned(), true));
        }
    }
    Some((folder.to_owned(), false))
}

/// Whether the `.git` file `git` is a linked worktree's, whose git
/// folder is under its repository's `worktrees`, where a submodule's
/// is under `modules`.
fn is_worktree(git: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(git).ok()?;
    let folder = Path::new(text.strip_prefix("gitdir:")?.trim());
    Some(
        folder
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "worktrees"),
    )
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
            let conversations = exports::conversations(&found.path)?;
            exports::read(
                &conversations,
                &found.path,
                &found.session,
            )?
        }
    };
    Ok(read.finish())
}

/// The conversations `found`, each as [`read`] reads it, an export
/// read once for all of its conversations.
pub fn read_all(found: &[Found]) -> Vec<Result<Imported>> {
    let mut exported = HashMap::<&Path, Result<Vec<Value>>>::new();
    found
        .iter()
        .map(|found| match found.source {
            ImportSource::Chatgpt | ImportSource::Claude => {
                let conversations =
                    exported.entry(&found.path).or_insert_with(
                        || exports::conversations(&found.path),
                    );
                match conversations {
                    Ok(conversations) => exports::read(
                        conversations,
                        &found.path,
                        &found.session,
                    )
                    .map(Draft::finish),
                    Err(error) => Err(anyhow!("{error:#}")),
                }
            }
            _ => read(found),
        })
        .collect()
}

/// A conversation as a source reads it, before it is cut.
struct Draft {
    source: ImportSource,
    session: String,
    title: Option<String>,
    model: Option<String>,
    entries: Vec<ImportedEntry>,
    /// Where each of the person's turns began in `entries`, those
    /// with no words included.
    turns: Vec<usize>,
}

impl Draft {
    fn new(source: ImportSource, session: String) -> Self {
        Self {
            source,
            session,
            title: None,
            model: None,
            entries: Vec::new(),
            turns: Vec::new(),
        }
    }

    /// Begins one of the person's turns, which [`Self::take_back`]
    /// counts.
    fn turn(&mut self) {
        self.turns.push(self.entries.len());
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
    /// came between, not even a turn without words.
    fn assistant(&mut self, at: u64, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let turned = self.turns.last() == Some(&self.entries.len());
        if !turned
            && let Some(ImportedEntry::Assistant {
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

    /// Takes back the newest `turns` turns begun with [`Self::turn`]
    /// and everything after them.
    fn take_back(&mut self, turns: usize) {
        let kept = self.turns.len().saturating_sub(turns);
        if let Some(&start) = self.turns.get(kept) {
            self.entries.truncate(start);
            self.turns.truncate(kept);
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
    let mut over =
        entries.len().saturating_sub(Imported::MOST_ENTRIES);
    entries.retain(|entry| {
        let goes =
            over > 0 && matches!(entry, ImportedEntry::Step { .. });
        over -= usize::from(goes);
        !goes
    });
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

/// The JSON records of a file of one per line, those not JSON left
/// out; the first `limit` bytes of it when given, where the line the
/// limit cuts is left out too.
fn records(path: &Path, limit: Option<u64>) -> Result<Vec<Value>> {
    let file = File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut records = Vec::new();
    for line in
        BufReader::new(file.take(limit.unwrap_or(u64::MAX))).lines()
    {
        let line = match line {
            Ok(line) => line,
            // The limit may cut a letter in two.
            Err(_) if limit.is_some() => break,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("reading {}", path.display())
                });
            }
        };
        if let Ok(record) = serde_json::from_str(&line) {
            records.push(record);
        }
    }
    Ok(records)
}

/// What `pick` finds in the records at the start of `path`, its first
/// [`HEAD`] bytes, else in all of them, for a file whose first record
/// of note is longer than that.
fn listing<T>(
    path: &Path,
    pick: impl Fn(&[Value]) -> Option<T>,
) -> Option<T> {
    if let Some(found) = pick(&records(path, Some(HEAD)).ok()?) {
        return Some(found);
    }
    let longer = std::fs::metadata(path).ok()?.len() > HEAD;
    longer.then(|| pick(&records(path, None).ok()?)).flatten()
}

/// An RFC 3339 time in milliseconds since the epoch.
fn time(stamp: &Value) -> Option<u64> {
    let stamp = stamp.as_str()?.parse::<jiff::Timestamp>().ok()?;
    u64::try_from(stamp.as_millisecond()).ok()
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

    #[test]
    fn takes_back_a_wordless_turn_with_its_answer() {
        let mut draft =
            Draft::new(ImportSource::Codex, "session".into());
        draft.turn();
        draft.user(0, "0");
        draft.assistant(1, "kept");
        draft.turn();
        draft.assistant(2, "taken back");
        draft.take_back(1);
        assert_eq!(
            draft.entries,
            [
                user(0),
                ImportedEntry::Assistant {
                    at: 1,
                    text: "kept".into(),
                },
            ]
        );
    }
}
