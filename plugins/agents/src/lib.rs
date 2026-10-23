//! The agents' plugin: the chat, the Inspector, the Skills panel, the
//! agents' section of the Petitions panel, their status items, their
//! settings pages and the first-run card. Each follows the focus the
//! agents service keeps, so what one opens or selects the others
//! show.

mod chat;
mod inspector;
mod petitions;
mod settings;
mod shared;
mod skills;
mod status;
mod welcome;

use hamlet_engine_bevy::prelude::*;

use crate::shared::{CONFIRM, confirmation};

#[derive(Default)]
struct Agents;

impl Plugin for Agents {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            shared::plugin,
            chat::plugin,
            inspector::plugin,
            skills::plugin,
            petitions::plugin,
            status::plugin,
            settings::plugin,
            welcome::plugin,
        ))
        .add_overlay(CONFIRM, confirmation);
    }
}

hamlet_engine_bevy::export!(Agents);
