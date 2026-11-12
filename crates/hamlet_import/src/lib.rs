//! Conversations of other programs found on this machine, and each
//! read as an [`Imported`] for the server to bring in (C35).

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use hamlet_protocol::client::{
    ImportSource, Imported, ImportedEntry,
};

mod claude_code;

/// The characters of a tool call's summary and of a title.
const SUMMARY: usize = 200;

/// A conversation found on this machine.
#[derive(Clone, Debug)]
pub struct Found {
    pub source: ImportSource,
    /// The source's own id of it.
    pub session: String,
    pub title: String,
    /// The folder it worked in.
    pub folder: Option<String>,
    pub modified: SystemTime,
    pub path: PathBuf,
}

/// Every conversation found on this machine, newest first.
pub fn find() -> Vec<Found> {
    let mut found = claude_code::find();
    found.sort_by_key(|found| std::cmp::Reverse(found.modified));
    found
}

/// The conversation `found`, as the server brings it in.
pub fn read(found: &Found) -> Result<Imported> {
    match found.source {
        ImportSource::ClaudeCode => claude_code::read(&found.path),
        source => anyhow::bail!(
            "reading {} conversations is not supported yet",
            source.name()
        ),
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
