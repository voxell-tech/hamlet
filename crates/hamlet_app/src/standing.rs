//! What an agent is doing, in one glyph, tone and word wherever the
//! panels show it.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy_fynix::tokens::Tone;
use hamlet_client::realm::{Busy, Faulted, Left, name_of};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client::WaitDetail;
use hamlet_ui::widgets::icons::Glyph;

/// What an agent is doing, the most pressing first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Standing {
    Waits,
    Stopped,
    Works,
    Rests,
    Gone,
}

impl Standing {
    /// What `agent` is doing.
    pub(crate) fn of(world: &World, agent: Entity) -> Self {
        if world.get::<Left>(agent).is_some() {
            Self::Gone
        } else if waits(world, agent) > 0 {
            Self::Waits
        } else if world.get::<Faulted>(agent).is_some() {
            Self::Stopped
        } else if world.get::<Busy>(agent).is_some() {
            Self::Works
        } else {
            Self::Rests
        }
    }

    /// The most pressing of `all`, or at rest when there are none.
    pub(crate) fn of_all(
        all: impl IntoIterator<Item = Self>,
    ) -> Self {
        all.into_iter()
            .filter(|one| *one != Self::Gone)
            .min_by_key(|one| *one as u8)
            .unwrap_or(Self::Rests)
    }

    pub(crate) fn glyph(self) -> Glyph {
        match self {
            Self::Waits => Glyph::Bell,
            Self::Stopped => Glyph::Stop,
            Self::Works => Glyph::Bolt,
            Self::Rests => Glyph::Moon,
            Self::Gone => Glyph::Gone,
        }
    }

    pub(crate) fn tone(self) -> Tone {
        match self {
            Self::Waits | Self::Stopped => Tone::Critical,
            Self::Works => Tone::Accent,
            Self::Rests | Self::Gone => Tone::Dim,
        }
    }

    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Waits => "Waits for you",
            Self::Stopped => "Stopped",
            Self::Works => "At work",
            Self::Rests => "At rest",
            Self::Gone => "Gone",
        }
    }
}

/// How many waits `agent` has raised. A newcomer's wait is about the
/// scholar, not the agent that made it.
pub(crate) fn waits(world: &World, agent: Entity) -> usize {
    let address = name_of(world, agent);
    world.try_query::<&Waiting>().map_or(0, |mut waiting| {
        waiting
            .iter(world)
            .filter(|wait| {
                wait.by == address
                    && !matches!(
                        wait.detail,
                        WaitDetail::Newcomer { .. }
                    )
            })
            .count()
    })
}
