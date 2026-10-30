//! Terminals the server runs: shells the chief and agents type into,
//! and the views of what they print.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{AgentRef, RequestId};

/// The most columns or rows a terminal takes.
pub const TERMINAL_SIZE: u16 = 1000;

/// A terminal the server runs, as the state lists it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TerminalView {
    /// The agent whose terminal it is. None when the chief opened
    /// it.
    #[serde(deserialize_with = "Option::deserialize")]
    pub agent: Option<AgentRef>,
    pub cols: u16,
    /// The folder its shell started in.
    pub cwd: String,
    /// The exit code of its shell, once the shell ended.
    #[serde(deserialize_with = "Option::deserialize")]
    pub exit: Option<i64>,
    /// The hamlet it belongs to. None for an unbounded terminal,
    /// which runs as the server's account with no sandbox.
    #[serde(deserialize_with = "Option::deserialize")]
    pub hamlet: Option<String>,
    pub id: String,
    pub kind: TerminalKind,
    pub name: String,
    pub rows: u16,
    /// Whether the sandbox holds its commands.
    pub sandboxed: bool,
    /// The title its programs last set, if any.
    pub title: String,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum TerminalKind {
    /// A shell that takes input.
    #[serde(rename = "shell")]
    Shell,
    /// The commands an agent's shell tool ran and what they printed.
    /// It takes no input.
    #[serde(rename = "runs")]
    Runs,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Terminal {
    /// Opens a shell in `hamlet`'s folder under the sandbox, or an
    /// unbounded one without a hamlet. Its first output answers `id`
    /// and names it.
    #[serde(rename = "open")]
    Open {
        #[schemars(range(min = 1, max = TERMINAL_SIZE))]
        cols: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        hamlet: Option<String>,
        id: RequestId,
        #[schemars(range(min = 1, max = TERMINAL_SIZE))]
        rows: u16,
    },
    /// Types `data` into a terminal, as keys and pastes give it.
    #[serde(rename = "input")]
    Input {
        #[schemars(length(max = 65_536))]
        data: String,
        terminal: String,
    },
    #[serde(rename = "resize")]
    Resize {
        #[schemars(range(min = 1, max = TERMINAL_SIZE))]
        cols: u16,
        #[schemars(range(min = 1, max = TERMINAL_SIZE))]
        rows: u16,
        terminal: String,
    },
    /// Sends what a terminal shows now, then what it prints, until
    /// `unwatch`.
    #[serde(rename = "watch")]
    Watch { id: RequestId, terminal: String },
    #[serde(rename = "unwatch")]
    Unwatch { terminal: String },
    /// Ends a terminal's processes and removes it.
    #[serde(rename = "close")]
    Close { id: RequestId, terminal: String },
}

/// Bytes a watched terminal printed.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TerminalOutput {
    /// The bytes, in base64. A redraw marks where the terminal
    /// changed size, as [`size_mark`] writes it, so each part is
    /// drawn at the size it was printed at.
    pub data: String,
    /// Whether `data` redraws the terminal from a blank one: the
    /// answer to a watch, or a catch-up after output was skipped.
    pub reset: bool,
    pub terminal: String,
    /// The open or watch this answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "RequestId")]
    pub to: Option<RequestId>,
}

/// The start of a size mark: an OSC no program is expected to print.
const MARK: &[u8] = b"\x1b]hamlet;size;";

/// The mark a redraw holds where its terminal became `cols` by
/// `rows`.
pub fn size_mark(cols: u16, rows: u16) -> Vec<u8> {
    format!("\x1b]hamlet;size;{cols};{rows}\x07").into_bytes()
}

/// A part of a redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replayed<'a> {
    /// Bytes the terminal printed.
    Printed(&'a [u8]),
    /// The terminal became `cols` by `rows` here.
    Sized { cols: u16, rows: u16 },
}

/// `bytes` in the parts [`size_mark`] splits them into. A mark that
/// does not read as a size is left in the printed bytes.
pub fn replayed(bytes: &[u8]) -> Vec<Replayed<'_>> {
    let mut parts = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while let Some(found) = find(&bytes[at..], MARK) {
        let start = at + found;
        let rest = &bytes[start + MARK.len()..];
        let size =
            rest.iter().position(|b| *b == 0x07).and_then(|end| {
                let text = std::str::from_utf8(&rest[..end]).ok()?;
                let (cols, rows) = text.split_once(';')?;
                Some((cols.parse().ok()?, rows.parse().ok()?, end))
            });
        let Some((cols, rows, end)) = size else {
            at = start + 1;
            continue;
        };
        if from < start {
            parts.push(Replayed::Printed(&bytes[from..start]));
        }
        parts.push(Replayed::Sized { cols, rows });
        from = start + MARK.len() + end + 1;
        at = from;
    }
    if from < bytes.len() {
        parts.push(Replayed::Printed(&bytes[from..]));
    }
    parts
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redraw_splits_at_its_size_marks_and_keeps_what_is_not_one() {
        let mut bytes = size_mark(80, 24);
        bytes.extend_from_slice(b"ab");
        bytes.extend(size_mark(40, 10));
        bytes.extend_from_slice(b"c\x1b]hamlet;size;x\x07d");
        assert_eq!(
            replayed(&bytes),
            [
                Replayed::Sized { cols: 80, rows: 24 },
                Replayed::Printed(b"ab"),
                Replayed::Sized { cols: 40, rows: 10 },
                Replayed::Printed(b"c\x1b]hamlet;size;x\x07d"),
            ]
        );
        assert_eq!(replayed(b""), []);
    }
}
