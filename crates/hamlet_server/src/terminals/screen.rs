//! What a terminal printed: its screen with the scrollback, the bytes
//! that draw it again, and the last command typed into it.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use hamlet_os::process::pty::Size;
use hamlet_protocol::client::{Replayed, replayed, size_mark};

/// Bytes of output kept to redraw a terminal for a window that
/// begins watching it, and across restarts.
const REPLAY: usize = 512 * 1024;

/// Lines an agent's read of a terminal can reach above its screen.
pub(super) const SCROLLBACK: usize = 2000;
/// The most characters of a last command kept.
pub(super) const LAST: usize = 200;

/// What a terminal printed, and what a window needs to draw it again.
pub(crate) struct Screen {
    pub(super) parser: vt100::Parser<Titled>,
    /// What it printed, with a size mark where it was resized.
    replay: VecDeque<u8>,
    /// Its size where `replay` starts.
    start: Size,
    /// Where the line being typed starts: the cursor, as a row
    /// counted from the top of the scrollback and a column, when
    /// typing began after a prompt.
    typing: Option<(usize, u16)>,
    /// Where a line entered starts, read once the shell has echoed
    /// it.
    entered: Option<(usize, u16)>,
    /// The last command typed into it or run in it.
    pub(super) last: Option<String>,
    pub(super) seq: u64,
    pub(super) ended: Option<i64>,
}

#[derive(Default)]
pub(super) struct Titled(pub(super) Option<String>);

impl vt100::Callbacks for Titled {
    fn set_window_title(
        &mut self,
        _: &mut vt100::Screen,
        title: &[u8],
    ) {
        self.0 = Some(String::from_utf8_lossy(title).into_owned());
    }
}

/// `n` cells, at least one.
fn cells(n: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(n).unwrap_or(std::num::NonZeroU16::MIN)
}

pub(super) fn lock(
    screen: &Mutex<Screen>,
) -> std::sync::MutexGuard<'_, Screen> {
    screen.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Screen {
    pub(super) fn new(size: Size) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(
                cells(size.rows),
                cells(size.cols),
                SCROLLBACK,
                Titled::default(),
            ),
            replay: VecDeque::new(),
            start: size,
            typing: None,
            entered: None,
            last: None,
            seq: 0,
            ended: None,
        }
    }

    /// Takes the bytes a redraw of it holds, at their sizes.
    pub(super) fn restore(&mut self, bytes: &[u8]) {
        for part in replayed(bytes) {
            match part {
                Replayed::Printed(bytes) => self.take(bytes),
                Replayed::Sized { cols, rows } => {
                    self.resize(Size { rows, cols });
                }
            }
        }
    }

    pub(super) fn take(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        // The line entered is read at the echo of its Enter, before
        // what the command prints scrolls it away.
        if self.entered.is_some()
            && let Some(at) = rest.iter().position(|b| *b == b'\n')
        {
            self.parser.process(&rest[..=at]);
            self.read_entered();
            rest = &rest[at + 1..];
        }
        self.parser.process(rest);
        self.replay.extend(bytes);
        if self.replay.len() > REPLAY {
            let over = self.replay.len() - REPLAY;
            // Starts the kept bytes at a line, not inside a sequence.
            let cut = self
                .replay
                .range(over..)
                .position(|byte| *byte == b'\n')
                .map_or(self.replay.len(), |at| over + at + 1);
            let dropped =
                self.replay.drain(..cut).collect::<Vec<_>>();
            let sized = replayed(&dropped)
                .into_iter()
                .rev()
                .find_map(|part| match part {
                    Replayed::Sized { cols, rows } => {
                        Some(Size { rows, cols })
                    }
                    Replayed::Printed(_) => None,
                });
            if let Some(size) = sized {
                self.start = size;
            }
        }
        self.seq += 1;
    }

    /// The bytes that draw it again from a blank screen, each at the
    /// size it was printed at.
    pub(super) fn replay(&self) -> Vec<u8> {
        let mut bytes = size_mark(self.start.cols, self.start.rows);
        bytes.extend(self.replay.iter());
        bytes
    }

    /// The cursor, its row counted from the top of the scrollback.
    fn cursor(&self) -> (usize, u16) {
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        (screen.scrollback_len() + usize::from(row), col)
    }

    /// Notes what `bytes` typed means for the last command: where the
    /// line starts, as typing begins after a prompt, and the line
    /// entered with Enter. A full-screen program's keys are not
    /// commands.
    pub(super) fn typed(&mut self, bytes: &[u8]) {
        if self.parser.screen().alternate_screen() {
            self.typing = None;
            return;
        }
        // Ctrl+C and Ctrl+L leave the line where it was.
        if bytes.iter().any(|b| matches!(b, 0x03 | 0x0c)) {
            self.typing = None;
            return;
        }
        let cursor = self.cursor();
        let start = *self.typing.get_or_insert(cursor);
        if bytes.contains(&b'\r') {
            self.entered = Some(start);
            self.typing = None;
        }
    }

    /// Reads the line entered once the cursor has left it.
    fn read_entered(&mut self) {
        let Some((row, col)) = self.entered else {
            return;
        };
        if self.cursor().0 <= row {
            return;
        }
        self.entered = None;
        let line = self.line_from(row, col);
        if !line.is_empty() {
            self.last = Some(line);
        }
    }

    /// The line that starts at `row` and `col`, its wrapped rows
    /// joined, without what a prompt shows at its right.
    fn line_from(&mut self, row: usize, col: u16) -> String {
        let screen = self.parser.screen_mut();
        let back = screen.scrollback_len();
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());
        let mut at = if row >= back {
            u16::try_from(row - back).unwrap_or(rows)
        } else {
            screen.set_scrollback(back - row);
            0
        };
        let mut text = String::new();
        let mut from = col;
        while at < rows {
            text.push_str(
                &screen.contents_between(at, from, at, cols),
            );
            if !screen.row_wrapped(at) {
                break;
            }
            at += 1;
            from = 0;
        }
        screen.set_scrollback(0);
        let text = text.split("    ").next().unwrap_or_default();
        text.trim().chars().take(LAST).collect()
    }

    pub(super) fn resize(&mut self, size: Size) {
        let (rows, cols) = self.parser.screen().size();
        if (rows, cols) == (cells(size.rows), cells(size.cols)) {
            return;
        }
        // Rewrapping moves the lines being typed.
        self.typing = None;
        self.entered = None;
        hamlet_protocol::screen::resize(
            &mut self.parser,
            cells(size.rows),
            cells(size.cols),
        );
        self.replay.extend(size_mark(size.cols, size.rows));
    }

    /// The text it shows, with up to `above` lines of scrollback.
    pub(crate) fn text(&mut self, above: usize) -> String {
        let screen = self.parser.screen_mut();
        screen.set_scrollback(above);
        let shown = screen.scrollback();
        let (rows, cols) = screen.size();
        let (rows, cols) = (usize::from(rows.get()), cols.get());
        let mut lines =
            screen.rows(0, cols).take(rows).collect::<Vec<_>>();
        if shown > 0 {
            screen.set_scrollback(0);
            let now = screen.rows(0, cols).collect::<Vec<_>>();
            let skip = rows.saturating_sub(shown);
            lines.extend(now.into_iter().skip(skip));
        }
        while lines.last().is_some_and(|line| line.trim().is_empty())
        {
            lines.pop();
        }
        lines
            .iter()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn ended(&self) -> Option<i64> {
        self.ended
    }

    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_reads_the_screen_with_the_scrollback_it_asks_for() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        for line in 1..=6 {
            screen.take(format!("line {line}\r\n").as_bytes());
        }
        assert_eq!(screen.text(0), "line 5\nline 6");
        assert_eq!(screen.text(2), "line 3\nline 4\nline 5\nline 6");
        assert_eq!(screen.text(100).lines().next(), Some("line 1"));
    }

    #[test]
    fn the_replay_keeps_its_last_bytes_from_the_start_of_a_line() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        let line = format!("{}\n", "x".repeat(1023));
        for _ in 0..(REPLAY / 1024 + 10) {
            screen.take(line.as_bytes());
        }
        let replay = screen.replay();
        let mark = size_mark(20, 3);
        assert!(replay.starts_with(&mark));
        let replay = &replay[mark.len()..];
        assert!(replay.len() <= REPLAY, "{}", replay.len());
        assert!(replay.len() > REPLAY - 1024);
        assert_eq!(replay[0], b'x');
        assert_eq!(replay[replay.len() - 1], b'\n');
    }

    #[test]
    fn a_redraw_draws_each_part_at_the_size_it_was_printed_at() {
        let mut screen = Screen::new(Size { rows: 3, cols: 4 });
        screen.take(b"abcdef");
        screen.resize(Size { rows: 3, cols: 8 });
        screen.resize(Size { rows: 3, cols: 8 });
        screen.take(b"\r\ngh");
        let replay = screen.replay();
        assert_eq!(
            replayed(&replay),
            [
                Replayed::Sized { cols: 4, rows: 3 },
                Replayed::Printed(b"abcdef"),
                Replayed::Sized { cols: 8, rows: 3 },
                Replayed::Printed(b"\r\ngh"),
            ]
        );
        let mut again = Screen::new(Size { rows: 3, cols: 8 });
        again.restore(&replay);
        assert_eq!(again.text(0), "abcdef\ngh");
        assert_eq!(again.text(0), screen.text(0));
    }

    /// Types `keys` into `screen`, and has the shell echo `echo`.
    fn typed(screen: &mut Screen, keys: &str, echo: &str) {
        screen.typed(keys.as_bytes());
        screen.take(echo.as_bytes());
    }

    #[test]
    fn the_last_command_is_the_line_typed_after_the_prompt() {
        let mut screen = Screen::new(Size { rows: 4, cols: 12 });
        screen.take(b"$ ");
        typed(&mut screen, "l", "l");
        typed(&mut screen, "s -la /tmp", "s -la /tmp");
        typed(&mut screen, "\r", "\r\n");
        screen.take(b"total 0\r\n$ ");
        assert_eq!(screen.last.as_deref(), Some("ls -la /tmp"));
        // A prompt that hides what is typed shows no command.
        screen.take(b"pass: ");
        typed(&mut screen, "secret\r", "\r\n");
        assert_eq!(screen.last.as_deref(), Some("ls -la /tmp"));
        // Nor what is typed at a full-screen program.
        screen.take(b"\x1b[?1049h");
        typed(&mut screen, "q\r", "\r\n");
        assert_eq!(screen.last.as_deref(), Some("ls -la /tmp"));
    }

    #[test]
    fn the_last_command_leaves_out_what_a_prompt_shows_at_its_right()
    {
        let mut screen = Screen::new(Size { rows: 3, cols: 30 });
        screen.take(b"% \x1b[s\x1b[1;25H~/src\x1b[u");
        typed(&mut screen, "make\r", "make\r\n");
        assert_eq!(screen.last.as_deref(), Some("make"));
    }

    #[test]
    fn a_trimmed_replay_starts_at_the_size_its_first_kept_byte_had() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        screen.take(b"first\n");
        screen.resize(Size { rows: 4, cols: 30 });
        let line = format!("{}\n", "x".repeat(1023));
        for _ in 0..(REPLAY / 1024 + 10) {
            screen.take(line.as_bytes());
        }
        assert!(screen.replay().starts_with(&size_mark(30, 4)));
    }
}
