//! Bounded, full-file syntax and paired-line word decorations.

use std::ops::Range;
use std::path::Path;
use std::time::{Duration, Instant};

use similar::{ChangeTag, TextDiff};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};

use super::{Diff, Mark};

const SYNTAX_BYTES: usize = 4 * 1024 * 1024;
const SYNTAX_LINE: usize = 8192;
const SYNTAX_SPANS: usize = 250_000;
const WORD_LINE: usize = 4096;
const LINE_SPANS: usize = 128;

/// A syntax role, resolved against the UI theme when drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Plain,
    Comment,
    String,
    Keyword,
    Constant,
    Name,
}

/// A UTF-8 byte range with independent syntax and change emphasis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: Kind,
    pub changed: bool,
}

/// Decorations indexed by full-context diff line, separately for each
/// revision.
#[derive(Clone, Debug, Default)]
pub struct Highlighting {
    pub old: Vec<Vec<Span>>,
    pub new: Vec<Vec<Span>>,
    pub limited: bool,
}

impl Highlighting {
    /// Prepares decorations off-thread. Unknown extensions stay
    /// plain.
    pub fn new(
        diff: &Diff,
        old_path: &Path,
        new_path: &Path,
    ) -> Self {
        if diff.lines.is_empty() {
            return Self::default();
        }
        let mut result = Self {
            old: vec![Vec::new(); diff.lines.len()],
            new: vec![Vec::new(); diff.lines.len()],
            limited: false,
        };
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let scopes = [
            ("comment", Kind::Comment),
            ("string", Kind::String),
            ("keyword", Kind::Keyword),
            ("storage", Kind::Keyword),
            ("constant", Kind::Constant),
            ("entity.name", Kind::Name),
            ("support", Kind::Name),
        ]
        .map(|(name, kind)| {
            (Scope::new(name).expect("built-in scope"), kind)
        });
        for (path, new, output) in [
            (old_path, false, &mut result.old),
            (new_path, true, &mut result.new),
        ] {
            let syntax = path
                .extension()
                .and_then(|e| e.to_str())
                .and_then(|e| syntaxes.find_syntax_by_extension(e));
            let Some(syntax) = syntax else { continue };
            let mut parser = ParseState::new(syntax);
            let mut stack = ScopeStack::new();
            let deadline = Instant::now() + Duration::from_secs(2);
            let (mut bytes, mut spans) = (0, 0);
            'lines: for (i, line) in diff.lines.iter().enumerate() {
                if if new {
                    line.new.is_none()
                } else {
                    line.old.is_none()
                } {
                    continue;
                }
                bytes += line.text.len();
                if bytes > SYNTAX_BYTES
                    || line.text.len() > SYNTAX_LINE
                    || spans >= SYNTAX_SPANS
                    || Instant::now() >= deadline
                {
                    // Never resume after skipping a line: its closing
                    // delimiter may change every later scope.
                    result.limited = true;
                    break;
                }
                let mut text = line.text.clone();
                if !line.no_newline {
                    text.push('\n');
                }
                let Ok(ops) = parser.parse_line(&text, &syntaxes)
                else {
                    result.limited = true;
                    break;
                };
                let mut start = 0;
                for (at, op) in ops {
                    let end = at.min(line.text.len());
                    append(
                        &mut output[i],
                        start..end,
                        kind(&stack, &scopes),
                        false,
                    );
                    if stack.apply(&op).is_err() {
                        output[i].clear();
                        result.limited = true;
                        break 'lines;
                    }
                    start = end;
                }
                append(
                    &mut output[i],
                    start..line.text.len(),
                    kind(&stack, &scopes),
                    false,
                );
                spans += output[i].len();
                if output[i].len() > LINE_SPANS {
                    output[i].clear();
                    result.limited = true;
                }
            }
        }
        let deadline = Instant::now() + Duration::from_millis(250);
        let mut at = 0;
        while at < diff.lines.len() {
            if diff.lines[at].mark == Mark::Context {
                at += 1;
                continue;
            }
            let start = at;
            while at < diff.lines.len()
                && diff.lines[at].mark != Mark::Context
            {
                at += 1;
            }
            let deleted = (start..at)
                .filter(|&i| diff.lines[i].mark == Mark::Delete);
            let added = (start..at)
                .filter(|&i| diff.lines[i].mark == Mark::Add);
            for (old, new) in deleted.zip(added) {
                let (a, b) =
                    (&diff.lines[old].text, &diff.lines[new].text);
                if a.len() > WORD_LINE
                    || b.len() > WORD_LINE
                    || Instant::now() >= deadline
                {
                    result.limited = true;
                    continue;
                }
                let words = TextDiff::configure()
                    .deadline(deadline.min(
                        Instant::now() + Duration::from_millis(5),
                    ))
                    .diff_unicode_words(a.as_str(), b.as_str());
                let (mut x, mut y) = (0, 0);
                let (mut removed, mut inserted) =
                    (Vec::new(), Vec::new());
                for change in words.iter_all_changes() {
                    let len = change.value().len();
                    match change.tag() {
                        ChangeTag::Equal => {
                            x += len;
                            y += len;
                        }
                        ChangeTag::Delete => {
                            removed.push(x..x + len);
                            x += len;
                        }
                        ChangeTag::Insert => {
                            inserted.push(y..y + len);
                            y += len;
                        }
                    }
                }
                emphasize(&mut result.old[old], a.len(), &removed);
                emphasize(&mut result.new[new], b.len(), &inserted);
                for spans in
                    [&mut result.old[old], &mut result.new[new]]
                {
                    if spans.len() > LINE_SPANS {
                        spans.clear();
                        result.limited = true;
                    }
                }
            }
        }
        result
    }
}

fn kind(stack: &ScopeStack, scopes: &[(Scope, Kind)]) -> Kind {
    // An escape or interpolation may have its own nested syntax role.
    stack
        .scopes
        .iter()
        .rev()
        .find_map(|scope| {
            scopes.iter().find_map(|(prefix, kind)| {
                prefix.is_prefix_of(*scope).then_some(*kind)
            })
        })
        .unwrap_or_default()
}

fn append(
    spans: &mut Vec<Span>,
    range: Range<usize>,
    kind: Kind,
    changed: bool,
) {
    if range.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut()
        && last.range.end == range.start
        && last.kind == kind
        && last.changed == changed
    {
        last.range.end = range.end;
    } else {
        spans.push(Span {
            range,
            kind,
            changed,
        });
    }
}

fn emphasize(
    spans: &mut Vec<Span>,
    len: usize,
    changes: &[Range<usize>],
) {
    if changes.is_empty() {
        return;
    }
    if spans.is_empty() {
        append(spans, 0..len, Kind::Plain, false);
    }
    let mut result = Vec::new();
    let mut change = 0;
    for span in spans.iter() {
        let mut at = span.range.start;
        while at < span.range.end {
            while change < changes.len() && changes[change].end <= at
            {
                change += 1;
            }
            let next = changes.get(change);
            let changed = next.is_some_and(|r| r.contains(&at));
            let end = next
                .map(|r| if changed { r.end } else { r.start })
                .unwrap_or(span.range.end)
                .min(span.range.end);
            append(&mut result, at..end, span.kind, changed);
            at = end;
        }
    }
    *spans = result;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepare(old: &str, new: &str) -> (Diff, Highlighting) {
        let diff = Diff::between(old.as_bytes(), new.as_bytes());
        let highlights = Highlighting::new(
            &diff,
            Path::new("file.rs"),
            Path::new("file.rs"),
        );
        (diff, highlights)
    }

    #[test]
    fn hidden_lines_preserve_separate_multiline_states_on_each_side()
    {
        for (open, close, expected) in [
            ("/*", "*/", Kind::Comment),
            ("r###\"", "\"###", Kind::String),
        ] {
            let middle = "let hidden = 1;\n".repeat(10_000);
            let (diff, highlights) = prepare(
                &format!("{open}\n{middle}{close}\nlet tail = 2;\n"),
                &format!(
                    "// opening removed\n{middle}{close}\nlet tail \
                     = 3;\n"
                ),
            );
            let i = diff
                .lines
                .iter()
                .position(|l| l.old == Some(10_001))
                .unwrap();
            assert_eq!(diff.lines[i].mark, Mark::Context);
            assert!(
                highlights.old[i].iter().all(|s| s.kind == expected)
            );
            assert!(
                highlights.new[i]
                    .iter()
                    .any(|s| s.kind == Kind::Keyword)
            );
            assert!(!highlights.old[i].is_empty());
            let tail = diff
                .lines
                .iter()
                .position(|l| l.old == Some(10_003))
                .unwrap();
            assert!(
                highlights.old[tail]
                    .iter()
                    .any(|s| s.kind == Kind::Keyword)
            );
        }
    }

    #[test]
    fn unicode_words_and_one_sided_edits_keep_exact_byte_ranges() {
        for (old, new, removed, inserted) in [
            (
                "\tprint(\"café\", 12);\r",
                "\tprint(\"茶\", 123);\r",
                "café12",
                "茶123",
            ),
            ("call(a);", "call(a, beta);", "", ", beta"),
            ("call(a, beta);", "call(a);", ", beta", ""),
            (
                "let x = \"👩‍💻 e\u{301}\";",
                "let x = \"👩‍🔬 a\u{301}\";",
                "👩‍💻e\u{301}",
                "👩‍🔬a\u{301}",
            ),
        ] {
            let (diff, highlights) = prepare(old, new);
            for (spans, line, expected) in [
                (&highlights.old[0], &diff.lines[0], removed),
                (&highlights.new[1], &diff.lines[1], inserted),
            ] {
                assert_eq!(
                    spans
                        .iter()
                        .map(|s| &line.text[s.range.clone()])
                        .collect::<String>(),
                    line.text
                );
                assert_eq!(
                    spans
                        .iter()
                        .filter(|s| s.changed)
                        .map(|s| &line.text[s.range.clone()])
                        .collect::<String>(),
                    expected
                );
            }
        }
    }

    #[test]
    fn unequal_blocks_only_emphasize_paired_lines_and_renames_use_each_extension()
     {
        let diff =
            Diff::between(b"let a = 1;\n", b"let a = 2;\nextra\n");
        let highlights = Highlighting::new(
            &diff,
            Path::new("old.rs"),
            Path::new("new.unknown"),
        );
        assert!(
            highlights.old[0].iter().any(|s| s.kind == Kind::Keyword)
        );
        assert!(
            highlights.new[1].iter().all(|s| s.kind == Kind::Plain)
        );
        assert_eq!(
            highlights.new[1]
                .iter()
                .filter(|s| s.changed)
                .map(|s| &diff.lines[1].text[s.range.clone()])
                .collect::<String>(),
            "2"
        );
        assert!(highlights.new[2].is_empty());
    }

    #[test]
    fn oversized_lines_stop_syntax_without_resuming_with_a_false_state()
     {
        let (_, edge) = prepare(
            "",
            &format!("//{}", "x".repeat(SYNTAX_LINE - 2)),
        );
        assert!(!edge.limited);
        assert_eq!(edge.new[0][0].range, 0..SYNTAX_LINE);
        let long = format!("{} */", "x".repeat(SYNTAX_LINE));
        let (diff, highlights) =
            prepare("", &format!("/*\n{long}\nlet after = 1;\n"));
        assert!(highlights.limited);
        assert_eq!(diff.lines[1].text, long);
        assert!(
            highlights.new[0].iter().all(|s| s.kind == Kind::Comment)
        );
        assert!(highlights.new[1].is_empty());
        assert!(highlights.new[2].is_empty());
        let (diff, highlights) =
            prepare(&"a ".repeat(WORD_LINE), &"b ".repeat(WORD_LINE));
        assert!(highlights.limited);
        assert!(
            highlights
                .old
                .iter()
                .chain(&highlights.new)
                .flatten()
                .all(|s| !s.changed)
        );
        assert_eq!(diff.lines.len(), 2);
        let dense = format!(
            "{}/*\nlet still_a_comment = 1;\n",
            "let x = 1; ".repeat(40)
        );
        let (_, highlights) = prepare("", &dense);
        assert!(highlights.limited);
        assert!(highlights.new[0].is_empty());
        assert_eq!(highlights.new[1].len(), 1);
        assert_eq!(highlights.new[1][0].kind, Kind::Comment);
    }
}
