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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thought_shows_what_it_is_about() {
        // Not until its first sentence is whole.
        assert_eq!(gist("The chief wants the", 56), None);
        assert_eq!(
            gist("**The chief** wants pears. Then I will", 56),
            Some("The chief wants pears".into())
        );
        let long = "I should look through every row of the orchard \
                    before the frost comes and say what each needs";
        let cut = gist(long, 30).unwrap();
        assert_eq!(cut, "I should look through every...");
        assert_eq!(deed(Doing::Run), Some("Running a command"));
        assert_eq!(deed(Doing::Think), None);
    }

    #[test]
    fn a_long_speech_keeps_its_end() {
        assert_eq!(tail("  a  short   one ", 40), "a short one");
        let long = "one two three four five six seven eight nine ten";
        let cut = tail(long, 20);
        assert_eq!(cut, "...eight nine ten");
        assert!(cut.chars().count() <= 23);
    }
}
