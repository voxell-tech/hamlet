//! An edit's change as a chief is shown it while it waits: the lines
//! from a few before the first that differs to a few after the last,
//! each cut to a width.

use hamlet_messages::workspace::files::EditDiff;
use hamlet_protocol::client::{Change, ChangeLines, ChangeLinesMark};

/// The most lines of a change that are kept.
const KEPT: usize = 200;

/// The most characters kept of a line or a path.
const WIDE: usize = 240;

/// The lines that stay shown before the first line that differs and
/// after the last.
const AROUND: usize = 3;

/// The change of `diff`, whose wait's `subject` names its path on its
/// first line and, for a write that makes the file, "A new file:" on
/// the next.
pub(super) fn shown(diff: &EditDiff, subject: &str) -> Change {
    let mut words = subject.lines();
    let path = words.next().unwrap_or_default();
    let created = words.next() == Some("A new file:");
    // The patch's two header lines, its hunks' ranges and its notes
    // of a last line with no newline are not lines of the file.
    let lines = diff
        .patch
        .lines()
        .skip(2)
        .filter(|line| {
            !line.starts_with("@@") && !line.starts_with('\\')
        })
        .map(|line| match line.split_at_checked(1) {
            Some(("+", text)) => (ChangeLinesMark::Plus, text),
            Some(("-", text)) => (ChangeLinesMark::Minus, text),
            Some((_, text)) => (ChangeLinesMark::Space, text),
            None => (ChangeLinesMark::Space, line),
        })
        .collect::<Vec<_>>();
    let differs = |(mark, _): &&(ChangeLinesMark, &str)| {
        *mark != ChangeLinesMark::Space
    };
    let first = lines.iter().position(|line| differs(&line));
    let last = lines.iter().rposition(|line| differs(&line));
    let kept = match (first, last) {
        (Some(first), Some(last)) => {
            &lines[first.saturating_sub(AROUND)
                ..(last + 1 + AROUND).min(lines.len())]
        }
        _ => &lines[..0],
    };
    let count_of = |wanted: ChangeLinesMark| {
        lines.iter().filter(|(mark, _)| *mark == wanted).count()
    };
    let added = count_of(ChangeLinesMark::Plus);
    let removed = count_of(ChangeLinesMark::Minus);
    Change {
        path: clip(path),
        created,
        lines: kept
            .iter()
            .take(KEPT)
            .map(|(mark, text)| ChangeLines {
                mark: mark.clone(),
                text: clip(text),
            })
            .collect(),
        total: kept.len() as u64,
        added: added as u64,
        removed: removed as u64,
        count: count(created, added, removed),
    }
}

/// The count of a change's lines, in words.
fn count(created: bool, added: usize, removed: usize) -> String {
    let lines = |n: usize| {
        if n == 1 {
            "1 line".to_owned()
        } else {
            format!("{n} lines")
        }
    };
    if created {
        format!("New file, {}", lines(added))
    } else if added == 0 && removed == 0 {
        "No line changed".into()
    } else if removed == 0 {
        format!("{} added", lines(added))
    } else if added == 0 {
        format!("{} removed", lines(removed))
    } else {
        format!("{} added, {removed} removed", lines(added))
    }
}

/// At most [`WIDE`] characters of `text`.
fn clip(text: &str) -> String {
    match text.char_indices().nth(WIDE) {
        Some((end, _)) => text[..end].to_owned(),
        None => text.to_owned(),
    }
}
