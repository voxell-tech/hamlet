//! The chat with an agent: who it is with, how it is read and typed,
//! the markdown and pictures in it, selecting its words and the deeds
//! that ask the chief first.

pub mod chat;
pub mod links;
pub mod markdown;
pub mod me;
pub mod pictures;
pub mod select;
mod tuning;

use bevy::app::App;
pub use hamlet_agents_api::roll;
pub use hamlet_chat_api::deed;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};
pub use hamlet_sdk::{
    AnyView, Cx, View, dock, feel, icons, looks, platform, theme,
    with_cx,
};

/// The chat and what it is made of. Add it after the SDK's plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins((
        tuning::plugin,
        me::plugin,
        select::plugin,
        chat::plugin,
        links::plugin,
        pictures::plugin,
    ))
    .add_hamlet_plugin::<ChatPlugin>();
}

/// The built-in plugin of the chat. Essential: the chat is where the
/// chief talks to the agents.
pub struct ChatPlugin;

impl HamletPlugin for ChatPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("chat").named("Chat").essential()
    }

    fn entry(_: &mut Entry) {}
}
