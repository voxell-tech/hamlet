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
    /// What the line being typed showed before typing began on it:
    /// the prompt. The line is found again by it, not by where it
    /// was, which scrolling and a full scrollback move.
    typing: Option<String>,
    /// The prompt of a line entered, read once the shell has echoed
    /// its Enter.
    entered: Option<String>,
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

/// The first row of the line `row` is part of, its rows before it
/// wrapped into it.
fn first_row(screen: &vt100::Screen, mut row: u16) -> u16 {
    while row > 0 && screen.row_wrapped(row - 1) {
        row -= 1;
    }
    row
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

    /// The bytes that bring a window watching it to what it is now:
    /// the replay, then what the replay may have lost to its trim,
    /// which a program sets once as it starts. On the alternate
    /// screen, that screen as it shows now; then the modes the
    /// program asked for, such as reading the mouse and bracketed
    /// paste. A restored terminal takes [`Self::replay`], since its
    /// shell starts again without them.
    pub(super) fn redraw(&self) -> Vec<u8> {
        let mut bytes = self.replay();
        let screen = self.parser.screen();
        if screen.alternate_screen() {
            bytes.extend(b"\x1b[?1049h");
            bytes.extend(screen.contents_formatted().as_bytes());
        }
        bytes.extend(screen.input_mode_formatted().as_bytes());
        bytes
    }

    /// What the cursor's line shows before the cursor, its wrapped
    /// rows joined.
    fn before_cursor(&self) -> String {
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        screen.contents_between(first_row(screen, row), 0, row, col)
    }

    /// Notes what `bytes` typed means for the last command: the
    /// prompt of the line, as typing begins on it, and the line
    /// entered with Enter. Keys typed at a program that left the
    /// line, such as the `q` that ends a pager, are not taken for
    /// the next prompt's: a line that no longer shows its prompt
    /// begins again. A full-screen program's keys are not
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
        let before = self.before_cursor();
        let kept = self.typing.as_ref().is_some_and(|prompt| {
            before.starts_with(prompt.trim_end())
        });
        if !kept {
            self.typing = Some(before);
        }
        if bytes.contains(&b'\r') {
            self.entered = self.typing.take();
        }
    }

    /// Reads the line entered, which ends on the row above the cursor
    /// once its Enter has been echoed, without its prompt and what a
    /// prompt shows at its right. A line that does not show the
    /// prompt typing began after is not read.
    fn read_entered(&mut self) {
        let Some(prompt) = self.entered.take() else {
            return;
        };
        let screen = self.parser.screen();
        let (row, _) = screen.cursor_position();
        let Some(end) = row.checked_sub(1) else {
            return;
        };
        let cols = screen.size().1.get();
        let line = screen.contents_between(
            first_row(screen, end),
            0,
            end,
            cols,
        );
        let Some(typed) = line.strip_prefix(prompt.trim_end()) else {
            return;
        };
        let text =
            typed.split("    ").next().unwrap_or_default().trim();
        if !text.is_empty() {
            self.last = Some(text.chars().take(LAST).collect());
        }
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

    #[test]
    fn a_redraw_keeps_the_modes_and_the_screen_a_trim_cut_away() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        screen
            .take(b"$ copilot\r\n\x1b[?1049h\x1b[?1003h\x1b[?1006h");
        let frame = "\x1b[H\x1b[2Jframe\r\n".repeat(64);
        for _ in 0..REPLAY / frame.len() + 2 {
            screen.take(frame.as_bytes());
        }
        screen.take(b"\x1b[Htabs");
        assert!(
            !screen.replay().windows(8).any(|w| w == b"\x1b[?1003h")
        );
        let mut window = vt100::Parser::new(cells(3), cells(20), 0);
        for part in replayed(&screen.redraw()) {
            match part {
                Replayed::Printed(bytes) => window.process(bytes),
                Replayed::Sized { cols, rows } => {
                    window
                        .screen_mut()
                        .set_size(cells(rows), cells(cols));
                }
            }
        }
        let shown = window.screen();
        assert_eq!(
            shown.mouse_protocol_mode(),
            vt100::MouseProtocolMode::AnyMotion
        );
        assert_eq!(
            shown.mouse_protocol_encoding(),
            vt100::MouseProtocolEncoding::Sgr
        );
        assert!(shown.alternate_screen());
        assert_eq!(
            shown.contents(),
            screen.parser.screen().contents()
        );
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
    fn a_key_typed_at_a_pager_is_not_taken_for_the_next_command() {
        let mut screen = Screen::new(Size { rows: 4, cols: 30 });
        screen.take(b"diff --git a/x b/x\r\n:");
        // The q that ends the pager, which does not echo it.
        typed(&mut screen, "q", "\r\x1b[K");
        screen.take(b"me@host src % ");
        typed(&mut screen, "ls", "ls");
        typed(&mut screen, "\r", "\r\n");
        assert_eq!(screen.last.as_deref(), Some("ls"));
    }

    #[test]
    fn the_last_command_is_read_once_the_scrollback_is_full() {
        let mut screen = Screen::new(Size { rows: 4, cols: 30 });
        for line in 0..SCROLLBACK + 50 {
            screen.take(format!("{line}\r\n").as_bytes());
        }
        for command in ["git log", "ls"] {
            screen.take(b"% ");
            typed(&mut screen, command, command);
            typed(&mut screen, "\r", "\r\n");
            screen.take(b"a\r\nb\r\nc\r\n");
            assert_eq!(screen.last.as_deref(), Some(command));
        }
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
