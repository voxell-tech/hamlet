//! pi's `read`, `write` and `edit`, whose paths fall in the house or
//! the folder as C20 says.

use hamlet_protocol::runtime::DiagnosticSeverity;
use hamlet_workspace_wire::Code;
use serde_json::{Map, Value, json};
use unicode_normalization::UnicodeNormalization;

use super::environment::Reach;
use super::truncate::{
    MAX_BYTES, character_end, format_size, truncate_head,
};
use super::{Arguments, Outcome, diagnostic, diff, number_words};
use crate::sandbox::{FileError, FileKind};

/// The failure of a file call, in pi's words.
#[derive(Debug, thiserror::Error)]
enum CallError {
    #[error(transparent)]
    File(#[from] FileError),
    #[error(
        "Offset {} is beyond end of file ({lines} lines total)",
        number_words(*.offset)
    )]
    BeyondEnd { offset: f64, lines: usize },
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

fn outcome(result: Result<Outcome, CallError>) -> Outcome {
    result.unwrap_or_else(|error| {
        Outcome::thrown("tool_error", error.to_string())
    })
}

/// The path pi's tools open for `path`: unusual spaces made plain and
/// a leading `@` dropped.
fn tool_path(env: &impl Reach, path: &str) -> String {
    let plain = path
        .chars()
        .map(|c| match c {
            '\u{a0}'
            | '\u{2000}'..='\u{200a}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}' => ' ',
            c => c,
        })
        .collect::<String>();
    env.absolute(plain.strip_prefix('@').unwrap_or(&plain))
}

/// pi's `read`: a text file's lines, at most 2000 or 50KB from
/// `offset`, with remarks on what was left out.
pub(super) async fn read(
    env: &mut impl Reach,
    arguments: &Map<String, Value>,
) -> Outcome {
    let args = Arguments::new("read", arguments);
    let (path, offset, limit) = match (
        args.string("path"),
        args.number("offset"),
        args.number("limit"),
    ) {
        (Ok(path), Ok(offset), Ok(limit)) => (path, offset, limit),
        (Err(invalid), _, _)
        | (_, Err(invalid), _)
        | (_, _, Err(invalid)) => {
            return invalid;
        }
    };
    outcome(
        async {
            let path = env.footing(path).await?;
            read_text(env, &path, offset, limit).await
        }
        .await,
    )
}

async fn read_text(
    env: &mut impl Reach,
    path: &str,
    offset: Option<f64>,
    limit: Option<f64>,
) -> Result<Outcome, CallError> {
    let resolved = tool_path(env, path);
    let decomposed = resolved.nfd().collect::<String>();
    let variants = [
        resolved.clone(),
        narrow_meridiem(&resolved),
        decomposed.clone(),
        resolved.replace('\'', "\u{2019}"),
        decomposed.replace('\'', "\u{2019}"),
    ];
    let mut absolute = resolved.clone();
    let mut tried = Vec::new();
    for variant in variants {
        if tried.contains(&variant) {
            continue;
        }
        if env.exists(&variant).await? {
            absolute = variant;
            break;
        }
        tried.push(variant);
    }
    let bytes = env.read_file(&absolute).await?;
    if let Some(mime) = image_type(&bytes) {
        return Ok(Outcome {
            diagnostics: vec![diagnostic(
                DiagnosticSeverity::Error,
                "unsupported_image",
                format!(
                    "{path} is an image ({mime}) that cannot be \
                     prepared for the model"
                ),
            )],
            ..Outcome::failed("")
        });
    }
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let lines = text.split('\n').collect::<Vec<_>>();
    let total = lines.len() as f64;
    let start = match offset {
        Some(offset) if offset != 0.0 && !offset.is_nan() => {
            (offset - 1.0).max(0.0)
        }
        _ => 0.0,
    };
    if start >= total {
        return Err(CallError::BeyondEnd {
            offset: offset.unwrap_or(0.0),
            lines: lines.len(),
        });
    }
    let (selected, end) = match limit {
        Some(limit) => {
            let end = (start + limit).min(total);
            (slice(&lines, start, end), Some(end))
        }
        None => (slice(&lines, start, total), None),
    };
    let (head, truncation) = truncate_head(&selected.join("\n"));
    let shown_start = start + 1.0;
    let mut out = Outcome::text(head);
    if truncation.first_line_exceeds_limit {
        let line = if start.fract() == 0.0 {
            selected[0]
        } else {
            ""
        };
        let cut = character_end(line.as_bytes(), MAX_BYTES);
        out.text = line[..cut].to_owned();
        out.diagnostics.push(diagnostic(
            DiagnosticSeverity::Warn,
            "truncated",
            format!(
                "Line {line_number} is {}, exceeds the {} limit; \
                 showing its first {}. Use bash: sed -n \
                 '{line_number}p' {path} | tail -c +{}",
                format_size(line.len()),
                format_size(MAX_BYTES),
                format_size(cut),
                cut + 1,
                line_number = number_words(shown_start),
            ),
        ));
        let mut details = truncation.json();
        details["outputBytes"] = json!(cut);
        details["outputLines"] = json!(1);
        out.details =
            Some(Map::from_iter([("truncation".into(), details)]));
    } else if truncation.truncated {
        let last = shown_start + truncation.output_lines as f64 - 1.0;
        let why = if truncation.by == Some("lines") {
            String::new()
        } else {
            format!(" ({} limit)", format_size(MAX_BYTES))
        };
        out.diagnostics.push(diagnostic(
            DiagnosticSeverity::Info,
            "truncated",
            format!(
                "Showing lines {}-{} of {}{why}. Use offset={} to \
                 continue.",
                number_words(shown_start),
                number_words(last),
                lines.len(),
                number_words(last + 1.0),
            ),
        ));
        out.details = Some(Map::from_iter([(
            "truncation".into(),
            truncation.json(),
        )]));
    } else if let Some(end) = end.filter(|end| *end < total) {
        // pi gives this remark no code.
        out.diagnostics.push(diagnostic(
            DiagnosticSeverity::Info,
            "",
            format!(
                "{} more lines in file. Use offset={} to continue.",
                number_words(total - end),
                number_words(end + 1.0),
            ),
        ));
    }
    Ok(out)
}

/// The lines pi's `read` selects from `start` to `end`: each index
/// cut toward zero, `NaN` as 0 and a negative one counted
/// from the end.
fn slice<'a>(
    lines: &[&'a str],
    start: f64,
    end: f64,
) -> Vec<&'a str> {
    let len = lines.len() as f64;
    let index = |at: f64| {
        let at = if at.is_nan() { 0.0 } else { at.trunc() };
        (if at < 0.0 {
            (len + at).max(0.0)
        } else {
            at.min(len)
        }) as usize
    };
    let (start, end) = (index(start), index(end));
    if start < end {
        lines[start..end].to_vec()
    } else {
        Vec::new()
    }
}

/// `path` with a narrow no-break space before ` AM.` or ` PM.`, as
/// macOS names its screenshots.
fn narrow_meridiem(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(at) = rest.find(' ') {
        let after = &rest[at + 1..];
        let meridiem = after.get(..3).filter(|word| {
            let lower = word.to_ascii_lowercase();
            lower == "am." || lower == "pm."
        });
        out.push_str(&rest[..at]);
        match meridiem {
            Some(word) => {
                out.push('\u{202f}');
                out.push_str(word);
                rest = &after[3..];
            }
            None => {
                out.push(' ');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The image type pi sends a file as, by its first bytes.
fn image_type(bytes: &[u8]) -> Option<&'static str> {
    const PNG: [u8; 8] =
        [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    let at = |offset: usize, text: &[u8]| {
        bytes.get(offset..offset + text.len()) == Some(text)
    };
    if at(0, &[0xff, 0xd8, 0xff]) {
        return (bytes.get(3) != Some(&0xf7)).then_some("image/jpeg");
    }
    if at(0, &PNG) {
        let header = bytes.len() >= 16
            && at(12, b"IHDR")
            && be32(bytes, 8) == 13;
        return (header && !is_animated_png(bytes))
            .then_some("image/png");
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some("image/gif");
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some("image/webp");
    }
    None
}

fn be32(bytes: &[u8], offset: usize) -> usize {
    bytes.get(offset..offset + 4).map_or(0, |b| {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize
    })
}

fn is_animated_png(bytes: &[u8]) -> bool {
    let mut offset = 8;
    while offset + 8 <= bytes.len() {
        match &bytes[offset + 4..offset + 8] {
            b"acTL" => return true,
            b"IDAT" => return false,
            _ => {}
        }
        let next = offset + 8 + be32(bytes, offset) + 4;
        if next <= offset || next > bytes.len() {
            return false;
        }
        offset = next;
    }
    false
}

/// The policy's check of a file a write or an edit changes, as pi's
/// queue of changes to one file resolves it.
async fn queued(
    env: &mut impl Reach,
    path: &str,
) -> Result<(), CallError> {
    let mut path = path.to_owned();
    loop {
        match env.canonical_path(&path).await {
            Ok(_) => return Ok(()),
            Err(error) if error.code == Code::NotFound => {
                let parent =
                    hamlet_os::fs::path::join(&[&path, ".."]);
                if parent == path || !path.starts_with(&parent) {
                    return Ok(());
                }
                path = parent;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// pi's `write`.
pub(super) async fn write(
    env: &mut impl Reach,
    arguments: &Map<String, Value>,
) -> Outcome {
    let args = Arguments::new("write", arguments);
    let (path, content) =
        match (args.string("path"), args.string("content")) {
            (Ok(path), Ok(content)) => (path, content),
            (Err(invalid), _) | (_, Err(invalid)) => return invalid,
        };
    outcome(
        async {
            let path = env.footing(path).await?;
            let absolute = tool_path(env, &path);
            queued(env, &absolute).await?;
            env.write_file(&absolute, content).await?;
            Ok(Outcome::text(format!("Successfully wrote to {path}")))
        }
        .await,
    )
}

/// The text of the file a `write` to `path` would replace, or `None`
/// where there is none; a file that cannot be read as text is told
/// of in brackets.
pub(super) async fn preview(
    env: &mut impl Reach,
    path: &str,
) -> Result<Option<String>, FileError> {
    let full = env.footing(path).await?;
    Ok(match env.read_text(&full).await {
        Ok(text) => Some(text),
        Err(error) if error.code == Code::NotFound => None,
        Err(error) => {
            let size = env
                .file_info(&full)
                .await
                .map(|info| format!(", {} bytes", info.size))
                .unwrap_or_default();
            let reason = if error.code == Code::Invalid {
                "is not UTF-8 text"
            } else {
                "could not be read as text"
            };
            Some(format!("[a file that {reason}{size}]"))
        }
    })
}

/// One replacement of an edit.
struct Replace {
    old: String,
    new: String,
}

/// pi's `edit`.
pub(super) async fn edit(
    env: &mut impl Reach,
    arguments: &Map<String, Value>,
) -> Outcome {
    let arguments = repaired(arguments);
    let args = Arguments::new("edit", &arguments);
    let path = match args.string("path") {
        Ok(path) => path,
        Err(invalid) => return invalid,
    };
    let Some(edits) =
        arguments.get("edits").and_then(Value::as_array)
    else {
        return args.invalid("edits", "array");
    };
    let mut replaces = Vec::new();
    for (i, edit) in edits.iter().enumerate() {
        let side = |name| edit.get(name).and_then(Value::as_str);
        match (side("oldText"), side("newText")) {
            (Some(old), Some(new)) => replaces.push(Replace {
                old: old.to_owned(),
                new: new.to_owned(),
            }),
            _ => {
                return args.invalid(&format!("edits/{i}"), "object");
            }
        }
    }
    outcome(
        async {
            let path = env.footing(path).await?;
            edited(env, &path, &replaces).await
        }
        .await,
    )
}

async fn edited(
    env: &mut impl Reach,
    path: &str,
    replaces: &[Replace],
) -> Result<Outcome, CallError> {
    if replaces.is_empty() {
        return Err(CallError::NoEdits);
    }
    let access = |error: FileError| CallError::Unedited {
        path: path.into(),
        code: error.code,
    };
    let absolute = tool_path(env, path);
    queued(env, &absolute).await?;
    let info = env.file_info(&absolute).await.map_err(access)?;
    if !matches!(info.kind, FileKind::File | FileKind::Symlink) {
        return Err(CallError::NotAFile(path.into()));
    }
    let content = env.read_text(&absolute).await.map_err(access)?;
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
    env.write_file(&absolute, &format!("{bom}{written}"))
        .await
        .map_err(access)?;
    let (diff, first) = diff::diff_string(&base, &new);
    let mut details = Map::from_iter([
        ("diff".into(), json!(diff)),
        (
            "patch".into(),
            json!(diff::unified_patch(path, &base, &new)),
        ),
    ]);
    if let Some(first) = first {
        details.insert("firstChangedLine".into(), json!(first));
    }
    Ok(Outcome {
        details: Some(details),
        ..Outcome::text(format!(
            "Successfully replaced {} block(s) in {path}.",
            replaces.len()
        ))
    })
}

/// The shapes models commonly send repaired: `edits` as a JSON string
/// or as one edit, and a lone `oldText` and `newText`.
fn repaired(arguments: &Map<String, Value>) -> Map<String, Value> {
    let single = |value: &Value| {
        value.get("oldText").is_some_and(Value::is_string)
            && value.get("newText").is_some_and(Value::is_string)
            && value.is_object()
    };
    let mut args = arguments.clone();
    match args.get("edits") {
        Some(Value::String(text)) => {
            match serde_json::from_str::<Value>(text) {
                Ok(parsed @ Value::Array(_)) => {
                    args.insert("edits".into(), parsed);
                }
                Ok(parsed) if single(&parsed) => {
                    args.insert("edits".into(), json!([parsed]));
                }
                _ => {}
            }
        }
        Some(edit) if single(edit) => {
            let edit = edit.clone();
            args.insert("edits".into(), json!([edit]));
        }
        _ => {}
    }
    if !single(&Value::Object(args.clone())) {
        return args;
    }
    let mut edits = match args.get("edits") {
        Some(Value::Array(edits)) => edits.clone(),
        _ => Vec::new(),
    };
    let old = args.remove("oldText").unwrap_or_default();
    let new = args.remove("newText").unwrap_or_default();
    edits.push(json!({"oldText": old, "newText": new}));
    args.insert("edits".into(), Value::Array(edits));
    args
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
        .map(|line| line.trim_end_matches(super::is_pi_space))
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
