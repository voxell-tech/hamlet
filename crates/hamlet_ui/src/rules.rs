//! Where the theme meets the kinds: the one place a kind of box is
//! given its room, corner, fill and edge. [`root`] holds for every
//! panel; a context, such as [`floating`], restyles a kind within the
//! view it is applied to.

use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{FlexDirection, UiRect, px};
use bevy_fynix::ScopedExt;
use bevy_fynix::views::{Frame, frame};

use crate::kinds::{Card, Chip, Inset, Kind, Look, Tile};
use crate::theme::{HamletTheme, StatusKind};
use crate::{Cx, View};

impl Kind for Card {
    const DIRECTION: FlexDirection = FlexDirection::Column;

    /// A card that calls for the chief or reports a fault is edged in
    /// its hue; any other status leaves it as it is.
    fn status(kind: StatusKind, theme: &HamletTheme) -> Frame {
        match kind {
            StatusKind::Attention | StatusKind::Danger => frame()
                .border_color(theme.palette.status.get(kind).solid),
            _ => frame(),
        }
    }
}

impl Kind for Tile {
    const DIRECTION: FlexDirection = FlexDirection::Column;

    fn status(kind: StatusKind, theme: &HamletTheme) -> Frame {
        frame().fill(theme.palette.status.get(kind).fill)
    }
}

impl Kind for Chip {
    const DIRECTION: FlexDirection = FlexDirection::Row;

    /// A chip in the tint of its status, with no edge: a reference is
    /// one in the info tint.
    fn status(kind: StatusKind, theme: &HamletTheme) -> Frame {
        frame()
            .fill(theme.palette.status.get(kind).fill)
            .border_color(Color::NONE)
    }
}

impl Kind for Inset {
    const DIRECTION: FlexDirection = FlexDirection::Column;

    fn status(kind: StatusKind, theme: &HamletTheme) -> Frame {
        frame().fill(theme.palette.status.get(kind).fill)
    }
}

/// The rules every panel is built under.
pub fn root(cx: &mut Cx) {
    cx.set::<Look<Card>>(|look, t| {
        let space = &t.space;
        look.frame(|f| {
            f.padding(card_padding(t))
                .gap(space.gap / 2.0)
                .radius(space.round_card)
                .fill(t.palette.card)
                .border(space.border)
                .border_color(t.palette.gold_6)
        })
    });
    cx.set::<Look<Tile>>(|look, t| {
        look.frame(|f| {
            f.padding(card_padding(t))
                .gap(t.space.gap / 2.0)
                .radius(t.space.round_card)
                .fill(t.palette.card)
        })
    });
    cx.set::<Look<Chip>>(|look, t| {
        let space = &t.space;
        look.frame(|f| {
            f.padding(UiRect::axes(
                px(space.chip_padding_x),
                px(space.chip_padding_y),
            ))
            .gap(space.gap / 2.0)
            .radius(space.round_small)
            .fill(t.palette.code)
            .border(space.border)
            .border_color(t.palette.gold_6)
        })
    });
    cx.set::<Look<Inset>>(|look, t| {
        let space = &t.space;
        look.frame(|f| {
            f.padding(UiRect::axes(px(space.section), px(space.gap)))
                .gap(space.gap / 2.0)
                .min_width(px(0.0))
                .radius(space.round)
                .fill(t.palette.code)
        })
    });
}

/// What floats over the world or the panels: a toast, a card under
/// the pointer, a question in the chat. Its cards are soft and edged
/// a step brighter, so they stand off what is under them.
pub fn floating(cx: &mut Cx) {
    cx.set::<Look<Card>>(|look, t| {
        look.frame(|f| {
            f.radius(t.space.soft).border_color(t.palette.gold_5)
        })
    });
}

/// A row of tiles that has to fit a narrow page: less room at their
/// sides.
pub fn narrow(cx: &mut Cx) {
    cx.set::<Look<Tile>>(|look, t| {
        look.frame(|f| {
            f.padding(UiRect::axes(
                px(t.space.gap),
                px(t.space.card_padding_y),
            ))
        })
    });
}

fn card_padding(t: &HamletTheme) -> UiRect {
    UiRect::axes(
        px(t.space.card_padding_x),
        px(t.space.card_padding_y),
    )
}

/// Mounts `view` under the [`root`] rules.
pub fn mount(world: &mut World, view: impl View) -> Entity {
    bevy_fynix::mount::<HamletTheme>(world, view.rules(root))
}
