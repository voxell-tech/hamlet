//! pi's bounds on what a tool gives: a read's first lines, and a
//! command's last.

use serde_json::{Value, json};

pub(crate) const MAX_LINES: usize = 2000;
pub(crate) const MAX_BYTES: usize = 50 * 1024;

/// `bytes` as pi writes a size, rounding a half up.
pub(crate) fn format_size(bytes: usize) -> String {
    let tenths = |unit: usize| {
        let tenths = (bytes * 10 + unit / 2) / unit;
        format!("{}.{}", tenths / 10, tenths % 10)
    };
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{}KB", tenths(1024))
    } else {
        format!("{}MB", tenths(1024 * 1024))
    }
}

/// How a text was cut to its first lines.
#[derive(Clone, Debug)]
pub(crate) struct Truncation {
    pub(crate) truncated: bool,
    /// "lines" or "bytes".
    pub(crate) by: Option<&'static str>,
    pub(crate) total_lines: usize,
    pub(crate) total_bytes: usize,
    pub(crate) output_lines: usize,
    pub(crate) output_bytes: usize,
    pub(crate) first_line_exceeds_limit: bool,
}

impl Truncation {
    pub(crate) fn json(&self) -> Value {
        json!({
            "truncated": self.truncated,
            "truncatedBy": self.by,
            "totalLines": self.total_lines,
            "totalBytes": self.total_bytes,
            "outputLines": self.output_lines,
            "outputBytes": self.output_bytes,
            "lastLinePartial": false,
            "firstLineExceedsLimit": self.first_line_exceeds_limit,
            "maxLines": MAX_LINES,
            "maxBytes": MAX_BYTES,
        })
    }
}

fn counted_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines = content.split('\n').collect::<Vec<_>>();
    if content.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// The first whole lines of `content` within the limits. When the
/// first line alone is too long, nothing.
pub(crate) fn truncate_head(content: &str) -> (String, Truncation) {
    let lines = counted_lines(content);
    let mut truncation = Truncation {
        truncated: false,
        by: None,
        total_lines: lines.len(),
        total_bytes: content.len(),
        output_lines: lines.len(),
        output_bytes: content.len(),
        first_line_exceeds_limit: false,
    };
    if lines.len() <= MAX_LINES && content.len() <= MAX_BYTES {
        return (content.to_owned(), truncation);
    }
    truncation.truncated = true;
    if lines.first().map_or(0, |line| line.len()) > MAX_BYTES {
        truncation.by = Some("bytes");
        truncation.output_lines = 0;
        truncation.output_bytes = 0;
        truncation.first_line_exceeds_limit = true;
        return (String::new(), truncation);
    }
    let mut kept = Vec::new();
    let mut bytes = 0;
    let mut by = "lines";
    for (i, line) in lines.iter().take(MAX_LINES).enumerate() {
        let size = line.len() + usize::from(i > 0);
        if bytes + size > MAX_BYTES {
            by = "bytes";
            break;
        }
        kept.push(*line);
        bytes += size;
    }
    // Without a byte break, only omitted lines prove the line limit
    // was reached.
    if by != "bytes" {
        by = if kept.len() < lines.len() {
            "lines"
        } else {
            "bytes"
        };
    }
    let content = kept.join("\n");
    truncation.by = Some(by);
    truncation.output_lines = kept.len();
    truncation.output_bytes = content.len();
    (content, truncation)
}

/// The last character boundary at or before `index`.
pub(crate) fn character_end(bytes: &[u8], index: usize) -> usize {
    let mut end = index;
    while end > 0 && bytes.get(end).is_some_and(|b| b & 0xc0 == 0x80)
    {
        end -= 1;
    }
    end
}

/// The first character boundary at or after `index`.
fn character_start(bytes: &[u8], index: usize) -> usize {
    let mut start = index;
    while bytes.get(start).is_some_and(|b| b & 0xc0 == 0x80) {
        start += 1;
    }
    start
}

/// Where the last whole lines of `text` within the limits begin; a
/// last line longer than the byte limit is cut.
fn tail_start(text: &str) -> usize {
    let bytes = text.as_bytes();
    let newline_before =
        |end: usize| bytes[..=end].iter().rposition(|&b| b == b'\n');
    // A trailing newline ends the last line rather than starting
    // another.
    let last =
        bytes.len().checked_sub(if bytes.last() == Some(&b'\n') {
            2
        } else {
            1
        });
    let mut start = 0;
    let mut lines = 1;
    let mut index = last.and_then(newline_before);
    while let Some(at) = index {
        if lines == MAX_LINES {
            start = at + 1;
            break;
        }
        lines += 1;
        index = at.checked_sub(1).and_then(newline_before);
    }
    if bytes.len() - start > MAX_BYTES {
        let from = bytes.len() - MAX_BYTES;
        let newline = bytes[from - 1..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|at| at + from - 1);
        start = match newline {
            Some(at) if at + 1 < bytes.len() => at + 1,
            _ => character_start(bytes, from),
        };
    }
    start
}

/// The shortest end of `text` longer than the limits by a byte or a
/// line, whose last lines are those of `text` with anything after.
fn tail_margin(text: &str) -> &str {
    let bytes = text.as_bytes();
    let byte_start = if bytes.len() > MAX_BYTES {
        character_end(bytes, bytes.len() - MAX_BYTES - 1)
    } else {
        0
    };
    let line_start = bytes
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, b)| **b == b'\n')
        .nth(MAX_LINES)
        .map_or(0, |(at, _)| at);
    &text[byte_start.max(line_start)..]
}

/// Takes out the control characters that break display and
/// transcripts; tabs and newlines stay.
pub(crate) fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            !matches!(c, '\0'..='\u{8}' | '\u{b}'..='\u{1f}' | '\u{fff9}'..='\u{fffb}')
        })
        .collect()
}

/// The running output of a command, of which its last lines are kept.
#[derive(Default)]
pub(crate) struct Tail {
    stored: String,
}

impl Tail {
    pub(crate) fn push(&mut self, text: &str) {
        self.stored.push_str(text);
        if self.stored.len() > 4 * (MAX_BYTES + 1) {
            self.stored = tail_margin(&self.stored).to_owned();
        }
    }

    /// The last lines within the limits, sanitized.
    pub(crate) fn text(&self) -> String {
        sanitize(&self.stored[tail_start(&self.stored)..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_lines_of_a_long_output() {
        let mut tail = Tail::default();
        for n in 0..300_000 {
            tail.push(&format!("{n}\n"));
        }
        let kept = tail.text();
        assert!(kept.starts_with("298000\n"));
        assert!(kept.ends_with("299999\n"));
        let mut wide = Tail::default();
        wide.push(&format!("{}\nend", "x".repeat(60 * 1024)));
        assert_eq!(wide.text(), "end");
        let mut long = Tail::default();
        long.push(&"\u{e9}".repeat(30 * 1024));
        assert_eq!(long.text().len(), MAX_BYTES);
    }

    #[test]
    fn writes_sizes_as_pi_does() {
        assert_eq!(format_size(1280), "1.3KB");
        assert_eq!(format_size(MAX_BYTES), "50.0KB");
        assert_eq!(format_size(512), "512B");
    }
}
