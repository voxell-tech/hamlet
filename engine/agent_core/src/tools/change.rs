//! The change an `edit` call asks for, as lines taken out and put
//! in.

use serde_json::Value;

/// The most lines of a change that are kept.
const KEPT: usize = 200;

/// The most of a line, a path or a failure that is kept, in
/// characters.
const WIDE: usize = 240;

/// The lines that stay shown before the first line that differs and
/// after the last.
const AROUND: usize = 3;

/// The most pairs of lines compared one by one. A larger change is
/// its old lines out and its new lines in.
const PAIRS: usize = 1_000_000;

/// The part a line has in a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// In the file before and after.
    Same,
    Out,
    In,
}

/// A line of a change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub mark: Mark,
    pub text: String,
}

/// The change one `edit` call asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The file, as the call named it.
    pub path: String,
    /// True when the call makes the file.
    pub created: bool,
    /// The first lines of the change.
    pub lines: Vec<Line>,
    /// The lines of the whole change, those that stay among them.
    pub total: usize,
    pub added: usize,
    pub removed: usize,
    /// The answer to the call, when it changed nothing.
    pub failure: Option<String>,
}

fn clip(text: &str) -> String {
    text.chars().take(WIDE).collect()
}

/// The first line of a failed call's answer.
pub(crate) fn failure(output: &str) -> String {
    let line = output
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim();
    clip(line.strip_prefix("Error: ").unwrap_or(line))
}

/// The lines of `old` and `new` in the order of a diff: a line of
/// both once, a line of `old` alone before the lines of `new` put in
/// its place.
fn compare<'a>(old: &'a str, new: &'a str) -> Vec<(Mark, &'a str)> {
    let (a, b) = (
        old.lines().collect::<Vec<_>>(),
        new.lines().collect::<Vec<_>>(),
    );
    let head = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b, end) = (
        &a[head..a.len() - tail],
        &b[head..b.len() - tail],
        &a[a.len() - tail..],
    );
    let mut lines = old
        .lines()
        .take(head)
        .map(|line| (Mark::Same, line))
        .collect::<Vec<_>>();
    if a.len().saturating_mul(b.len()) > PAIRS {
        lines.extend(a.iter().map(|line| (Mark::Out, *line)));
        lines.extend(b.iter().map(|line| (Mark::In, *line)));
    } else {
        // The longest run of lines both have, from each pair of
        // places to the end.
        let wide = b.len() + 1;
        let mut common = vec![0u32; (a.len() + 1) * wide];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                common[i * wide + j] = if a[i] == b[j] {
                    common[(i + 1) * wide + j + 1] + 1
                } else {
                    common[(i + 1) * wide + j]
                        .max(common[i * wide + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            if a[i] == b[j] {
                lines.push((Mark::Same, a[i]));
                (i, j) = (i + 1, j + 1);
            } else if common[(i + 1) * wide + j]
                >= common[i * wide + j + 1]
            {
                lines.push((Mark::Out, a[i]));
                i += 1;
            } else {
                lines.push((Mark::In, b[j]));
                j += 1;
            }
        }
        lines.extend(a[i..].iter().map(|line| (Mark::Out, *line)));
        lines.extend(b[j..].iter().map(|line| (Mark::In, *line)));
    }
    lines.extend(end.iter().map(|line| (Mark::Same, *line)));
    lines
}

impl Change {
    /// The change the call of the tool `name` with `arguments` asks
    /// for, if it is an edit.
    pub fn of(name: &str, arguments: &str) -> Option<Self> {
        if name != "edit" {
            return None;
        }
        let arguments =
            serde_json::from_str::<Value>(arguments).ok()?;
        let old = arguments["old"].as_str().unwrap_or_default();
        Some(Change::between(
            arguments["path"].as_str()?,
            old,
            arguments["new"].as_str()?,
        ))
    }

    /// The change of `old` into `new` in the file at `path`. An
    /// empty `old` makes the file.
    pub fn between(path: &str, old: &str, new: &str) -> Self {
        let lines = compare(old, new);
        let differs = |(mark, _): &(Mark, &str)| *mark != Mark::Same;
        let first = lines.iter().position(differs);
        let last = lines.iter().rposition(differs);
        let shown = match (first, last) {
            (Some(first), Some(last)) => {
                first.saturating_sub(AROUND)
                    ..(last + 1 + AROUND).min(lines.len())
            }
            _ => 0..0,
        };
        let count = |which: Mark| {
            lines.iter().filter(|(mark, _)| *mark == which).count()
        };
        Change {
            path: clip(path),
            created: old.is_empty(),
            total: shown.len(),
            added: count(Mark::In),
            removed: count(Mark::Out),
            lines: lines[shown]
                .iter()
                .take(KEPT)
                .map(|(mark, text)| Line {
                    mark: *mark,
                    text: clip(text),
                })
                .collect(),
            failure: None,
        }
    }

    /// The count of its lines, in words.
    pub fn count(&self) -> String {
        let lines = |n: usize| match n {
            1 => "1 line".to_string(),
            n => format!("{n} lines"),
        };
        match (self.created, self.added, self.removed) {
            (true, added, _) => format!("New file, {}", lines(added)),
            (false, 0, 0) => "No line changed".to_string(),
            (false, added, 0) => format!("{} added", lines(added)),
            (false, 0, removed) => {
                format!("{} removed", lines(removed))
            }
            (false, added, removed) => {
                format!("{} added, {removed} removed", lines(added))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(old: &str, new: &str) -> String {
        Change::between("a.rs", old, new)
            .lines
            .iter()
            .map(|line| match line.mark {
                Mark::Same => format!(" {}", line.text),
                Mark::Out => format!("-{}", line.text),
                Mark::In => format!("+{}", line.text),
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    #[test]
    fn a_change_is_the_lines_that_differ_among_those_that_stay() {
        // A line replaced, one put in and one taken out.
        assert_eq!(marks("a\nb\nc", "a\nB\nc"), " a|-b|+B| c");
        assert_eq!(marks("a\nc", "a\nb\nc"), " a|+b| c");
        assert_eq!(marks("a\nb\nc", "a\nc"), " a|-b| c");
        assert_eq!(marks("a\nb\n", "a\nb"), "");
        assert_eq!(marks("", "a\nb"), "+a|+b");
        let change =
            Change::between("a.rs", "a\nb\nc\nd", "a\nx\ny\nd");
        assert_eq!((change.added, change.removed), (2, 2));
        assert_eq!(change.count(), "2 lines added, 2 removed");
        assert!(!change.created);
        assert_eq!(
            Change::between("a.rs", "", "a").count(),
            "New file, 1 line"
        );
    }

    #[test]
    fn a_long_change_keeps_its_counts_and_its_first_lines() {
        let old = "1\n2\n3\n4\n5\nold\n7";
        let new = (0..50_000)
            .map(|n| format!("new {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let change = Change::between(
            "a.rs",
            old,
            &format!("1\n2\n3\n4\n5\n{new}\n7"),
        );
        assert_eq!((change.added, change.removed), (50_000, 1));
        // Three lines that stay before what differs, and the one
        // there is after.
        assert_eq!(change.total, 50_005);
        assert_eq!(change.lines.len(), KEPT);
        assert_eq!(change.lines[0].text, "3");
        assert_eq!(change.lines[3].mark, Mark::Out);
        // Arguments that are no edit, or not whole.
        assert_eq!(
            Change::of("bash", r#"{"path":"a","new":"b"}"#),
            None
        );
        assert_eq!(Change::of("edit", r#"{"path":"a","new":"#), None);
        assert_eq!(
            Change::of("edit", r#"{"path":1,"new":"b"}"#),
            None
        );
    }
}
