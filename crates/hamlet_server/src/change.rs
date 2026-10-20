//! The change an edit or a write makes to a file, as the gate shows
//! it (C30).

use hamlet_protocol::v0::{Change, ChangeLines, ChangeLinesMark};
use serde_json::Value;

/// The most lines of a change that are kept.
const KEPT: usize = 200;

/// The most characters kept of a line or a path.
const WIDE: usize = 240;

/// The lines that stay shown before the first line that differs and
/// after the last.
const AROUND: usize = 3;

/// The most pairs of lines compared one by one; a larger change is
/// its old lines out and its new lines in.
const PAIRS: usize = 1_000_000;

/// The change the call of tool `name` with `args` asks for, when it
/// is an edit or a write, from its arguments alone. A write changes
/// `before`, the file's text when it is there, or else makes the
/// file.
pub fn change_of(
    name: &str,
    args: &Value,
    before: Option<&str>,
) -> Option<Change> {
    let path = args.get("path")?.as_str()?;
    match name {
        "write" => {
            let content = args.get("content")?.as_str()?;
            Some(change_between(
                path,
                before.unwrap_or(""),
                content,
                before.is_none(),
            ))
        }
        "edit" => {
            let edits = args.get("edits")?.as_array()?;
            let mut old = Vec::new();
            let mut now = Vec::new();
            for edit in edits {
                old.push(edit.get("oldText")?.as_str()?);
                now.push(edit.get("newText")?.as_str()?);
            }
            Some(change_between(
                path,
                &old.join("\n"),
                &now.join("\n"),
                false,
            ))
        }
        _ => None,
    }
}

/// The change of `old` into `now` in the file at `path`, which
/// `created` says the change makes.
pub fn change_between(
    path: &str,
    old: &str,
    now: &str,
    created: bool,
) -> Change {
    let lines = compare(old, now);
    let differs = |(mark, _): &&(ChangeLinesMark, &str)| {
        *mark != ChangeLinesMark::Space
    };
    let first = lines.iter().position(|line| differs(&line));
    let last = lines.iter().rposition(|line| differs(&line));
    let shown = match (first, last) {
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
        path: clip(path, WIDE, false, false),
        created,
        lines: shown
            .iter()
            .take(KEPT)
            .map(|(mark, text)| ChangeLines {
                mark: mark.clone(),
                text: clip(text, WIDE, false, false),
            })
            .collect(),
        total: shown.len() as u64,
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

/// The lines of `old` and `now` in a diff's order: a line of both
/// once, a line of `old` alone before those of `now` in its place.
fn compare<'a>(
    old: &'a str,
    now: &'a str,
) -> Vec<(ChangeLinesMark, &'a str)> {
    use ChangeLinesMark::{Minus, Plus, Space};
    let a = lines_of(old);
    let b = lines_of(now);
    let mut head = 0;
    while head < a.len() && head < b.len() && a[head] == b[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < a.len() - head
        && tail < b.len() - head
        && a[a.len() - 1 - tail] == b[b.len() - 1 - tail]
    {
        tail += 1;
    }
    let out = &a[head..a.len() - tail];
    let into = &b[head..b.len() - tail];
    let mut lines = a[..head]
        .iter()
        .map(|line| (Space, *line))
        .collect::<Vec<_>>();
    if out.len() * into.len() > PAIRS {
        lines.extend(out.iter().map(|line| (Minus, *line)));
        lines.extend(into.iter().map(|line| (Plus, *line)));
    } else {
        // The longest run of lines both have, from each pair of
        // places on.
        let wide = into.len() + 1;
        let mut common = vec![0u32; (out.len() + 1) * wide];
        for i in (0..out.len()).rev() {
            for j in (0..into.len()).rev() {
                common[i * wide + j] = if out[i] == into[j] {
                    common[(i + 1) * wide + j + 1] + 1
                } else {
                    common[(i + 1) * wide + j]
                        .max(common[i * wide + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < out.len() && j < into.len() {
            if out[i] == into[j] {
                lines.push((Space, out[i]));
                i += 1;
                j += 1;
            } else if common[(i + 1) * wide + j]
                >= common[i * wide + j + 1]
            {
                lines.push((Minus, out[i]));
                i += 1;
            } else {
                lines.push((Plus, into[j]));
                j += 1;
            }
        }
        lines.extend(out[i..].iter().map(|line| (Minus, *line)));
        lines.extend(into[j..].iter().map(|line| (Plus, *line)));
    }
    lines.extend(
        a[a.len() - tail..].iter().map(|line| (Space, *line)),
    );
    lines
}

/// The lines of `text`: a last newline ends a line, and a carriage
/// return that ends one goes.
pub fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// An excerpt of at most `most` characters. Unless `flat` is false,
/// each run of white space is one space and the ends are trimmed;
/// unless `dots` is false, a cut excerpt ends with `...`.
pub fn clip(
    text: &str,
    most: usize,
    flat: bool,
    dots: bool,
) -> String {
    let shown = if flat {
        text.split(js_space)
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        text.to_owned()
    };
    match shown.char_indices().nth(most) {
        Some((cut, _)) => {
            format!(
                "{}{}",
                &shown[..cut],
                if dots { "..." } else { "" }
            )
        }
        None => shown,
    }
}

/// True for the characters JavaScript's `\s` and `trim` take as white
/// space.
pub(crate) fn js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(old: &str, now: &str) -> String {
        change_between("a.rs", old, now, false)
            .lines
            .iter()
            .map(|line| {
                let mark = match line.mark {
                    ChangeLinesMark::Space => ' ',
                    ChangeLinesMark::Plus => '+',
                    ChangeLinesMark::Minus => '-',
                };
                format!("{mark}{}", line.text)
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    #[test]
    fn marks_the_lines_that_differ_among_those_that_stay() {
        assert_eq!(marks("a\nb\nc", "a\nB\nc"), " a|-b|+B| c");
        assert_eq!(marks("a\nc", "a\nb\nc"), " a|+b| c");
        assert_eq!(marks("a\nb\nc", "a\nc"), " a|-b| c");
        assert_eq!(marks("a\nb\n", "a\nb"), "");
        assert_eq!(marks("", "a\nb"), "+a|+b");
        let change =
            change_between("a.rs", "a\nb\nc\nd", "a\nx\ny\nd", false);
        assert_eq!((change.added, change.removed), (2, 2));
        assert_eq!(change.count, "2 lines added, 2 removed");
        let count = |old, now, created| {
            change_between("a.rs", old, now, created).count
        };
        assert_eq!(count("", "a", true), "New file, 1 line");
        assert_eq!(count("a", "a", false), "No line changed");
        assert_eq!(count("a", "a\nb", false), "1 line added");
        assert_eq!(count("a\nb\nc", "a", false), "2 lines removed");
    }

    #[test]
    fn keeps_its_counts_and_first_lines_past_a_million_pairs() {
        let added = (0..50_000)
            .map(|n| format!("new {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let change = change_between(
            "a.rs",
            "1\n2\n3\n4\n5\nold\n7",
            &format!("1\n2\n3\n4\n5\n{added}\n7"),
            false,
        );
        assert_eq!((change.added, change.removed), (50_000, 1));
        assert_eq!(change.total, 50_005);
        assert_eq!(change.lines.len(), 200);
        assert_eq!(change.lines[0].text, "3");
        assert_eq!(change.lines[3].mark, ChangeLinesMark::Minus);
    }
}
