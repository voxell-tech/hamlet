//! One node of a tree, built as the panels build theirs.

use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row, scroll, text_field,
};
use bevy_fynix::{Each, ScopedExt, ViewExt, each};
use hamlet_sdk::feel::Mood;
use hamlet_sdk::looks::{
    Fills, ICON, brass, hairline, lettered, note, tick, well,
};
use hamlet_sdk::platform::{DisplayText, Wraps};
use hamlet_sdk::theme::HamletTheme;
use hamlet_sdk::{AnyView, Cx, icons};

use super::bind::At;
use crate::tree::{Key, Tag};

/// The space between the children of a column or a row.
const GAP: f32 = 8.0;

/// The view of the node at `at`, which is of this tag.
pub(super) fn view(at: &At, tag: Tag) -> AnyView {
    match tag {
        Tag::Column => rows(at).within(column(()).gap(GAP)).boxed(),
        Tag::Row => rows(at)
            .within(
                row(())
                    .gap(GAP)
                    .align(AlignItems::Center)
                    .tagged(Wraps),
            )
            .boxed(),
        Tag::Scroll => rows(at)
            .within(scroll(()).gap(GAP).fills_height())
            .boxed(),
        Tag::Label => label(at.text()).size(13.0).boxed(),
        Tag::Heading => label(at.text())
            .size(11.0)
            .bold(true)
            .tone(Tone::Faint)
            .tagged(DisplayText)
            .boxed(),
        Tag::Note => note(at.text()).boxed(),
        Tag::Icon(which) => {
            icons::glyph(which, ICON).tone(Tone::Faint).boxed()
        }
        Tag::Button => lettered(at.text())
            .rules(brass)
            .tagged(Mood::Plain)
            .on_activate(|_| {})
            .boxed(),
        Tag::Checkbox => {
            row((tick(at.checked()), label(at.text()).size(13.0)))
                .gap(GAP)
                .align(AlignItems::Center)
                .boxed()
        }
        Tag::TextField => text_field(at.text(), |_, _| {})
            .fills_width()
            .rules(well)
            .tagged(Mood::Field)
            .boxed(),
        Tag::Divider => AnyView::new(|cx: &mut Cx| {
            let line = hairline(cx.theme().palette.brass_6);
            cx.build(line)
        }),
    }
}

/// The children of the node at `at`, each kept while its key is among
/// them.
fn rows(at: &At) -> Each<HamletTheme, Key, Key> {
    let below = at.clone();
    each(at.keys(), Key::clone, move |key| {
        view(&below.under(key), key.tag)
    })
}
