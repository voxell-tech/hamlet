//! The popup of how far an agent goes before it asks: its
//! permissions, as T3 Code words them.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::ui::{AlignItems, UiRect, px};
use bevy::ui_widgets::popover::{
    PopoverAlign, PopoverPlacement, PopoverSide,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, popup, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::Access;
use hamlet_core::words;
use hamlet_ui::styles::{ICON, description, link_row, small_rules};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use super::{close, model};

/// The width of the popup.
const WIDE: f32 = 300.0;

/// The icon of a level: a padlock, a pencil, a shield with a check,
/// and a shield struck through.
pub(crate) fn glyph_of(access: Access) -> Glyph {
    match access {
        Access::Supervised => Glyph::Lock,
        Access::AcceptEdits => Glyph::Rename,
        Access::Auto => Glyph::ShieldCheck,
        Access::Full => Glyph::ShieldOff,
    }
}

/// The name of a level, as T3 Code gives it.
pub(crate) fn label_of(access: Access) -> &'static str {
    match access {
        Access::Supervised => words::SUPERVISED,
        Access::AcceptEdits => words::ACCEPT_EDITS,
        Access::Auto => words::AUTO_ACCESS,
        Access::Full => words::FULL_ACCESS,
    }
}

/// What a level means.
pub(crate) fn words_of(access: Access) -> &'static str {
    match access {
        Access::Supervised => words::SUPERVISED_WORDS,
        Access::AcceptEdits => words::ACCEPT_EDITS_WORDS,
        Access::Auto => words::AUTO_ACCESS_WORDS,
        Access::Full => words::FULL_ACCESS_WORDS,
    }
}

/// The permissions `agent` has now.
pub(crate) fn of(world: &World, agent: Entity) -> Access {
    world
        .get::<Access>(agent)
        .copied()
        .unwrap_or(Access::Supervised)
}

/// Gives `agent` the permissions `to`.
pub(crate) fn set(world: &mut World, agent: Entity, to: Access) {
    let address = model::address(world, agent);
    crate::chat::command(
        world,
        format!("/permissions {address} {}", to.word()),
    );
}

fn above() -> Vec<PopoverPlacement> {
    use PopoverAlign::{End, Start};
    use PopoverSide::{Bottom, Top};
    [(Top, Start), (Top, End), (Bottom, Start), (Bottom, End)]
        .into_iter()
        .map(|(side, align)| PopoverPlacement {
            side,
            align,
            gap: 6.0,
        })
        .collect()
}

/// A level, pressed to give it to the agent.
fn level(agent: Entity, access: Access, now: Access) -> AnyView {
    let chosen = access == now;
    let tone = if access == Access::Full {
        Tone::Warning
    } else {
        Tone::Body
    };
    button(
        row((
            glyph(glyph_of(access), ICON).tone(tone),
            column((
                label(label_of(access)).tone(tone).wrap(false),
                description(words_of(access)),
            ))
            .gap(2.0)
            .grow(1.0),
            if chosen {
                glyph(Glyph::Check, ICON).boxed()
            } else {
                frame().width(px(ICON)).boxed()
            },
        ))
        .gap(10.0)
        .grow(1.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(8.0), px(6.0)))
    .rules(link_row)
    .tagged(Variant::Text)
    .on_activate(move |world| {
        if access != of(world, agent) {
            set(world, agent, access);
        }
        close(world);
    })
    .boxed()
}

/// The popup of `agent`'s permissions, against `anchor`.
pub(super) fn permissions(agent: Entity, anchor: Rect) -> impl View {
    with_cx(move |cx| {
        let (gap, padding) = {
            let theme = cx.theme();
            (theme.space.gap, theme.space.overlay_padding)
        };
        let levels = keyed(
            derived(move |world: &World| of(world, agent))
                .when(every_frame()),
            move |&now| {
                column(
                    Access::ALL
                        .into_iter()
                        .map(|access| level(agent, access, now))
                        .collect::<Vec<_>>(),
                )
                .gap(2.0)
                .boxed()
            },
        )
        .within(column(()));
        let body = column((
            label("Permissions")
                .tone(Tone::Faint)
                .bold(true)
                .wrap(false)
                .rules(small_rules),
            levels,
            description(words::PERMISSIONS),
        ))
        .gap(gap)
        .align(AlignItems::Stretch)
        .width(px(WIDE));
        popup(anchor, (body,))
            .placements(above())
            .on_dismiss(|world, _| close(world))
            .padding(UiRect::all(px(padding)))
    })
}
