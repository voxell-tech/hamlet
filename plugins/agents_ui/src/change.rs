//! The change an edit asks for: its file, its count and how the call
//! ended, over its lines taken out and put in, the first of them
//! shown and the rest a press away.

use hamlet_engine_bevy::api::agents::{Change, Mark};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Size, Space, Tone,
    UiExt, ViewExt, column, keyed, label, pressable, resource, row,
};

/// The lines of a change shown before "more lines".
const SHOWN: usize = 16;

/// The changes shut and those shown whole, by their key.
#[derive(Resource, Default)]
pub struct Changes {
    shut: Vec<String>,
    whole: Vec<String>,
}

/// How much of a change is shown.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Extent {
    Shut,
    First,
    Whole,
}

impl Changes {
    fn extent(&self, key: &str) -> Extent {
        if self.shut.iter().any(|shut| shut == key) {
            Extent::Shut
        } else if self.whole.iter().any(|whole| whole == key) {
            Extent::Whole
        } else {
            Extent::First
        }
    }
}

/// Puts `key` in `all` or takes it out.
fn flip(all: &mut Vec<String>, key: &str) {
    match all.iter().position(|one| one == key) {
        Some(at) => {
            all.remove(at);
        }
        None => all.push(key.to_string()),
    }
}

/// How a tool call stands: "Done", "Failed (exit N)" or "Running".
pub(crate) fn state(exit: Option<i32>) -> AnyView {
    let (words, tone) = match exit {
        Some(0) => ("Done".to_string(), Tone::Dim),
        Some(code) => {
            (format!("Failed (exit {code})"), Tone::Critical)
        }
        None => ("Running".to_string(), Tone::Accent),
    };
    label(words).size(Size::Small).tone(tone).boxed()
}

/// The change `change` of an edit that ended with `exit`, folded by
/// `key`, which names it among the changes.
pub fn change_view(
    key: String,
    change: &Change,
    exit: Option<i32>,
) -> AnyView {
    let verb = if change.created { "Create" } else { "Edit" };
    let toggled = key.clone();
    let marked = key.clone();
    let fold =
        pressable((label(resource::<Changes, _>(move |changes| {
            let mark = match changes.extent(&marked) {
                Extent::Shut => "+",
                _ => "-",
            };
            format!("{mark}  {verb}")
        }))
        .size(Size::Small)
        .tone(Tone::Dim),))
        .style(ButtonStyle::Quiet)
        .on_click(move |world| {
            flip(&mut world.resource_mut::<Changes>().shut, &toggled);
        });
    let head = row((
        fold,
        label(change.path.clone()).size(Size::Small),
        state(exit),
    ))
    .space(Space::Gap);
    let refused = match (exit, &change.failure) {
        (Some(0) | None, _) => None,
        (Some(_), Some(why)) => Some(format!("Not applied: {why}")),
        (Some(_), None) => Some("Not applied".to_string()),
    };
    let change = change.clone();
    let looked = key.clone();
    let body = keyed(
        resource::<Changes, _>(move |changes| {
            changes.extent(&looked)
        }),
        move |extent| {
            let mut parts = vec![
                label(change.count())
                    .size(Size::Small)
                    .tone(Tone::Dim)
                    .boxed(),
            ];
            if let Some(refused) = &refused {
                parts.push(
                    label(refused.clone())
                        .size(Size::Small)
                        .tone(Tone::Critical)
                        .boxed(),
                );
            }
            if *extent != Extent::Shut && !change.lines.is_empty() {
                parts.push(lines(&key, &change, *extent));
            }
            column(parts).space(Space::Half).boxed()
        },
    );
    column((head, body)).space(Space::Half).boxed()
}

/// The lines of `change` as far as `extent` goes, with the button
/// that shows the rest or puts it away.
fn lines(key: &str, change: &Change, extent: Extent) -> AnyView {
    let whole = extent == Extent::Whole;
    let shown = if whole { change.lines.len() } else { SHOWN };
    let mut parts = change
        .lines
        .iter()
        .take(shown)
        .map(|line| {
            let (mark, tone) = match line.mark {
                Mark::Same => ("  ", Tone::Dim),
                Mark::Out => ("- ", Tone::Critical),
                Mark::In => ("+ ", Tone::Accent),
            };
            label(format!("{mark}{}", line.text))
                .size(Size::Small)
                .tone(tone)
                .boxed()
        })
        .collect::<Vec<_>>();
    let total = change.total as usize;
    if change.lines.len() > SHOWN {
        let words = if whole {
            "Show fewer lines".to_string()
        } else {
            format!("... {} more lines", total.saturating_sub(SHOWN))
        };
        let key = key.to_string();
        parts.push(
            pressable((label(words)
                .size(Size::Small)
                .tone(Tone::Dim),))
            .style(ButtonStyle::Quiet)
            .on_click(move |world: &mut World| {
                flip(
                    &mut world.resource_mut::<Changes>().whole,
                    &key,
                );
            })
            .boxed(),
        );
    }
    // Past what a message carries of a change.
    let lost = total.saturating_sub(change.lines.len());
    if shown >= change.lines.len() && lost > 0 {
        parts.push(
            label(format!("... {lost} more lines"))
                .size(Size::Small)
                .tone(Tone::Dim)
                .boxed(),
        );
    }
    column(parts).space(Space::None).boxed()
}
