//! A terminal's screen as both ends of the client protocol keep it,
//! so a redraw's size marks ([`crate::client::replayed`]) mean the
//! same resize to each.

use std::fmt::Write;
use std::num::NonZeroU16;

use vt100::{Callbacks, Color, Parser};

/// Makes the screen `cols` by `rows`, as a terminal window does: what
/// was printed is wrapped again at the new width, colours kept, rows
/// that only wrapped joined first, and the cursor stays by the letter
/// it was at. What had scrolled off stays off. The alternate screen
/// is only cut, since its program draws it again for the new size.
pub fn resize<CB: Callbacks>(
    parser: &mut Parser<CB>,
    rows: NonZeroU16,
    cols: NonZeroU16,
) {
    let screen = parser.screen();
    let (old_rows, old_cols) = screen.size();
    if (old_rows, old_cols) == (rows, cols) {
        return;
    }
    if screen.alternate_screen() || cols == old_cols {
        parser.screen_mut().set_size(rows, cols);
        return;
    }
    let attributes = screen.attributes_formatted();
    let (above, shown, cursor) = lines(parser);
    let screen = parser.screen_mut();
    screen.set_size(rows, cols);
    screen.clear_scrollback();
    parser.process(b"\x1b[m\x1b[H\x1b[2J");
    if !above.is_empty() {
        for (at, line) in above.iter().enumerate() {
            if at > 0 {
                parser.process(b"\r\n");
            }
            parser.process(line.text(line.trimmed()).as_bytes());
        }
        // Scrolls exactly the rows they took off the screen.
        let took = parser.screen().cursor_position().0 + 1;
        let mut push = format!("\x1b[m\x1b[{};1H", rows.get());
        push.push_str(&"\n".repeat(usize::from(took)));
        push.push_str("\x1b[H");
        parser.process(push.as_bytes());
    }
    let last = shown
        .iter()
        .rposition(|line| line.trimmed() > 0)
        .map_or(cursor.0, |last| last.max(cursor.0));
    let mut at_cursor = None;
    for (at, line) in shown.iter().enumerate().take(last + 1) {
        if at > 0 {
            parser.process(b"\r\n");
        }
        if at == cursor.0 {
            // Where the line starts, since a full row leaves the
            // cursor waiting to wrap rather than on the next.
            at_cursor = Some((
                parser.screen().cursor_position().0,
                parser.screen().scrollback_len(),
            ));
            let end = line.trimmed().max(cursor.1);
            parser.process(line.text(end).as_bytes());
        } else {
            parser.process(line.text(line.trimmed()).as_bytes());
        }
    }
    let mut end = String::from("\x1b[m");
    if let Some((start, scrolled)) = at_cursor {
        let width = usize::from(cols.get());
        let since = parser.screen().scrollback_len() - scrolled;
        let row = (usize::from(start) + cursor.1 / width)
            .saturating_sub(since)
            .min(usize::from(rows.get()) - 1);
        let col = cursor.1 % width;
        let _ = write!(end, "\x1b[{};{}H", row + 1, col + 1);
    }
    end.push_str(&attributes);
    parser.process(end.as_bytes());
}

/// A line as printed, before it was wrapped to the screen.
#[derive(Default)]
struct Line {
    cells: Vec<Cell>,
}

#[derive(Clone, PartialEq)]
struct Cell {
    /// Empty for the right half of a wide letter.
    text: String,
    look: Look,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Look {
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Line {
    /// The cells up to the last that shows anything.
    fn trimmed(&self) -> usize {
        self.cells
            .iter()
            .rposition(|cell| {
                cell.look != Look::default()
                    || !cell.text.trim().is_empty()
            })
            .map_or(0, |last| last + 1)
    }

    fn text(&self, end: usize) -> String {
        self.slice(0, end)
    }

    /// The bytes that print cells `from..to`, from plain attributes.
    fn slice(&self, from: usize, to: usize) -> String {
        let mut out = String::new();
        let mut look = None;
        for cell in self.cells.iter().take(to).skip(from) {
            if cell.text.is_empty() {
                continue;
            }
            if look != Some(cell.look) {
                out.push_str(&sgr(cell.look));
                look = Some(cell.look);
            }
            out.push_str(&cell.text);
        }
        if look.is_some() {
            out.push_str("\x1b[m");
        }
        out
    }
}

/// The lines in the scrollback, the lines on the screen, and the
/// cursor as a line among them and a cell in it.
fn lines<CB: Callbacks>(
    parser: &mut Parser<CB>,
) -> (Vec<Line>, Vec<Line>, (usize, usize)) {
    let screen = parser.screen_mut();
    let was = screen.scrollback();
    let (rows, cols) = screen.size();
    let (rows, cols) = (usize::from(rows.get()), cols.get());
    screen.set_scrollback(usize::MAX);
    let back = screen.scrollback();
    let mut above = Vec::new();
    let mut line = Line::default();
    // The scrollback a screen at a time, oldest first.
    let mut from = 0;
    while from < back {
        let offset = back - from;
        screen.set_scrollback(offset);
        for row in 0..rows.min(offset) {
            row_into(screen, row as u16, cols, &mut line);
            if !screen.row_wrapped(row as u16)
                || from + row + 1 == back
            {
                above.push(std::mem::take(&mut line));
            }
        }
        from += rows.min(offset);
    }
    screen.set_scrollback(0);
    let (cursor_row, cursor_col) = screen.cursor_position();
    let mut shown = Vec::new();
    let mut cursor = (0, 0);
    for row in 0..rows as u16 {
        if row == cursor_row {
            cursor = (
                shown.len(),
                line.cells.len() + usize::from(cursor_col),
            );
        }
        row_into(screen, row, cols, &mut line);
        if !screen.row_wrapped(row) || usize::from(row) + 1 == rows {
            shown.push(std::mem::take(&mut line));
        }
    }
    screen.set_scrollback(was);
    (above, shown, cursor)
}

fn row_into(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    line: &mut Line,
) {
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        let text = if cell.is_wide_continuation() {
            String::new()
        } else if cell.has_contents() {
            cell.contents().to_owned()
        } else {
            " ".to_owned()
        };
        line.cells.push(Cell {
            text,
            look: Look {
                fg: cell.fgcolor(),
                bg: cell.bgcolor(),
                bold: cell.bold(),
                dim: cell.dim(),
                italic: cell.italic(),
                underline: cell.underline(),
                inverse: cell.inverse(),
            },
        });
    }
}

/// The SGR sequence that sets exactly `look`.
fn sgr(look: Look) -> String {
    let mut out = String::from("\x1b[0");
    for (on, code) in [
        (look.bold, "1"),
        (look.dim, "2"),
        (look.italic, "3"),
        (look.underline, "4"),
        (look.inverse, "7"),
    ] {
        if on {
            out.push(';');
            out.push_str(code);
        }
    }
    colour(&mut out, look.fg, 30, 90, 38);
    colour(&mut out, look.bg, 40, 100, 48);
    out.push('m');
    out
}

fn colour(
    out: &mut String,
    color: Color,
    low: u8,
    bright: u8,
    any: u8,
) {
    let _ = match color {
        Color::Default => Ok(()),
        Color::Idx(i) if i < 8 => write!(out, ";{}", low + i),
        Color::Idx(i) if i < 16 => write!(out, ";{}", bright + i - 8),
        Color::Idx(i) => write!(out, ";{any};5;{i}"),
        Color::Rgb(r, g, b) => write!(out, ";{any};2;{r};{g};{b}"),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    fn rows(parser: &Parser) -> Vec<String> {
        let (_, cols) = parser.screen().size();
        parser
            .screen()
            .rows(0, cols.get())
            .map(|row| row.trim_end().to_owned())
            .collect()
    }

    #[test]
    fn a_long_line_wraps_again_at_the_new_width_and_keeps_its_colour()
    {
        let mut parser = Parser::new(size(4), size(10), 100);
        parser.process(b"$ \x1b[31mabcdefghijklmn\x1b[m\r\n$ ");
        assert_eq!(rows(&parser), ["$ abcdefgh", "ijklmn", "$", ""]);
        resize(&mut parser, size(4), size(20));
        assert_eq!(rows(&parser), ["$ abcdefghijklmn", "$", "", ""]);
        assert_eq!(parser.screen().cursor_position(), (1, 2));
        let red = parser.screen().cell(0, 5).unwrap().fgcolor();
        assert_eq!(red, Color::Idx(1));
        resize(&mut parser, size(4), size(6));
        assert_eq!(rows(&parser), ["$ abcd", "efghij", "klmn", "$"]);
        assert_eq!(parser.screen().cursor_position(), (3, 2));
    }

    #[test]
    fn what_scrolled_off_stays_off_and_wraps_again_too() {
        let mut parser = Parser::new(size(2), size(6), 100);
        parser.process(b"abcdefgh\r\none\r\ntwo\r\n> ");
        resize(&mut parser, size(2), size(10));
        assert_eq!(rows(&parser), ["two", ">"]);
        assert_eq!(parser.screen().cursor_position(), (1, 2));
        parser.screen_mut().set_scrollback(2);
        assert_eq!(rows(&parser), ["abcdefgh", "one"]);
    }

    #[test]
    fn the_cursor_keeps_its_letter_inside_a_line_and_the_alternate_screen_is_cut()
     {
        let mut parser = Parser::new(size(3), size(8), 100);
        parser.process(b"0123456789\x1b[2D");
        resize(&mut parser, size(3), size(4));
        assert_eq!(rows(&parser), ["0123", "4567", "89"]);
        assert_eq!(parser.screen().cursor_position(), (2, 0));
        parser.process(b"\x1b[?1049hfull");
        resize(&mut parser, size(3), size(2));
        assert_eq!(rows(&parser)[0], "fu");
    }
}
