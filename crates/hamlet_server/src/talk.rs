//! Conversations: the runtimes they run in, what they are given, what
//! they keep, and what follows a turn.

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
