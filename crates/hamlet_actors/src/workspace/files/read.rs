//! A read of a file: a text file's first lines within the limits,
//! with remarks on what was left out, or an image as it is.

use hamlet_messages::bytes::Base64;
use hamlet_messages::workspace::files::{
    CutBy, FileAnswer, Image, Note, Severity, Text, Truncation,
};
use hamlet_sandbox::{Env, FileError};
use unicode_normalization::UnicodeNormalization;

use crate::workspace::{MOST_BYTES, MOST_LINES};

/// `path`'s lines from `offset`, at most `limit` of them and within
/// the limits, or the image it holds.
// Copied from the server's `tools/files.rs` `read_text`.
pub(super) fn read(
    env: &Env,
    path: &str,
    offset: Option<u64>,
    limit: Option<u64>,
) -> Result<FileAnswer, FileError> {
    let decomposed = path.nfd().collect::<String>();
    let variants = [
        path.to_owned(),
        narrow_meridiem(path),
        decomposed.clone(),
        path.replace('\'', "\u{2019}"),
        decomposed.replace('\'', "\u{2019}"),
    ];
    let mut absolute = path.to_owned();
    let mut tried = Vec::new();
    for variant in variants {
        if tried.contains(&variant) {
            continue;
        }
        if env.exists(&variant)? {
            absolute = variant;
            break;
        }
        tried.push(variant);
    }
    let bytes = env.read_file(&absolute)?;
    if let Some(mime) = image(&bytes) {
        return Ok(FileAnswer::Image(Image {
            mime: mime.to_owned(),
            data: Base64(bytes),
        }));
    }
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let lines = text.split('\n').collect::<Vec<_>>();
    let total = lines.len();
    let start = offset.map_or(0, |offset| {
        usize::try_from(offset.saturating_sub(1))
            .unwrap_or(usize::MAX)
    });
    if start >= total {
        return Ok(FileAnswer::Failed {
            words: format!(
                "Offset {} is beyond end of file ({total} lines \
                 total)",
                offset.unwrap_or(0)
            ),
        });
    }
    let end = limit.map(|limit| {
        start
            .saturating_add(
                usize::try_from(limit).unwrap_or(usize::MAX),
            )
            .min(total)
    });
    let selected = &lines[start..end.unwrap_or(total)];
    let (head, truncation) = truncate_head(&selected.join("\n"));
    let shown_start = start + 1;
    let mut out = Text {
        text: head,
        notes: Vec::new(),
        truncation: None,
    };
    if truncation.first_line_exceeds_limit {
        let line = selected[0];
        let cut = character_end(line.as_bytes(), MOST_BYTES);
        out.text = line[..cut].to_owned();
        out.notes.push(Note {
            severity: Severity::Warn,
            code: "truncated".into(),
            message: format!(
                "Line {shown_start} is {}, exceeds the {} limit; \
                 showing its first {}. Use bash: sed -n \
                 '{shown_start}p' {path} | tail -c +{}",
                format_size(line.len()),
                format_size(MOST_BYTES),
                format_size(cut),
                cut + 1,
            ),
        });
        out.truncation = Some(Truncation {
            output_bytes: cut as u64,
            output_lines: 1,
            ..truncation
        });
    } else if truncation.by.is_some() {
        let last = shown_start + truncation.output_lines as usize - 1;
        let why = if truncation.by == Some(CutBy::Lines) {
            String::new()
        } else {
            format!(" ({} limit)", format_size(MOST_BYTES))
        };
        out.notes.push(Note {
            severity: Severity::Info,
            code: "truncated".into(),
            message: format!(
                "Showing lines {shown_start}-{last} of \
                 {total}{why}. Use offset={} to continue.",
                last + 1,
            ),
        });
        out.truncation = Some(truncation);
    } else if let Some(end) = end.filter(|end| *end < total) {
        // pi gives this remark no code.
        out.notes.push(Note {
            severity: Severity::Info,
            code: String::new(),
            message: format!(
                "{} more lines in file. Use offset={} to continue.",
                total - end,
                end + 1,
            ),
        });
    }
    Ok(FileAnswer::Text(out))
}

/// The type of image `bytes` begin as, if it is one a model takes.
// Copied from the server's `images::Format::detect`.
fn image(bytes: &[u8]) -> Option<&'static str> {
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
            && bytes.get(8..12) == Some(&13u32.to_be_bytes());
        return header.then_some("image/png");
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some("image/gif");
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some("image/webp");
    }
    None
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

/// `bytes` as pi writes a size, rounding a half up.
// Copied from the server's `truncate::format_size`.
fn format_size(bytes: usize) -> String {
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

/// The first whole lines of `content` within the limits, and how it
/// was cut. When the first line alone is too long, nothing.
// Copied from the server's `truncate::truncate_head`.
fn truncate_head(content: &str) -> (String, Truncation) {
    let mut lines = content.split('\n').collect::<Vec<_>>();
    if content.is_empty() {
        lines.clear();
    } else if content.ends_with('\n') {
        lines.pop();
    }
    let mut truncation = Truncation {
        by: None,
        total_lines: lines.len() as u64,
        total_bytes: content.len() as u64,
        output_lines: lines.len() as u64,
        output_bytes: content.len() as u64,
        first_line_exceeds_limit: false,
    };
    if lines.len() <= MOST_LINES && content.len() <= MOST_BYTES {
        return (content.to_owned(), truncation);
    }
    if lines.first().map_or(0, |line| line.len()) > MOST_BYTES {
        truncation.by = Some(CutBy::Bytes);
        truncation.output_lines = 0;
        truncation.output_bytes = 0;
        truncation.first_line_exceeds_limit = true;
        return (String::new(), truncation);
    }
    let mut kept = Vec::new();
    let mut bytes = 0;
    let mut by = CutBy::Lines;
    for (i, line) in lines.iter().take(MOST_LINES).enumerate() {
        let size = line.len() + usize::from(i > 0);
        if bytes + size > MOST_BYTES {
            by = CutBy::Bytes;
            break;
        }
        kept.push(*line);
        bytes += size;
    }
    // Without a byte break, only omitted lines prove the line limit
    // was reached.
    if by != CutBy::Bytes && kept.len() == lines.len() {
        by = CutBy::Bytes;
    }
    let content = kept.join("\n");
    truncation.by = Some(by);
    truncation.output_lines = kept.len() as u64;
    truncation.output_bytes = content.len() as u64;
    (content, truncation)
}

/// The last character boundary at or before `index`.
fn character_end(bytes: &[u8], index: usize) -> usize {
    let mut end = index;
    while end > 0 && bytes.get(end).is_some_and(|b| b & 0xc0 == 0x80)
    {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(count: usize, width: usize) -> String {
        vec!["x".repeat(width); count].join("\n")
    }

    /// A read keeps at most 2000 lines, and says the line limit cut
    /// it only when lines were left out.
    #[test]
    fn a_read_is_cut_at_two_thousand_lines() {
        let (kept, cut) = truncate_head(&lines(2500, 1));
        assert_eq!(kept, lines(2000, 1));
        assert_eq!(cut.by, Some(CutBy::Lines));
        assert_eq!((cut.total_lines, cut.output_lines), (2500, 2000));

        let whole = lines(2000, 1);
        let (kept, cut) = truncate_head(&whole);
        assert_eq!(kept, whole);
        assert_eq!(cut.by, None);
    }

    /// Past 50 KiB a read keeps the whole lines that fit, and a file
    /// that fits all its lines but one byte over, with its last
    /// newline, is still cut by bytes.
    #[test]
    fn a_read_is_cut_at_fifty_kilobytes() {
        // 51 lines of 1000 bytes and their 50 newlines fit in 51200.
        let (kept, cut) = truncate_head(&lines(100, 1000));
        assert_eq!(cut.by, Some(CutBy::Bytes));
        assert_eq!((cut.output_lines, cut.output_bytes), (51, 51050));
        assert_eq!(kept, lines(51, 1000));

        let (kept, cut) =
            truncate_head(&format!("{}\n", "a".repeat(MOST_BYTES)));
        assert_eq!(cut.by, Some(CutBy::Bytes));
        assert_eq!(kept, "a".repeat(MOST_BYTES));
        assert!(!cut.first_line_exceeds_limit);
    }

    /// A first line over 50 KiB keeps nothing, and the read shows its
    /// start, cut at a character's start.
    #[test]
    fn a_first_line_too_long_keeps_nothing() {
        let (kept, cut) = truncate_head(&format!(
            "{}\nend",
            "x".repeat(MOST_BYTES + 1)
        ));
        assert_eq!(kept, "");
        assert!(cut.first_line_exceeds_limit);
        assert_eq!(cut.by, Some(CutBy::Bytes));
        assert_eq!((cut.output_lines, cut.output_bytes), (0, 0));

        // Byte 51200 of "a" and two-byte characters is the second
        // byte of one.
        let line = format!("a{}", "\u{e9}".repeat(30_000));
        assert_eq!(
            character_end(line.as_bytes(), MOST_BYTES),
            51_199
        );
        assert_eq!(character_end(b"abc", MOST_BYTES), MOST_BYTES);
    }

    /// Sizes as pi writes them, a half rounded up.
    #[test]
    fn sizes_are_written_as_pi_writes_them() {
        assert_eq!(format_size(1023), "1023B");
        assert_eq!(format_size(1024), "1.0KB");
        assert_eq!(format_size(1280), "1.3KB");
        assert_eq!(format_size(60_001), "58.6KB");
        assert_eq!(format_size(3 * 1024 * 1024 / 2), "1.5MB");
    }

    /// An image is told by its first bytes, and only a whole header
    /// counts.
    #[test]
    fn an_image_is_told_by_its_first_bytes() {
        let mut png =
            vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend(13u32.to_be_bytes());
        png.extend(b"IHDR");
        let mut no_header = png.clone();
        no_header[12..16].copy_from_slice(b"IDAT");
        let mut webp = b"RIFF".to_vec();
        webp.extend([0; 4]);
        webp.extend(b"WEBP");

        assert_eq!(image(&png), Some("image/png"));
        assert_eq!(image(&no_header), None);
        assert_eq!(image(&png[..12]), None);
        assert_eq!(
            image(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(image(&[0xff, 0xd8, 0xff, 0xf7]), None);
        assert_eq!(image(b"GIF89a..."), Some("image/gif"));
        assert_eq!(image(b"GIF87a"), Some("image/gif"));
        assert_eq!(image(&webp), Some("image/webp"));
        assert_eq!(image(b"RIFF"), None);
        assert_eq!(image(b"plain text"), None);
    }

    /// A screenshot's name as macOS writes it, with a narrow no-break
    /// space before its meridiem.
    #[test]
    fn a_meridiem_gets_a_narrow_space() {
        assert_eq!(
            narrow_meridiem("/a/Shot at 9.41.10 PM.png"),
            "/a/Shot at 9.41.10\u{202f}PM.png"
        );
        assert_eq!(narrow_meridiem("/a/b am.c"), "/a/b\u{202f}am.c");
        assert_eq!(narrow_meridiem("/a/I am here"), "/a/I am here");
    }
}
