//! Lines quoted in a message the way chat apps quote them: a line
//! that opens with `> ` is drawn dim beside a bar, and is no more
//! than words.

use bevy::color::Color;
use bevy::ui::{AlignItems, px};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, frame, label, row};

use crate::looks::Fills;
use crate::{AnyView, markdown};

/// The parts of `text` in order: each its words, and whether they are
/// quoted.
pub(super) fn parts(text: &str) -> Vec<(bool, String)> {
    let mut parts = Vec::<(bool, String)>::new();
    for line in text.lines() {
        let quoted = line
            .strip_prefix("> ")
            .or_else(|| (line == ">").then_some(""));
        let (quoted, words) = match quoted {
            Some(words) => (true, words),
            None => (false, line),
        };
        match parts.last_mut() {
            Some((was, kept)) if *was == quoted => {
                kept.push('\n');
                kept.push_str(words);
            }
            _ => parts.push((quoted, words.to_string())),
        }
    }
    // The blank lines about a quote are the room the bar already
    // makes.
    parts
        .into_iter()
        .map(|(quoted, words)| match quoted {
            true => (quoted, words),
            false => (quoted, words.trim_matches('\n').to_string()),
        })
        .filter(|(quoted, words)| *quoted || !words.is_empty())
        .collect()
}

/// True when `text` quotes a line.
pub(super) fn has_quote(text: &str) -> bool {
    parts(text).iter().any(|(quoted, _)| *quoted)
}

/// `text` as quoted lines, each opened with `> `.
pub(super) fn quoting(text: &str) -> String {
    text.trim_end()
        .lines()
        .map(|line| match line.trim_end() {
            "" => ">".to_string(),
            line => format!("> {line}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `text` drawn with its quotes beside a bar of `bar`, and `reserve`
/// after its last words.
pub(super) fn drawn(
    text: &str,
    reserve: &str,
    bar: Color,
) -> AnyView {
    let parts = parts(text);
    let last = parts.len().saturating_sub(1);
    let views = parts
        .into_iter()
        .enumerate()
        .map(|(at, (quoted, words))| {
            let words = match at == last {
                true => format!("{words}{reserve}"),
                false => words,
            };
            match quoted {
                true => row((
                    frame().width(px(3.0)).shrink(0.0).fill(bar),
                    column((label(words).tone(Tone::Dim),))
                        .fills_width(),
                ))
                .gap(8.0)
                .align(AlignItems::Stretch)
                .boxed(),
                false => markdown::typed(&words, "")
                    .unwrap_or_else(|| label(words).boxed()),
            }
        })
        .collect::<Vec<_>>();
    column(views).gap(4.0).boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_lines_are_parted_from_the_rest() {
        assert_eq!(parts("hello"), vec![(false, "hello".into())]);
        assert_eq!(
            parts("> one\n>\n> two\n\nmine\n> three"),
            vec![
                (true, "one\n\ntwo".into()),
                (false, "mine".into()),
                (true, "three".into()),
            ]
        );
        // Only `> ` quotes, as in the chat apps.
        assert!(!has_quote(">no space"));
        assert!(has_quote("a\n> b"));
    }

    #[test]
    fn a_selection_quoted_reads_back_as_itself() {
        let chosen = "Two things:\n\n  the key  \n";
        let sent = format!("{}\nthanks", quoting(chosen));
        assert_eq!(sent, "> Two things:\n>\n>   the key\nthanks");
        assert_eq!(
            parts(&sent),
            vec![
                (true, "Two things:\n\n  the key".into()),
                (false, "thanks".into()),
            ]
        );
    }
}
