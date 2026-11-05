//! Conversations: the runtimes they run in, what they are given, what
//! they keep, and what follows a turn.

pub(crate) mod activity;
pub(crate) mod asking;
pub(crate) mod chats;
pub(crate) mod conversations;
pub(crate) mod follow;
pub(crate) mod inputs;
pub(crate) mod items;
pub(crate) mod marks;
pub(crate) mod models;
pub(crate) mod process;
pub(crate) mod prompt;
pub(crate) mod runtimes;
pub(crate) mod sync;
pub(crate) mod tasks;
pub(crate) mod turns;

use crate::settings::{Overrides, Settings};

/// What conversations read besides the database: the settings as this
/// process runs them.
#[derive(Clone, Debug)]
pub(crate) struct Env {
    pub settings: Settings,
    pub overrides: Overrides,
}

/// Closes every open wait `agent` raised, as `how` (H34).
pub(crate) fn waits_lapse(
    cx: &mut crate::change::Change<'_>,
    agent: hamlet_store::ids::AgentId,
    how: &str,
) -> anyhow::Result<()> {
    use diesel::prelude::*;
    use hamlet_store::schema::wait;
    diesel::update(
        wait::table
            .filter(wait::agent.eq(agent))
            .filter(wait::open.eq(true)),
    )
    .set((
        wait::open.eq(false),
        wait::how.eq(how),
        wait::settled.eq(cx.at),
    ))
    .execute(cx.db)?;
    Ok(())
}
