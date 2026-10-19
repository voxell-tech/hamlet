//! {{title}}, a native plugin. It runs as Hamlet, with no limits.

use bevy::app::Update;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy_fynix::views::label;
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_native::App;
use hamlet_sdk::AnyView;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::plugin::Entry;

// Exports what Hamlet looks for. `entry` is called once, when the
// plugin loads, with the app.
hamlet_native::export!(entry);

/// The frames seen since the plugin loaded.
#[derive(Resource, Default)]
pub struct Frames(pub u32);

fn entry(app: &mut App) {
    // Ordinary Bevy: resources, systems, observers. They stay until
    // Hamlet is started again.
    app.init_resource::<Frames>().add_systems(Update, count);
    // What goes through the SDK is removed when the plugin is
    // switched off: panels, commands, menu lines, keys.
    hamlet_native::declare(app, declare);
}

fn count(mut frames: ResMut<Frames>) {
    frames.0 += 1;
}

fn declare(entry: &mut Entry) {
    entry.panel("{{name}}", "{{TITLE}}", Glyph::Book, panel);
}

fn panel() -> AnyView {
    keyed(resource::<Frames, _>(|frames| frames.0 / 60), |seconds| {
        label(format!("Running for about {seconds} seconds")).boxed()
    })
    .boxed()
}
