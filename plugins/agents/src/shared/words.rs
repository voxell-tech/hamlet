//! Words as the panels set them: names, tags, times, text cut short,
//! lists narrowed and why a request failed.

use std::time::{SystemTime, UNIX_EPOCH};

use hamlet_engine_bevy::Failure;

/// The longest tag, in letters.
const TAG: usize = 24;

/// Words a tag does not end on.
const LOOSE: [&str; 12] = [
    "a", "an", "and", "the", "its", "of", "to", "in", "for", "with",
    "on", "or",
];

/// `name` with its first letter a capital.
pub fn titled(name: &str) -> String {
    let mut letters = name.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// The start of a purpose, cut at a word.
pub fn tag_of(purpose: &str) -> String {
    let stops = ['.', ',', ';', ':'];
    let words = purpose
        .split_whitespace()
        .map(|word| word.trim_end_matches(stops).to_lowercase())
        .collect::<Vec<_>>();
    let mut letters = 0;
    let mut kept = words
        .iter()
        .take_while(|word| {
            letters += word.chars().count() + 1;
            letters <= TAG + 1
        })
        .count();
    if kept == words.len() {
        return words.join(" ");
    }
    while kept > 1 && LOOSE.contains(&words[kept - 1].as_str()) {
        kept -= 1;
    }
    format!("{}...", words[..kept].join(" "))
}

/// `text` cut at a word to at most `most` characters, with "...".
pub fn short(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let cut = text
        .char_indices()
        .nth(most)
        .map_or(text.len(), |(at, _)| at);
    let kept = &text[..cut];
    let kept = kept.rsplit_once(' ').map_or(kept, |(kept, _)| kept);
    format!("{kept}...")
}

/// The start of `text` on one line: at most `most` characters, and
/// "..." where it was cut.
pub fn clip(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(most) {
        Some((cut, _)) => format!("{}...", &flat[..cut]),
        None => flat,
    }
}

/// The end of `text` on one line: at most `most` characters, "..."
/// before them when it is cut.
pub fn tail(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = flat.chars().count();
    if count <= most {
        return flat;
    }
    let kept = flat.chars().skip(count - most).collect::<String>();
    format!("...{kept}")
}

/// The hour and minute of `at`, milliseconds since 1970, on a clock
/// `offset` seconds east of UTC. Empty when the time is not known.
pub fn hour(at: u64, offset: i32) -> String {
    if at == 0 {
        return String::new();
    }
    let seconds = (at / 1000) as i64 + i64::from(offset);
    let minutes = seconds.rem_euclid(86_400) / 60;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The items of `all` whose name has every word of `narrow` in it,
/// whatever the case, the first `most` of them, and how many more
/// there are.
pub fn narrowed<T: Clone>(
    all: &[T],
    narrow: &str,
    most: usize,
    name: impl Fn(&T) -> &str,
) -> (Vec<T>, usize) {
    let words = narrow.to_lowercase();
    let mut found = all
        .iter()
        .filter(|item| {
            let name = name(item).to_lowercase();
            words.split_whitespace().all(|word| name.contains(word))
        })
        .cloned()
        .collect::<Vec<_>>();
    let more = found.len().saturating_sub(most);
    found.truncate(most);
    (found, more)
}

/// The panels' methods on [`Failure`].
pub trait FailureExt {
    /// Why it failed in plain words, as the person is told.
    fn plain(&self) -> String;
}

impl FailureExt for Failure {
    fn plain(&self) -> String {
        match self {
            Failure::Refused(why) | Failure::Failed(why) => {
                why.clone()
            }
            other => other.to_string(),
        }
    }
}

/// The time since `at`, milliseconds since 1970, in words.
pub fn since(at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    match now.saturating_sub(at / 1000) {
        0..=59 => "just now".into(),
        seconds @ 60..=3599 => format!("{} min ago", seconds / 60),
        seconds @ 3600..=86399 => format!("{} h ago", seconds / 3600),
        seconds => format!("{} d ago", seconds / 86400),
    }
}
