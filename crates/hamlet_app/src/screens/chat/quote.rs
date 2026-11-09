//! Lines quoted in a message the way chat apps quote them: a line
//! that opens with `> ` is drawn dim beside a bar, and is no more
//! than words.

use bevy::color::Color;
use bevy::ui::{AlignItems, px};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    Atom, FrameProps, column, frame, label, row,
};
use hamlet_ui::AnyView;
use hamlet_ui::markdown::{ATOM, typed, typed_with};
use hamlet_ui::styles::Fills;

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
/// after its last words. Each of `atoms` is in the place of the
/// [`ATOM`] that is its turn in `text`, in a quote or out of one.
pub(super) fn drawn(
    text: &str,
    reserve: &str,
    bar: Color,
    atoms: Vec<Atom>,
) -> AnyView {
    let parts = parts(text);
    let last = parts.len().saturating_sub(1);
    let beside_bar = |words: AnyView| {
        row((
            frame().width(px(3.0)).shrink(0.0).fill(bar),
            column((words,)).fills_width(),
        ))
        .gap(8.0)
        .align(AlignItems::Stretch)
        .boxed()
    };
    let mut atoms = atoms.into_iter();
    let views = parts
        .into_iter()
        .enumerate()
        .map(|(at, (quoted, words))| {
            let words = match at == last {
                true => format!("{words}{reserve}"),
                false => words,
            };
            let held = words.matches(ATOM).count();
            let atoms = atoms.by_ref().take(held).collect::<Vec<_>>();
            match (quoted, atoms.is_empty()) {
                (true, true) => {
                    beside_bar(label(words).tone(Tone::Dim).boxed())
                }
                (true, false) => beside_bar(typed_with(
                    &words,
                    "",
                    Tone::Dim,
                    atoms,
                )),
                (false, true) => typed(&words, "")
                    .unwrap_or_else(|| label(words).boxed()),
                (false, false) => {
                    typed_with(&words, "", Tone::Body, atoms)
                }
            }
        })
        .collect::<Vec<_>>();
    column(views).gap(4.0).boxed()
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::text::{InlineBox, TextSpan};
    use bevy::time::TimePlugin;
    use bevy_fynix::{FynixPlugin, mount};
    use hamlet_ui::markdown::Anchor;
    use hamlet_ui::styles::bare;
    use hamlet_ui::theme::{self, HamletTheme};

    use super::*;

    #[test]
    fn a_line_with_chips_keeps_its_links_and_its_quotes() {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<HamletTheme>::default(),
        ));
        theme::show(app.world_mut(), HamletTheme::designed());
        app.update();
        let chip = |key: &str| {
            Atom::new::<HamletTheme>(key, key, || bare().boxed())
        };
        let words =
            format!("See http://a.b/c{ATOM} now\n> quoted{ATOM}");
        mount::<HamletTheme>(
            app.world_mut(),
            drawn(
                &words,
                "",
                Color::WHITE,
                vec![chip("a"), chip("b")],
            ),
        );
        app.update();

        let world = app.world_mut();
        let links = world
            .query::<(&TextSpan, &Anchor)>()
            .iter(world)
            .map(|(span, anchor)| (span.0.clone(), anchor.clone()))
            .collect::<Vec<_>>();
        let address = "http://a.b/c".to_string();
        assert_eq!(links, [(address.clone(), Anchor::Web(address))]);
        // One chip in the words, and one in the quote.
        assert_eq!(
            world.query::<&InlineBox>().iter(world).count(),
            2
        );
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
