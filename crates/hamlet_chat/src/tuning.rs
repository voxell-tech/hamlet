//! `assets/chat.toml`, the settings struct of each section kept in
//! the module it tunes.

use bevy::app::App;
use bevy::asset::Asset;
use bevy::ecs::world::World;
use bevy::reflect::TypePath;
use hamlet_assets::{File, Tuning, TuningPlugin, designed, follow};

use crate::chat::ChatSettings;

/// Loads `assets/chat.toml` and keeps the chat in step with it.
pub fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<ChatTuning>::default());
}

/// The chat's tuning, as `assets/chat.toml` has it.
#[derive(Asset, TypePath, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatTuning {
    chat: ChatSettings,
}

impl Tuning for ChatTuning {
    const FILE: File = hamlet_assets::CHAT;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.chat, &self.chat);
    }
}

designed!(ChatTuning {
    ChatSettings => chat,
});
