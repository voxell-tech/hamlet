//! The About panel: the engine's name and version, and the version of
//! the interface its plugins are built against.

use hamlet_engine_bevy::ENGINE_VERSION;
use hamlet_engine_bevy::api::API_VERSION;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{Tone, column, heading, label};

#[derive(Default)]
struct About;

impl Plugin for About {
    fn build(&self, app: &mut App) {
        app.add_panel("about", "About", about);
    }
}

hamlet_engine_bevy::export!(About);

fn about() -> AnyView {
    column((
        heading("Hamlet"),
        label(format!("Engine {ENGINE_VERSION}")),
        label(format!("Plugin interface {API_VERSION}"))
            .tone(Tone::Dim),
    ))
    .boxed()
}
