//! What an agent is doing, in one glyph, tone and word wherever the
//! panels show it.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy_fynix::tokens::Tone;
use hamlet_client::realm::{Busy, Faulted, Left, name_of};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client::WaitDetail;
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::icons::Glyph;

/// What an agent is doing, the most pressing first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Standing {
    Waits,
    Stopped,
    Works,
    Rests,
    Gone,
    /// Gone into another agent.
    Merged,
    /// A scholar the Hand turned away.
    Turned,
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
            .filter(|one| {
                !matches!(
                    one,
                    Self::Gone | Self::Merged | Self::Turned
                )
            })
            .min_by_key(|one| *one as u8)
            .unwrap_or(Self::Rests)
    }

    pub(crate) fn glyph(self) -> Glyph {
        match self {
            Self::Waits => Glyph::Bell,
            Self::Stopped => Glyph::Stop,
            Self::Works => Glyph::Bolt,
            Self::Rests => Glyph::Moon,
            Self::Gone | Self::Turned => Glyph::Gone,
            Self::Merged => Glyph::Merge,
        }
    }

    /// Its status: orange for what waits on the chief, red for a
    /// fault or a refusal, ochre for work under way, purple for a
    /// merge, and neutral for the rest.
    pub(crate) fn kind(self) -> StatusKind {
        match self {
            Self::Waits => StatusKind::Attention,
            Self::Stopped | Self::Turned => StatusKind::Danger,
            Self::Merged => StatusKind::Merged,
            Self::Works => StatusKind::Warning,
            Self::Rests | Self::Gone => StatusKind::Rest,
        }
    }

    /// The colour of its mark, by the tone of its status.
    pub(crate) fn tone(self) -> Tone {
        self.kind().tone()
    }

    /// The colour of the words that say it.
    pub(crate) fn words(self) -> Tone {
        self.kind().words()
    }

    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Waits => "Needs you",
            Self::Stopped => "Stopped",
            Self::Works => "At work",
            Self::Rests => "At rest",
            Self::Gone => "Gone",
            Self::Merged => "Merged",
            Self::Turned => "Turned away",
        }
    }
}

/// What `count` waits for the chief do, after their number.
pub(crate) fn need_you(count: usize) -> &'static str {
    if count == 1 { "needs you" } else { "need you" }
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
