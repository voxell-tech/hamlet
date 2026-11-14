//! A command's output: the tail its end carries and, once it passes
//! the tail's limits, all of it in a spill file the agent may read.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use hamlet_messages::workspace::Cut;

use crate::workspace::{MOST_BYTES, MOST_LINES};

/// The most of a command's output a spill file holds.
pub(super) const SPILL_CAP: u64 = 512 * 1024 * 1024;

/// What a command printed, as it comes, its secrets already taken
/// out.
pub(super) struct Output {
    tail: Tail,
    bytes: u64,
    newlines: u64,
    /// The last of it ends a line.
    ends_a_line: bool,
    spill: Spill,
}

/// Where all of the output goes once it passes the tail's limits.
struct Spill {
    path: PathBuf,
    cap: u64,
    file: Option<BufWriter<File>>,
    written: u64,
    /// What was written ends a line, or nothing was.
    at_line_start: bool,
    dropped: bool,
    failed: Option<io::Error>,
}

impl Output {
    /// Output that spills to `path`, keeping at most `cap` bytes
    /// there.
    pub(super) fn new(path: PathBuf, cap: u64) -> Self {
        Self {
            tail: Tail::default(),
            bytes: 0,
            newlines: 0,
            ends_a_line: true,
            spill: Spill {
                path,
                cap,
                file: None,
                written: 0,
                at_line_start: true,
                dropped: false,
                failed: None,
            },
        }
    }

    pub(super) fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.bytes += text.len() as u64;
        self.newlines += text.matches('\n').count() as u64;
        self.ends_a_line = text.ends_with('\n');
        if self.spill.file.is_none() && self.spill.failed.is_none() {
            if self.bytes <= MOST_BYTES as u64
                && self.lines() <= MOST_LINES as u64
            {
                self.tail.push(text);
                return;
            }
            // All it printed before this is still in the tail, which
            // drops nothing below its limits.
            let before = std::mem::take(&mut self.tail.kept);
            self.spill.write(&before);
            self.tail.kept = before;
        }
        self.spill.write(text);
        self.tail.push(text);
    }

    fn lines(&self) -> u64 {
        self.newlines + u64::from(!self.ends_a_line)
    }

    /// The tail, and what was cut from it, or why the output could
    /// not be kept.
    pub(super) fn finish(
        self,
    ) -> (String, Result<Option<Cut>, Unsaved>) {
        let tail = self.tail.text();
        let total_lines = self.lines();
        let cut = self.spill.finish().map(|spilled| {
            spilled.map(|full_output| Cut {
                total_bytes: self.bytes,
                total_lines,
                full_output,
            })
        });
        (tail, cut.map_err(Unsaved))
    }
}

/// A command's full output that could not be saved.
#[derive(Debug, thiserror::Error)]
#[error(
    "The command ran but its full output could not be saved: {0}. \
     Inspect its files before repeating it."
)]
pub(super) struct Unsaved(io::Error);

impl Spill {
    fn write(&mut self, text: &str) {
        if self.dropped || self.failed.is_some() {
            return;
        }
        let room = self.cap - self.written;
        let (kept, dropped) = if text.len() as u64 <= room {
            (text, false)
        } else {
            let mut at = usize::try_from(room).unwrap_or(usize::MAX);
            while !text.is_char_boundary(at) {
                at -= 1;
            }
            (&text[..at], true)
        };
        let note = dropped.then(|| {
            let newline = if kept.is_empty() && self.at_line_start
                || kept.ends_with('\n')
            {
                ""
            } else {
                "\n"
            };
            format!(
                "{newline}[output past {} was dropped]\n",
                size(self.cap)
            )
        });
        let written = self.file().and_then(|file| {
            file.write_all(kept.as_bytes())?;
            file.write_all(note.as_deref().unwrap_or("").as_bytes())
        });
        match written {
            Ok(()) => {
                self.written += kept.len() as u64;
                if !kept.is_empty() {
                    self.at_line_start = kept.ends_with('\n');
                }
                self.dropped = dropped;
            }
            Err(error) => self.failed = Some(error),
        }
    }

    /// The spill file, made on first use: only the workspace writes
    /// it, and only its agent reads it.
    fn file(&mut self) -> io::Result<&mut BufWriter<File>> {
        if self.file.is_none() {
            if let Some(folder) = self.path.parent() {
                let mut builder = DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                std::os::unix::fs::DirBuilderExt::mode(
                    &mut builder,
                    0o700,
                );
                builder.create(folder)?;
            }
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(
                &mut options,
                0o600,
            );
            let file = options.open(&self.path)?;
            self.file =
                Some(BufWriter::with_capacity(64 * 1024, file));
        }
        Ok(self.file.as_mut().expect("the file was just made"))
    }

    /// The spill file's path, once it holds the output.
    fn finish(self) -> io::Result<Option<PathBuf>> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        match self.file {
            None => Ok(None),
            Some(mut file) => {
                file.flush()?;
                Ok(Some(self.path))
            }
        }
    }
}

/// `bytes` in whole MiB where it is some, else in bytes.
fn size(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes.is_multiple_of(MIB) && bytes > 0 {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} bytes")
    }
}

/// The running output of a command, of which its last lines are kept.
// Copied from the server's `truncate::Tail`.
#[derive(Default)]
struct Tail {
    kept: String,
}

impl Tail {
    fn push(&mut self, text: &str) {
        self.kept.push_str(text);
        if self.kept.len() > 4 * MOST_BYTES {
            self.kept = self.text();
        }
    }

    /// The last [`MOST_LINES`] lines within [`MOST_BYTES`], a line
    /// cut by the byte limit dropped whole when a newline follows it,
    /// and without the control characters that break display; tabs
    /// and newlines stay.
    fn text(&self) -> String {
        let kept = self.kept.as_str();
        let body = kept.strip_suffix('\n').unwrap_or(kept);
        let mut start = body
            .rmatch_indices('\n')
            .nth(MOST_LINES - 1)
            .map_or(0, |(at, _)| at + 1);
        if kept.len() - start > MOST_BYTES {
            let mut from = kept.len() - MOST_BYTES;
            while !kept.is_char_boundary(from) {
                from += 1;
            }
            start = match kept[from..].find('\n') {
                Some(at) if from + at + 1 < kept.len() => {
                    from + at + 1
                }
                _ => from,
            };
        }
        kept[start..]
            .chars()
            .filter(|&c| {
                !matches!(
                    c,
                    '\0'..='\u{8}' | '\u{b}'..='\u{1f}' | '\u{fff9}'..='\u{fffb}'
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spilled(
        cap: u64,
        chunks: &[&str],
    ) -> (String, Option<Cut>, String) {
        let scratch = hamlet_testing::Scratch::new("output");
        let path = scratch.join("7/1.log");
        let mut output = Output::new(path.clone(), cap);
        for chunk in chunks {
            output.push(chunk);
        }
        let (tail, cut) = output.finish();
        let cut = cut.unwrap();
        let file = std::fs::read_to_string(&path).unwrap_or_default();
        (tail, cut, file)
    }

    #[test]
    fn output_within_the_tail_spills_nothing() {
        let lines = "x\n".repeat(MOST_LINES);
        let (tail, cut, file) = spilled(SPILL_CAP, &[&lines]);
        assert_eq!((tail, cut, file), (lines, None, String::new()));
    }

    #[test]
    fn the_spill_holds_all_until_its_cap_then_a_note() {
        let first = "a".repeat(MOST_BYTES);
        let (tail, cut, file) =
            spilled(MOST_BYTES as u64 + 4, &[&first, "bcdefgh\n"]);
        let cut = cut.expect("a cut");
        assert_eq!(
            file,
            format!(
                "{first}bcde\n[output past 51204 bytes was \
                 dropped]\n"
            )
        );
        assert_eq!((cut.total_bytes, cut.total_lines), (51208, 1));
        assert!(tail.ends_with("bcdefgh\n"));
    }

    #[test]
    fn a_line_past_the_line_limit_opens_the_spill() {
        let lines: Vec<String> =
            (0..=MOST_LINES).map(|n| format!("{n}\n")).collect();
        let chunks: Vec<&str> =
            lines.iter().map(String::as_str).collect();
        let (_, cut, file) = spilled(SPILL_CAP, &chunks);
        assert_eq!(cut.expect("a cut").total_lines, 2001);
        assert_eq!(file, lines.concat());
    }

    #[test]
    fn a_spill_that_cannot_be_written_says_so() {
        let scratch =
            hamlet_testing::Scratch::new("output-unwritable");
        std::fs::write(scratch.join("7"), "a file, not a folder")
            .unwrap();
        let mut output =
            Output::new(scratch.join("7/1.log"), SPILL_CAP);
        output.push(&"y\n".repeat(MOST_LINES + 1));
        let (_, cut) = output.finish();
        assert!(cut.unwrap_err().to_string().starts_with(
            "The command ran but its full output could not be saved"
        ));
    }

    #[test]
    fn the_tail_keeps_the_last_lines_within_the_limits() {
        let mut tail = Tail::default();
        for n in 0..300_000 {
            tail.push(&format!("{n}\n"));
        }
        let kept = tail.text();
        assert!(kept.starts_with("298000\n"), "{}", &kept[..20]);
        assert!(kept.ends_with("299999\n"));
        let mut wide = Tail::default();
        wide.push(&format!("{}\nend", "x".repeat(60 * 1024)));
        assert_eq!(wide.text(), "end");
        let mut long = Tail::default();
        long.push(&"\u{e9}".repeat(30 * 1024));
        assert_eq!(long.text().len(), MOST_BYTES);
    }
}
