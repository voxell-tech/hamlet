//! What a panel shows while its plugin is stopped.

use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column};
use hamlet_sdk::AnyView;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::looks::{action, note, stated};
use hamlet_sdk::plugin::Enable;

use crate::panel;

/// A word that the plugin stopped, why, and the press that starts it
/// again.
pub(super) fn view(id: &str, why: &str) -> AnyView {
    let plugin = panel::plugin_of(id).to_string();
    column((
        stated(
            Glyph::Stop,
            "This plugin stopped",
            Tone::Critical,
            13.0,
        ),
        note(why.to_string()),
        action(Glyph::Restore, "RESTART", move |world| {
            world.trigger(Enable(plugin.clone()));
            world.flush();
        }),
    ))
    .gap(8.0)
    .boxed()
}
