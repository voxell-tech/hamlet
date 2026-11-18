//! The signs a word of a chief's line starts with to name what it
//! means: `@` an agent or a profile, `/` a command, `$` a skill and
//! `#` a file or a folder.

/// A sign a word starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sign {
    /// `@`, an agent or a profile, by its address.
    Mention,
    /// `/`, a command, only at the start of a line.
    Command,
    /// `$`, a skill.
    Skill,
    /// `#`, a file or a folder.
    File,
}

impl Sign {
    /// Every sign.
    pub const ALL: [Self; 4] =
        [Self::Mention, Self::Command, Self::Skill, Self::File];

    /// The character the sign is.
    pub const fn char(self) -> char {
        match self {
            Self::Mention => '@',
            Self::Command => '/',
            Self::Skill => '$',
            Self::File => '#',
        }
    }

    /// The sign `char` is.
    pub fn of(char: char) -> Option<Self> {
        Self::ALL.into_iter().find(|sign| sign.char() == char)
    }

    /// Whether `rest`, the word after the sign, means something else:
    /// `$5` is money, `#3` a wait and `/usr/bin` a path.
    pub fn refuses(self, rest: &str) -> bool {
        let digit = rest
            .trim_start_matches('.')
            .starts_with(|c: char| c.is_ascii_digit());
        match self {
            Self::Mention => false,
            Self::Command => rest.contains(['/', '\\']),
            Self::Skill => digit,
            Self::File => {
                rest.starts_with(|c: char| c.is_ascii_digit())
            }
        }
    }
}

/// A word that starts with a sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token<'a> {
    pub sign: Sign,
    /// The word after the sign.
    pub rest: &'a str,
    /// The byte of the line the sign is at.
    pub start: usize,
}

/// The token of the word `line` holds before `caret`, a byte of it:
/// the word being typed.
pub fn at_caret(line: &str, caret: usize) -> Option<Token<'_>> {
    let before = line.get(..caret)?;
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(at, c)| at + c.len_utf8());
    let word = &before[start..];
    let sign = Sign::of(word.chars().next()?)?;
    let rest = &word[sign.char().len_utf8()..];
    if sign.refuses(rest)
        || (sign == Sign::Command && !line[..start].trim().is_empty())
    {
        return None;
    }
    Some(Token { sign, rest, start })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_before_the_caret_is_the_token() {
        let line = "ask @mi about it";
        let token = at_caret(line, 7).unwrap();
        assert_eq!(
            (token.sign, token.rest, token.start),
            (Sign::Mention, "mi", 4)
        );
        assert_eq!(at_caret(line, 3), None);
    }

    #[test]
    fn money_waits_paths_and_late_commands_are_no_tokens() {
        for (line, caret) in [
            ("costs $5", 8),
            ("allow #3", 8),
            ("/usr/bin", 8),
            ("then /new", 9),
        ] {
            assert_eq!(at_caret(line, caret), None, "{line}");
        }
        assert_eq!(
            at_caret("  /ne", 5).map(|t| t.sign),
            Some(Sign::Command)
        );
    }
}
