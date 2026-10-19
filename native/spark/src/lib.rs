//! A sample native plugin. It adds a resource that counts the frames
//! it has seen, as a Bevy system does, and a panel that shows the
//! count, through the SDK's entry as a built-in plugin does.

use bevy::app::Update;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy_fynix::views::label;
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_native::App;
use hamlet_sdk::AnyView;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::plugin::Entry;

hamlet_native::export!(entry);

/// The frames seen since the plugin loaded.
#[derive(Resource, Default)]
pub struct Sparks(pub u32);

fn entry(app: &mut App) {
    app.init_resource::<Sparks>().add_systems(Update, strike);
    hamlet_native::declare(app, declare);
}

fn strike(mut sparks: ResMut<Sparks>) {
    sparks.0 += 1;
}

fn declare(entry: &mut Entry) {
    entry.panel("spark", "SPARK", Glyph::Book, panel);
}

fn panel() -> AnyView {
    keyed(resource::<Sparks, _>(|sparks| sparks.0 / 60), |seconds| {
        label(format!("Struck for about {seconds} seconds")).boxed()
    })
    .boxed()
}
