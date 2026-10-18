//! The words the panels and the hover card use for what an agent is
//! doing and how a hamlet stands.

use bevy_fynix::tokens::Tone;
use hamlet_core::agents::{Activity, Doing, Paper};

use crate::icons::Glyph;

/// The most of an agent's purpose a card shows, in letters.
pub const PURPOSE_SHOWN: usize = 96;

/// Words cut to `most` letters at a word's end.
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

/// The step an agent is on, in words.
pub fn now(busy: bool, activity: Activity) -> String {
    if !busy {
        return "At rest".into();
    }
    let paper = |p: Paper| match p {
        Paper::File => "a file",
        Paper::Notes => "its notes",
        Paper::Skill => "a skill",
    };
    match activity.doing {
        Doing::Unknown => "Working".into(),
        Doing::Think => "Thinking".into(),
        Doing::Read(p) => format!("Reading {}", paper(p)),
        Doing::Write(p) => format!("Writing {}", paper(p)),
        Doing::Run => "Running a command".into(),
        Doing::Fetch => "Fetching from the network".into(),
        Doing::CustomTool => "Using a tool added by hand".into(),
        Doing::Await => "Waiting for an agent's answer".into(),
    }
}

/// The state of a hamlet in a word: the agents `living` in it, and
/// those of them `working` or `waiting` for the chief.
pub fn standing(
    living: usize,
    working: usize,
    waiting: usize,
) -> (Glyph, String, Tone) {
    let count = match living {
        1 => "1 agent".to_string(),
        n => format!("{n} agents"),
    };
    match (waiting, working) {
        (0, 0) => {
            (Glyph::Moon, format!("{count}, all at rest"), Tone::Dim)
        }
        (0, n) => (
            Glyph::Bolt,
            format!("{count}, {n} at work"),
            Tone::Accent,
        ),
        (w, _) => (
            Glyph::Bell,
            format!("{count}, {w} waiting for you"),
            Tone::Critical,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_purpose_is_cut_at_a_word() {
        assert_eq!(short("Keeps the list.", 40), "Keeps the list.");
        assert_eq!(
            short("Reads every paper it is given", 12),
            "Reads every..."
        );
    }
}
