//! The About panel: the engine's name and version, and the version of
//! the interface its plugins are built against.

use hamlet_engine_sdk::api::API_VERSION;
use hamlet_engine_sdk::ui::{
    AnyView, Tone, ViewExt, World, column, heading, label,
};
use hamlet_engine_sdk::{ENGINE_VERSION, Entry, Plugin};

#[derive(Default)]
struct About;

impl Plugin for About {
    fn declare(&self, entry: &mut Entry) {
        entry.panel("about", "About");
    }

    fn mount(&mut self, _: &str, _: &mut World) -> AnyView {
        column((
            heading("Hamlet"),
            label(format!("Engine {ENGINE_VERSION}")),
            label(format!("Plugin interface {API_VERSION}"))
                .tone(Tone::Dim),
        ))
        .boxed()
    }
}

hamlet_engine_sdk::export!(About);
