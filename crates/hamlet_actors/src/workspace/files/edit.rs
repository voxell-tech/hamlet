//! An edit's text replacements, matched as pi's `edit` matches them.
// Copied from the server's `tools/files.rs`, deleted with it.

use hamlet_messages::workspace::files::{EditDiff, Replace};
use hamlet_workspace_wire::Code;
use unicode_normalization::UnicodeNormalization;

use super::diff;
use super::walk::is_pi_space;

/// The failure of an edit, in pi's words.
#[derive(Debug, thiserror::Error)]
pub(super) enum CallError {
    #[error(
        "Edit tool input is invalid. edits must contain at least \
         one replacement."
    )]
    NoEdits,
    #[error(
        "Could not edit file: {path}. Error code: {}.",
        .code.as_str()
    )]
    Unedited { path: String, code: Code },
    #[error("Could not edit file: {0}. Path is not a file.")]
    NotAFile(String),
    #[error("oldText must not be empty in {0}.")]
    EmptyOld(String),
    #[error("edits[{index}].oldText must not be empty in {path}.")]
    EmptyOldAt { index: usize, path: String },
    #[error(
        "Could not find the exact text in {0}. The old text must \
         match exactly including all whitespace and newlines."
    )]
    Missing(String),
    #[error(
        "Could not find edits[{index}] in {path}. The oldText must \
         match exactly including all whitespace and newlines."
    )]
    MissingAt { index: usize, path: String },
    #[error(
        "Found {count} occurrences of the text in {path}. The text \
         must be unique. Please provide more context to make it \
         unique."
    )]
    Repeated { count: usize, path: String },
    #[error(
        "Found {count} occurrences of edits[{index}] in {path}. \
         Each oldText must be unique. Please provide more context \
         to make it unique."
    )]
    RepeatedAt {
        count: usize,
        index: usize,
        path: String,
    },
    #[error(
        "edits[{first}] and edits[{second}] overlap in {path}. \
         Merge them into one edit or target disjoint regions."
    )]
    Overlap {
        first: usize,
        second: usize,
        path: String,
    },
    #[error(
        "No changes made to {0}. The replacement produced identical \
         content. This might indicate an issue with special \
         characters or the text not existing as expected."
    )]
    Unchanged(String),
    #[error(
        "No changes made to {0}. The replacements produced \
         identical content."
    )]
    UnchangedAll(String),
    #[error("Replacement range is outside the base content.")]
    Outside,
    #[error(
        "Cannot preserve unchanged lines because the base content \
         has a different line count."
    )]
    LineCount,
}

/// The file `path` holding `bytes` with `replaces` made, keeping its
/// byte-order mark and its line endings, and what changed.
pub(super) fn edited(
    path: &str,
    bytes: Option<Vec<u8>>,
    replaces: &[Replace],
) -> Result<(String, EditDiff), CallError> {
    if replaces.is_empty() {
        return Err(CallError::NoEdits);
    }
    let unedited = |code| CallError::Unedited {
        path: path.into(),
        code,
    };
    let bytes = bytes.ok_or_else(|| unedited(Code::NotFound))?;
    let content = String::from_utf8(bytes)
        .map_err(|_| unedited(Code::Invalid))?;
    let (bom, content) = match content.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", content.as_str()),
    };
    let crlf = has_crlf_endings(content);
    let base = to_lf(content);
    let new = apply(&base, replaces, path)?;
    let written = if crlf {
        new.replace('\n', "\r\n")
    } else {
        new.clone()
    };
    let (shown, first) = diff::diff_string(&base, &new);
    Ok((
        format!("{bom}{written}"),
        EditDiff {
            diff: shown,
            patch: diff::unified_patch(path, &base, &new),
            first_changed_line: first.map(|line| line as u64),
        },
    ))
}

fn has_crlf_endings(content: &str) -> bool {
    match (content.find("\r\n"), content.find('\n')) {
        (Some(crlf), Some(lf)) => crlf < lf,
        _ => false,
    }
}

fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `text` as pi compares it when an exact match fails.
fn fuzzy(text: &str) -> String {
    let normal = text.nfkc().collect::<String>();
    normal
        .split('\n')
        .map(|line| line.trim_end_matches(is_pi_space))
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .map(|c| match c {
            '\u{2018}'..='\u{201b}' => '\'',
            '\u{201c}'..='\u{201f}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{a0}'
            | '\u{2002}'..='\u{200a}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}' => ' ',
            c => c,
        })
        .collect()
}

/// Where `old` is in `content`, exactly or else as [`fuzzy`] text,
/// and how long the match is.
fn find(content: &str, old: &str) -> Option<(usize, usize, bool)> {
    if let Some(at) = content.find(old) {
        return Some((at, old.len(), false));
    }
    let old = fuzzy(old);
    fuzzy(content).find(&old).map(|at| (at, old.len(), true))
}

/// The number of times `old` is in `content`, as pi's `edit` counts
/// them.
fn occurrences(content: &str, old: &str) -> usize {
    let (content, old) = (fuzzy(content), fuzzy(old));
    if old.is_empty() {
        return content.encode_utf16().count().saturating_sub(1);
    }
    content.matches(old.as_str()).count()
}

/// A replacement matched in the content it applies to.
struct Matched<'a> {
    index: usize,
    at: usize,
    length: usize,
    new: &'a str,
}

/// The LF content with every replacement made, each matched against
/// the original. A fuzzy match changes only the lines it touches.
fn apply(
    base: &str,
    replaces: &[Replace],
    path: &str,
) -> Result<String, CallError> {
    let edits = replaces
        .iter()
        .map(|each| Replace {
            old: to_lf(&each.old),
            new: to_lf(&each.new),
        })
        .collect::<Vec<_>>();
    let many = edits.len() > 1;
    if let Some(i) = edits.iter().position(|edit| edit.old.is_empty())
    {
        let path = path.into();
        return Err(if many {
            CallError::EmptyOldAt { index: i, path }
        } else {
            CallError::EmptyOld(path)
        });
    }
    let used_fuzzy = edits.iter().any(|edit| {
        find(base, &edit.old).is_some_and(|(_, _, fuzzy)| fuzzy)
    });
    let replaced_in = if used_fuzzy {
        fuzzy(base)
    } else {
        base.to_owned()
    };
    let mut matched = Vec::new();
    for (i, edit) in edits.iter().enumerate() {
        let Some((at, length, _)) = find(&replaced_in, &edit.old)
        else {
            let path = path.into();
            return Err(if many {
                CallError::MissingAt { index: i, path }
            } else {
                CallError::Missing(path)
            });
        };
        let count = occurrences(&replaced_in, &edit.old);
        if count > 1 {
            let path = path.into();
            return Err(if many {
                CallError::RepeatedAt {
                    count,
                    index: i,
                    path,
                }
            } else {
                CallError::Repeated { count, path }
            });
        }
        matched.push(Matched {
            index: i,
            at,
            length,
            new: &edit.new,
        });
    }
    matched.sort_by_key(|each| each.at);
    for pair in matched.windows(2) {
        if pair[0].at + pair[0].length > pair[1].at {
            return Err(CallError::Overlap {
                first: pair[0].index,
                second: pair[1].index,
                path: path.into(),
            });
        }
    }
    let new = if used_fuzzy {
        preserving_lines(base, &replaced_in, &matched)?
    } else {
        replaced(&replaced_in, &matched.iter().collect::<Vec<_>>(), 0)
    };
    if new == base {
        let path = path.into();
        return Err(if many {
            CallError::UnchangedAll(path)
        } else {
            CallError::Unchanged(path)
        });
    }
    Ok(new)
}

/// `content`, which starts at `offset` of the matched text, with the
/// replacements made.
fn replaced(
    content: &str,
    matched: &[&Matched],
    offset: usize,
) -> String {
    let mut out = content.to_owned();
    for each in matched.iter().rev() {
        let at = each.at - offset;
        out.replace_range(at..at + each.length, each.new);
    }
    out
}

/// The replacements matched in `fuzzy_base` made on the lines they
/// touch, and every other line of `original` kept as it was.
fn preserving_lines(
    original: &str,
    fuzzy_base: &str,
    matched: &[Matched],
) -> Result<String, CallError> {
    let originals =
        original.split_inclusive('\n').collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut offset = 0;
    for line in fuzzy_base.split_inclusive('\n') {
        spans.push((offset, offset + line.len()));
        offset += line.len();
    }
    if originals.len() != spans.len() {
        return Err(CallError::LineCount);
    }
    // Groups of replacements by the lines they touch, as (first line,
    // end line, replacements).
    let mut groups = Vec::<(usize, usize, Vec<&Matched>)>::new();
    for each in matched {
        let start = spans
            .iter()
            .position(|(from, to)| each.at >= *from && each.at < *to)
            .ok_or(CallError::Outside)?;
        let end = start
            + spans[start..]
                .iter()
                .position(|(_, to)| *to >= each.at + each.length)
                .ok_or(CallError::Outside)?;
        match groups.last_mut() {
            Some(group) if start < group.1 => {
                group.1 = group.1.max(end + 1);
                group.2.push(each);
            }
            _ => groups.push((start, end + 1, vec![each])),
        }
    }
    let mut out = String::new();
    let mut line = 0;
    for (start, end, group) in groups {
        out.push_str(&originals[line..start].concat());
        let (from, to) = (spans[start].0, spans[end - 1].1);
        out.push_str(&replaced(&fuzzy_base[from..to], &group, from));
        line = end;
    }
    out.push_str(&originals[line..].concat());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edits(pairs: &[(&str, &str)]) -> Vec<Replace> {
        pairs
            .iter()
            .map(|(old, new)| Replace {
                old: old.to_string(),
                new: new.to_string(),
            })
            .collect()
    }

    #[test]
    fn matches_smart_quotes_and_trailing_spaces_but_keeps_other_lines()
     {
        let base =
            "keep \u{201c}this\u{201d}  \nsay \u{2018}hi\u{2019}  \n";
        let new =
            apply(base, &edits(&[("say 'hi'", "say 'bye'")]), "f");
        assert_eq!(
            new.unwrap(),
            "keep \u{201c}this\u{201d}  \nsay 'bye'\n"
        );
    }
}
