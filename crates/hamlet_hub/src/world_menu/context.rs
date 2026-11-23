//! The popup of how much of its model's context an agent's chat
//! holds, with the way to compact it.

use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy::ui_widgets::popover::{
    PopoverAlign, PopoverPlacement, PopoverSide,
};
use hamlet_core::client::chats::Status;
use hamlet_core::fynix::tokens::Tone;
use hamlet_core::fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, popup, row,
};
use hamlet_core::fynix::{ViewExt, derived, every_frame, keyed};
use hamlet_core::ui::styles::{Looks, description, lettered};
use hamlet_core::ui::theme::HamletTheme;
use hamlet_core::ui::widgets::effects::Variant;
use hamlet_core::ui::{View, with_cx};

use super::close;
use crate::convo::Convo;

/// The width of the popup.
const WIDE: f32 = 260.0;

/// The share of the context from which the ring warns, and from which
/// it is critical.
const NEAR: f32 = 0.7;
const FULL: f32 = 0.9;

/// The tokens a chat's context holds, the most its model holds, and
/// whether it is being compacted.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub(crate) struct ContextUse {
    pub held: Option<u64>,
    pub most: Option<u64>,
    pub compacting: bool,
}

impl ContextUse {
    /// The context of the chat `hub` shows.
    pub(crate) fn of(world: &World, hub: Entity) -> Self {
        let Some(convo) = world.get::<Convo>(hub) else {
            return Self::default();
        };
        let status = world.get::<Status>(convo.chat);
        Self {
            held: status.and_then(|status| status.context),
            most: super::model::card_of(world, convo.agent)
                .and_then(|card| card.context)
                .and_then(|most| u64::try_from(most).ok())
                .filter(|most| *most > 0),
            compacting: status
                .is_some_and(|status| status.compacting),
        }
    }

    /// The share of the context held, from 0 to 1, while both are
    /// known.
    pub(crate) fn share(&self) -> Option<f32> {
        let (held, most) = self.held.zip(self.most)?;
        Some((held as f32 / most as f32).clamp(0.0, 1.0))
    }

    /// The tone of a share of the context held.
    pub(crate) fn tone(&self) -> Tone {
        match self.share() {
            Some(share) if share >= FULL => Tone::Critical,
            Some(share) if share >= NEAR => Tone::Warning,
            _ => Tone::Dim,
        }
    }

    /// "62% of the context", or "Context" while the share is unknown.
    pub(crate) fn heading(&self) -> String {
        match self.share() {
            Some(share) => {
                format!("{}% of the context", (share * 100.0).round())
            }
            None => "Context".into(),
        }
    }

    /// "124k of 200k tokens", or what of it is known.
    pub(crate) fn tokens(&self) -> String {
        match (self.held, self.most) {
            (Some(held), Some(most)) => {
                format!("{} of {} tokens", count(held), count(most))
            }
            (Some(held), None) => format!("{} tokens", count(held)),
            (None, Some(most)) => {
                format!("Up to {} tokens", count(most))
            }
            (None, None) => "Measured after the next answer".into(),
        }
    }
}

/// The colour of a share held in `tone`, in the ring and the bar.
pub(crate) fn fill(tone: Tone, theme: &HamletTheme) -> Color {
    let status = &theme.palette.status;
    match tone {
        Tone::Critical => status.danger.mark,
        Tone::Warning => status.warning.mark,
        _ => theme.palette.accent_mark,
    }
}

/// The colour of the context not held, under the share.
pub(crate) fn track(theme: &HamletTheme) -> Color {
    theme.palette.border
}

/// `tokens` in a few figures: 850, 12k or 1.2M.
fn count(tokens: u64) -> String {
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => format!("{}k", (tokens + 500) / 1_000),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

fn above() -> Vec<PopoverPlacement> {
    use PopoverAlign::{End, Start};
    use PopoverSide::{Bottom, Top};
    [(Top, End), (Top, Start), (Bottom, End), (Bottom, Start)]
        .into_iter()
        .map(|(side, align)| PopoverPlacement {
            side,
            align,
            gap: 6.0,
        })
        .collect()
}

/// The bar of the share held, in the ring's tone.
fn bar(hub: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let held = ContextUse::of(world, hub);
            (
                held.share().map(|share| (share * 200.0).round()),
                held.tone(),
            )
        })
        .when(every_frame()),
        |&(share, tone)| {
            with_cx(move |cx| {
                let theme = cx.theme();
                let filled = frame()
                    .height(percent(100.0))
                    .width(percent(share.unwrap_or(0.0) / 2.0))
                    .radius(2.0)
                    .fill(fill(tone, theme));
                row((filled,))
                    .height(px(4.0))
                    .width(percent(100.0))
                    .radius(2.0)
                    .fill(track(theme))
            })
            .boxed()
        },
    )
    .within(row(()).width(percent(100.0)))
}

/// The popup of the context of the chat `hub` shows, against
/// `anchor`.
pub(super) fn context(hub: Entity, anchor: Rect) -> impl View {
    with_cx(move |cx| {
        let (gap, padding, heading) = {
            let theme = cx.theme();
            (
                theme.space.gap,
                theme.space.overlay_padding,
                theme.text.heading,
            )
        };
        let read = move |words: fn(&ContextUse) -> String| {
            derived(move |world: &World| {
                words(&ContextUse::of(world, hub))
            })
            .when(every_frame())
        };
        let compact = keyed(
            derived(move |world: &World| {
                ContextUse::of(world, hub).compacting
            })
            .when(every_frame()),
            move |&compacting| match compacting {
                true => lettered("Compacting")
                    .variant(Variant::Primary)
                    .tagged(bevy::ui::InteractionDisabled)
                    .boxed(),
                false => lettered("Compact Now")
                    .variant(Variant::Primary)
                    .on_activate(move |world| {
                        crate::hub::thread::compact_in(world, hub);
                    })
                    .boxed(),
            },
        )
        .within(row(()));
        let body = column((
            label(read(ContextUse::heading)).size(heading).bold(true),
            label(read(ContextUse::tokens)).tone(Tone::Dim),
            bar(hub),
            description(
                "What the model reads with each message. Compacting \
                 summarizes the earlier messages to make room.",
            ),
            compact,
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
