//! The bytes a terminal reads for the mouse, when a program asks for
//! it, as xterm sends them. After ratty's `encode_mouse_event`
//! (github.com/orhun/ratty, MIT), with xterm's modifier bits and the
//! UTF-8 form.

use vt100::{MouseProtocolEncoding, MouseProtocolMode};

use super::keys::Mods;

/// A mouse button, as xterm numbers them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    fn code(self) -> u16 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
        }
    }
}

/// What happened, at a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Report {
    Press(Button),
    Release(Button),
    /// The pointer moved to another cell, with a button held or none.
    Moved(Option<Button>),
    /// A notch of the wheel, up or down.
    Wheel {
        up: bool,
    },
}

/// Whether a program reading `mode` is told of `report`.
pub(crate) fn wanted(
    mode: MouseProtocolMode,
    report: Report,
) -> bool {
    !matches!(
        (mode, report),
        (MouseProtocolMode::None, _)
            | (
                MouseProtocolMode::Press,
                Report::Release(_) | Report::Moved(_),
            )
            | (MouseProtocolMode::PressRelease, Report::Moved(_))
            | (MouseProtocolMode::ButtonMotion, Report::Moved(None))
    )
}

/// The text for `report` at `cell`, its row and column counted from
/// 0, with `mods` held. None for a cell the encoding cannot carry.
pub(crate) fn encode(
    report: Report,
    cell: (u16, u16),
    mods: Mods,
    encoding: MouseProtocolEncoding,
) -> Option<String> {
    let mut code = match report {
        Report::Press(button) | Report::Release(button) => {
            button.code()
        }
        Report::Moved(button) => 32 + button.map_or(3, Button::code),
        Report::Wheel { up } => 64 + u16::from(!up),
    };
    code += 4 * u16::from(mods.shift)
        + 8 * u16::from(mods.alt)
        + 16 * u16::from(mods.ctrl);
    let release = matches!(report, Report::Release(_));
    let (row, col) = (u32::from(cell.0) + 1, u32::from(cell.1) + 1);
    match encoding {
        MouseProtocolEncoding::Sgr => {
            let end = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{col};{row}{end}"))
        }
        // The older forms cannot tell which button was released. A
        // terminal's input is text, so the first takes a cell only as
        // far as its byte is ASCII: the first 95 columns and rows.
        MouseProtocolEncoding::Default => {
            let code = if release { 3 + (code & !3) } else { code };
            let byte = |n: u32| {
                u8::try_from(n + 32).ok().filter(u8::is_ascii)
            };
            let bytes =
                [byte(u32::from(code))?, byte(col)?, byte(row)?];
            Some(format!(
                "\x1b[M{}",
                bytes.map(char::from).iter().collect::<String>()
            ))
        }
        MouseProtocolEncoding::Utf8 => {
            let code = if release { 3 + (code & !3) } else { code };
            let mut text = "\x1b[M".to_owned();
            for n in [u32::from(code), col, row] {
                text.push(
                    char::from_u32(n + 32)
                        .filter(|_| n + 32 < 2048)?,
                );
            }
            Some(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sgr(report: Report, cell: (u16, u16), mods: Mods) -> String {
        encode(report, cell, mods, MouseProtocolEncoding::Sgr)
            .unwrap()
    }

    #[test]
    fn sgr_reports_the_button_the_cell_from_one_and_the_release() {
        let none = Mods::default();
        assert_eq!(
            sgr(Report::Press(Button::Left), (0, 0), none),
            "\x1b[<0;1;1M"
        );
        assert_eq!(
            sgr(Report::Release(Button::Right), (4, 9), none),
            "\x1b[<2;10;5m"
        );
        assert_eq!(
            sgr(Report::Moved(Some(Button::Left)), (1, 2), none),
            "\x1b[<32;3;2M"
        );
        assert_eq!(
            sgr(Report::Moved(None), (1, 2), none),
            "\x1b[<35;3;2M"
        );
        assert_eq!(
            sgr(Report::Wheel { up: false }, (0, 0), none),
            "\x1b[<65;1;1M"
        );
        let ctrl_shift = Mods {
            ctrl: true,
            shift: true,
            ..Mods::default()
        };
        assert_eq!(
            sgr(Report::Press(Button::Middle), (0, 0), ctrl_shift),
            "\x1b[<21;1;1M"
        );
    }

    #[test]
    fn the_older_forms_offset_by_32_and_refuse_what_they_cannot_carry()
     {
        let none = Mods::default();
        let default = |report, cell| {
            encode(report, cell, none, MouseProtocolEncoding::Default)
        };
        assert_eq!(
            default(Report::Press(Button::Left), (2, 4)),
            Some("\x1b[M %#".to_owned())
        );
        // A release is button 3, whichever was held.
        assert_eq!(
            default(Report::Release(Button::Right), (0, 0)),
            Some("\x1b[M#!!".to_owned())
        );
        assert_eq!(
            default(Report::Press(Button::Left), (0, 95)),
            None
        );
        let utf8 = encode(
            Report::Press(Button::Left),
            (0, 300),
            none,
            MouseProtocolEncoding::Utf8,
        )
        .unwrap();
        assert_eq!(utf8, "\x1b[M \u{14d}!");
    }

    #[test]
    fn each_mode_reports_only_what_it_asks_for() {
        use MouseProtocolMode as Mode;
        let held = Report::Moved(Some(Button::Left));
        let hover = Report::Moved(None);
        let up = Report::Release(Button::Left);
        assert!(!wanted(Mode::None, Report::Press(Button::Left)));
        assert!(!wanted(Mode::Press, up));
        assert!(wanted(Mode::PressRelease, up));
        assert!(!wanted(Mode::PressRelease, held));
        assert!(wanted(Mode::ButtonMotion, held));
        assert!(!wanted(Mode::ButtonMotion, hover));
        assert!(wanted(Mode::AnyMotion, hover));
    }
}
