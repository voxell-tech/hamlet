//! The disclosure: a part of a page whose body folds away under its
//! header, which shows a chevron and the part's name.

use bevy::ecs::world::World;
use bevy::math::Rot2;
use bevy::ui::{AlignItems, UiTransform, percent};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, menu_item, row,
};
use bevy_fynix::{Signal, ViewExt, keyed};

use crate::icons::{Glyph, glyph};
use crate::looks::section;
use crate::{AnyView, Cx, View};

/// The side of the header's chevron.
const CHEVRON: f32 = 12.0;

/// A part named `title` whose body, built by `body`, shows while
/// `open` holds. Its header spans the page; a press on it runs
/// `toggle`, and `open` follows what that writes.
pub fn disclosure(
    title: &'static str,
    open: Signal<bool>,
    toggle: fn(&mut World),
    body: fn() -> AnyView,
) -> impl View {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        let part = keyed(open, move |&open| {
            // The chevron points up as drawn: right while shut, down
            // while open.
            let turn = if open { 180.0 } else { 90.0 };
            let head = menu_item(
                row((
                    glyph(Glyph::Chevron, CHEVRON)
                        .tone(Tone::Faint)
                        .tagged(UiTransform::from_rotation(
                            Rot2::degrees(turn),
                        )),
                    section(title),
                ))
                .gap(gap)
                .align(AlignItems::Center),
            )
            .on_activate(toggle);
            if open {
                column((head, body()))
                    .gap(gap)
                    .width(percent(100.0))
                    .boxed()
            } else {
                head.boxed()
            }
        })
        .within(column(()).width(percent(100.0)));
        cx.build(part)
    })
}
