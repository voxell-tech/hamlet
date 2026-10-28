//! A version's files against the version before it: a row each that
//! opens on the lines taken out and put in, found by a longest common
//! subsequence of their lines.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_protocol::client::{
    ChangeLines as Line, ChangeLinesMark as Mark,
};
use hamlet_ui::styles::{Fills, ICON, bare, link_row, note};
use hamlet_ui::theme::Palette;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, Cx};

use super::Desk;
use super::look::{aside, boxed, inked, lines};
use crate::bazaar::market::drafts::{Drafts, Files};
use crate::bazaar::market::versions::{Change, File, size_words};
use crate::bazaar::view::parts::problem;
use crate::chat::change::block;
use crate::roll::{either, moved};

/// The most cells of the table the common lines are found with; past
/// it, a file's old lines are all taken out and its new ones put in.
const MOST_CELLS: usize = 4_000_000;

/// The most lines of a file shown.
const MOST_LINES: usize = 400;

/// The lines taken out of `before` and put in to make `after`, with
/// those they share; in each run of changes, those taken out first.
pub(crate) fn diff(before: &str, after: &str) -> Vec<Line> {
    let old = before.lines().collect::<Vec<_>>();
    let new = after.lines().collect::<Vec<_>>();
    let head =
        old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b) =
        (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    let mut out = old[..head]
        .iter()
        .map(|text| line(Mark::Space, text))
        .collect::<Vec<_>>();
    let (mut taken, mut put) = (Vec::new(), Vec::new());
    if a.len() * b.len() > MOST_CELLS {
        taken.extend(a.iter().map(|text| line(Mark::Minus, text)));
        put.extend(b.iter().map(|text| line(Mark::Plus, text)));
    } else {
        // The length of the longest common run of `a[i..]` and
        // `b[j..]`.
        let width = b.len() + 1;
        let mut table = vec![0u32; (a.len() + 1) * width];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                table[i * width + j] = match a[i] == b[j] {
                    true => table[(i + 1) * width + j + 1] + 1,
                    false => table[(i + 1) * width + j]
                        .max(table[i * width + j + 1]),
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                out.append(&mut taken);
                out.append(&mut put);
                out.push(line(Mark::Space, a[i]));
                (i, j) = (i + 1, j + 1);
            } else if j < b.len()
                && (i == a.len()
                    || table[i * width + j + 1]
                        > table[(i + 1) * width + j])
            {
                put.push(line(Mark::Plus, b[j]));
                j += 1;
            } else {
                taken.push(line(Mark::Minus, a[i]));
                i += 1;
            }
        }
    }
    out.append(&mut taken);
    out.append(&mut put);
    out.extend(
        old[old.len() - tail..]
            .iter()
            .map(|text| line(Mark::Space, text)),
    );
    out
}

fn line(mark: Mark, text: &str) -> Line {
    Line {
        mark,
        text: text.to_string(),
    }
}

/// The lines `file` is shown with, opened.
fn lines_of(file: &File) -> Vec<Line> {
    let (old, new) = (
        file.previous_text.as_deref().unwrap_or_default(),
        file.text.as_deref().unwrap_or_default(),
    );
    match file.change {
        Change::Same => {
            new.lines().map(|text| line(Mark::Space, text)).collect()
        }
        Change::Added => diff("", new),
        Change::Removed => diff(old, ""),
        Change::Changed => diff(old, new),
    }
}

/// Whether `file` starts open: those changed or added do.
fn opens(file: &File) -> bool {
    matches!(file.change, Change::Changed | Change::Added)
}

/// The colour of the words of `change`.
fn ink(change: Change) -> fn(&Palette) -> bevy::color::Color {
    match change {
        Change::Changed => |p| p.glow,
        Change::Added => |p| p.green,
        Change::Removed => |p| p.red,
        Change::Same => |p| p.gold_4,
    }
}

/// The box of the files of `version` of the listing `id`, against
/// the version `against`.
pub(super) fn files(
    id: String,
    version: String,
    against: Option<String>,
) -> AnyView {
    let key = (id.clone(), version.clone());
    let now = derived(move |world: &World| {
        let files =
            world.resource::<Drafts>().files.get(&key).cloned();
        let flipped = world
            .resource::<Desk>()
            .flipped
            .iter()
            .filter(|(at, of, _)| *at == key.0 && *of == key.1)
            .map(|(_, _, path)| path.clone())
            .collect::<Vec<_>>();
        (files, flipped)
    })
    .when(either(moved::<Drafts>(), moved::<Desk>()));
    keyed(now, move |(files, flipped)| {
        let (head, body) = match files {
            None | Some(Files::Fetching) => (
                String::new(),
                padded(note("Reading its files...").boxed()),
            ),
            Some(Files::Failed(why)) => {
                (String::new(), padded(problem(why.clone())))
            }
            Some(Files::Got(files)) => {
                let rows = files
                    .iter()
                    .map(|file| {
                        let open = opens(file)
                            != flipped.contains(&file.path);
                        file_row(&id, &version, file, open)
                    })
                    .collect::<Vec<_>>();
                (counts(files, against.as_deref()), lines(rows))
            }
        };
        boxed("FILES".into(), aside(head), body)
    })
    .within(bare())
    .boxed()
}

fn padded(view: AnyView) -> AnyView {
    column((view,))
        .padding(UiRect::axes(px(12.0), px(10.0)))
        .boxed()
}

/// The head of the files: what they are against, and how many of
/// each change.
fn counts(files: &[File], against: Option<&str>) -> String {
    let Some(against) = against else {
        return "first version".into();
    };
    let count = |change| {
        files.iter().filter(|file| file.change == change).count()
    };
    let mut words = format!(
        "against v{against} \u{b7} {} changed \u{b7} {} added",
        count(Change::Changed),
        count(Change::Added)
    );
    let removed = count(Change::Removed);
    if removed > 0 {
        words.push_str(&format!(" \u{b7} {removed} removed"));
    }
    words
}

/// A file's row, which opens or shuts it, and its lines when open.
fn file_row(
    id: &str,
    version: &str,
    file: &File,
    open: bool,
) -> AnyView {
    let flip =
        (id.to_string(), version.to_string(), file.path.clone());
    let caret = match open {
        true => Glyph::Chevron,
        false => Glyph::Into,
    };
    let tone = match file.change {
        Change::Same => Tone::Dim,
        _ => Tone::Body,
    };
    let head = button(
        row((
            glyph(caret, ICON).tone(Tone::Dim),
            column((label(file.path.clone())
                .size(12.5)
                .tone(tone)
                .tagged(MonoText),))
            .fills_width(),
            label(match file.change {
                Change::Removed => String::new(),
                _ => size_words(file.size),
            })
            .size(11.0)
            .tone(Tone::Faint)
            .wrap(false),
            inked(file.change.words(), ink(file.change), 11.0, false),
        ))
        .gap(10.0)
        .grow(1.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(12.0), px(8.0)))
    .rules(link_row)
    .tagged(Variant::Default)
    .on_activate(move |world: &mut World| {
        let mut desk = world.resource_mut::<Desk>();
        match desk.flipped.iter().position(|one| *one == flip) {
            Some(at) => {
                desk.flipped.remove(at);
            }
            None => desk.flipped.push(flip.clone()),
        }
    })
    .boxed();
    if !open {
        return head;
    }
    column((head, plate(lines_of(file)))).gap(0.0).boxed()
}

/// `lines` on their plate, the first [`MOST_LINES`] of them.
fn plate(all: Vec<Line>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let shown = &all[..all.len().min(MOST_LINES)];
        let mut parts = vec![match shown.is_empty() {
            true => note("Not a text file, or empty.").boxed(),
            false => block(shown),
        }];
        if all.len() > MOST_LINES {
            parts.push(
                note(format!(
                    "... {} more lines",
                    all.len() - MOST_LINES
                ))
                .boxed(),
            );
        }
        cx.build(
            column(parts)
                .gap(4.0)
                .padding(UiRect::axes(px(10.0), px(8.0)))
                .margin(UiRect::new(
                    px(12.0),
                    px(12.0),
                    px(0.0),
                    px(10.0),
                ))
                .min_width(px(0.0))
                .fill(theme.palette.code)
                .border(1.0)
                .border_color(theme.palette.gold_6)
                .radius(theme.space.round),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                let mark = match line.mark {
                    Mark::Space => ' ',
                    Mark::Minus => '-',
                    Mark::Plus => '+',
                };
                format!("{mark}{}", line.text)
            })
            .collect()
    }

    #[test]
    fn a_changed_line_is_taken_out_before_its_new_one_is_put_in() {
        let before = "name = \"Keeper\"\nmodel = \"fast\"\ntools = \
                      [\"bell\"]\nend";
        let after = "name = \"Keeper\"\nmodel = \"fast\"\ntools = \
                     [\"bell\", \"lamp\"]\nmore\nend";
        assert_eq!(
            shown(&diff(before, after)),
            [
                " name = \"Keeper\"",
                " model = \"fast\"",
                "-tools = [\"bell\"]",
                "+tools = [\"bell\", \"lamp\"]",
                "+more",
                " end",
            ]
        );
    }

    #[test]
    fn shared_lines_between_changes_are_kept() {
        let diffed = diff("a\nb\nc\nd\ne", "a\nx\nc\ne\nf");
        assert_eq!(
            shown(&diffed),
            [" a", "-b", "+x", " c", "-d", " e", "+f"]
        );
        assert_eq!(shown(&diff("", "a\nb")), ["+a", "+b"]);
        assert_eq!(shown(&diff("a", "")), ["-a"]);
    }
}
