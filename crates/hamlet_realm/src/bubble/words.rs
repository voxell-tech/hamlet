//! The few words a balloon shows of what was said or thought.

use hamlet_client::realm::{Doing, Paper};

/// The end of `text`, cut at a word, as long as fits a bubble.
pub(super) fn tail(text: &str, most: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = text.chars().count();
    if count <= most {
        return text;
    }
    let cut = text
        .char_indices()
        .nth(count - most)
        .map_or(0, |(at, _)| at);
    let rest = &text[cut..];
    let rest = rest.split_once(' ').map_or(rest, |(_, rest)| rest);
    format!("...{rest}")
}

/// The gist of a thought: its first sentence, cut at a word if it
/// runs long. Nothing until there is that much of it.
pub(super) fn gist(text: &str, most: usize) -> Option<String> {
    let plain = text
        .chars()
        .filter(|c| !matches!(c, '*' | '#' | '`' | '_' | '>'))
        .collect::<String>();
    let plain = plain.trim_start();
    let end = plain
        .char_indices()
        .find(|(at, c)| {
            let rest = &plain[at + c.len_utf8()..];
            *c == '\n'
                || (matches!(c, '.' | '?' | '!')
                    && rest.starts_with([' ', '\n']))
        })
        .map(|(at, _)| at);
    let whole = end.is_some();
    let first = plain[..end.unwrap_or(plain.len())].trim();
    let first =
        first.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = first.chars().count();
    if count <= most {
        return (whole && count > 0).then_some(first);
    }
    let cut = first
        .char_indices()
        .nth(most)
        .map_or(first.len(), |(at, _)| at);
    let kept = &first[..cut];
    let kept = kept.rsplit_once(' ').map_or(kept, |(kept, _)| kept);
    Some(format!("{kept}..."))
}

/// The deed of an agent at work, for a thought with no words.
pub(super) fn deed(doing: Doing) -> Option<&'static str> {
    Some(match doing {
        Doing::Unknown | Doing::Think => return None,
        Doing::Read(Paper::File) => "Reading a file",
        Doing::Read(Paper::Notes) => "Reading its notes",
        Doing::Read(Paper::Skill) => "Reading a skill",
        Doing::Write(Paper::File) => "Writing a file",
        Doing::Write(Paper::Notes) => "Writing its notes",
        Doing::Write(Paper::Skill) => "Writing a skill",
        Doing::Run => "Running a command",
        Doing::Fetch => "Reaching the network",
        Doing::CustomTool => "Using a tool",
        Doing::Await => "Waiting for an answer",
    })
}
