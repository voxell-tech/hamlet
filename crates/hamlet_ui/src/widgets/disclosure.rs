//! The disclosure: a part of a page whose body folds away under its
//! header, which shows a chevron and the part's name.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::Rot2;
use bevy::ui::{AlignItems, UiTransform, percent};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, menu_item, row,
};
use bevy_fynix::{ViewExt, keyed, resource};

use crate::icons::{Glyph, glyph};
use crate::styles::section;
use crate::{AnyView, Cx, View};

/// The side of the header's chevron.
const CHEVRON: f32 = 12.0;

/// A part named `title` whose body, built by `body`, shows while
/// `open` reads true of `R`. Its header spans the page and stays as
/// it is; a press on it runs `toggle`, and the chevron and the body
/// follow what that writes.
pub fn disclosure<R: Resource>(
    title: &'static str,
    open: fn(&R) -> bool,
    toggle: fn(&mut World),
    body: fn() -> AnyView,
) -> impl View {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        let chevron = AnyView::new(move |cx: &mut Cx| {
            let node = cx.build(
                glyph(Glyph::Chevron, CHEVRON).tone(Tone::Faint),
            );
            // It points up as drawn: right while shut, down while
            // open. Turned in place, so the header is never built
            // again.
            cx.effect(
                node,
                resource::<R, _>(open).into(),
                |world, node, &open| {
                    let turn = if open { 180.0 } else { 90.0 };
                    if let Ok(mut entity) = world.get_entity_mut(node)
                    {
                        entity.insert(UiTransform::from_rotation(
                            Rot2::degrees(turn),
                        ));
                    }
                },
            );
            node
        });
        let head = menu_item(
            row((chevron, section(title)))
                .gap(gap)
                .align(AlignItems::Center),
        )
        .on_activate(toggle);
        let shown = keyed(resource::<R, _>(open), move |&open| {
            if open { body() } else { column(()).boxed() }
        })
        .within(column(()).width(percent(100.0)));
        cx.build(column((head, shown)).gap(gap).width(percent(100.0)))
    })
}
