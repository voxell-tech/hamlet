//! A message that answers another: a first line naming the message it
//! answers, with its words as a JSON string, and the words after it.

use hamlet_engine_bevy::api::agents::Entry;
use serde::{Deserialize, Serialize};

/// The head of a reply's first line.
const HEAD: &str = "(Replying to ";

/// The most characters of what is replied to that a reply carries.
pub const REPLY_CHARS: usize = 600;

/// The message a reply answers: its speaker and its words.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Answers {
    pub by: String,
    pub text: String,
}

impl Answers {
    /// The words `text` of `by`, cut to at most `most` characters.
    pub fn of(by: &str, text: &str, most: usize) -> Self {
        let text = text.trim();
        let text = match text.char_indices().nth(most) {
            Some((cut, _)) => {
                format!("{}...", text[..cut].trim_end())
            }
            None => text.to_string(),
        };
        Answers {
            by: by.to_string(),
            text,
        }
    }
}

/// `words` sent as a reply to `answers`.
pub fn replied(answers: &Answers, words: &str) -> String {
    format!(
        "{HEAD}{}: {})\n\n{words}",
        answers.by,
        quoted(&answers.text)
    )
}

/// What `text` replies to and the words after it. `None` when it is
/// no reply.
pub fn unreplied(text: &str) -> Option<(Answers, &str)> {
    let (head, words) = text.split_once("\n\n")?;
    let (by, said) = head
        .strip_prefix(HEAD)?
        .strip_suffix(')')?
        .split_once(": ")?;
    let said = unquoted(said)?;
    let by = by.trim();
    if by.is_empty() {
        return None;
    }
    let answers = Answers {
        by: by.to_string(),
        text: said,
    };
    Some((answers, words))
}

/// The words of `text` after any reply line it opens with.
pub fn rest(text: &str) -> &str {
    unreplied(text).map_or(text, |(_, words)| words)
}

/// The words of a letter `user` sent back, after the head that says
/// so.
fn letter<'a>(user: &str, text: &'a str) -> &'a str {
    let head = format!("Reply from `{user}`: ");
    text.strip_prefix(head.as_str()).unwrap_or(text)
}

/// The words of a message as its bubble shows them.
pub fn words(entry: &Entry) -> &str {
    match entry {
        Entry::User { user, text } => rest(letter(user, text)),
        Entry::Assistant(text)
        | Entry::Agent { text, .. }
        | Entry::Answered(text) => text,
        _ => "",
    }
}

/// The words of a message a reply can answer, if it has any.
pub fn answers_of(entry: &Entry) -> Option<&str> {
    let words = words(entry);
    (!words.trim().is_empty()).then_some(words)
}

/// `text` as a JSON string.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The text of the JSON string `quoted`.
fn unquoted(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'u' => {
                let code = chars.by_ref().take(4).collect::<String>();
                out.push(char::from_u32(
                    u32::from_str_radix(&code, 16).ok()?,
                )?);
            }
            other => out.push(other),
        }
    }
    Some(out)
}
