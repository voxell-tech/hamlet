//! The asset store. An asset is bytes and the name of its type, with
//! an id, a version, the plugin that owns it and a name; the engine
//! never looks inside. Plugins reach it through requests, answered on
//! the next frame.

mod answer;
mod hold;
mod registry;
mod store;

#[cfg(test)]
mod tests;

use bevy::app::{App, PostUpdate, Startup};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use hamlet_engine_core::{Home, LogLine};

pub use self::answer::{Outcome, Rules};
pub use self::hold::Holding;
pub use self::registry::TypeRegistry;
pub use self::store::{AssetStore, FOLDER, INDEX};

pub fn plugin(app: &mut App) {
    let home = app.world().resource::<Home>().clone();
    app.add_plugins(hold::plugin)
        .insert_resource(AssetStore::open(&home))
        .init_resource::<TypeRegistry>()
        .add_systems(Startup, tell_held)
        .add_systems(
            PostUpdate,
            (registry::follow, answer::answer).chain(),
        );
}

fn tell_held(
    store: Res<AssetStore>,
    mut log: MessageWriter<LogLine>,
) {
    if let Some(why) = store.held() {
        log.write(LogLine::error(
            "assets",
            format!(
                "{FOLDER}/{INDEX} cannot be read ({why}). It is \
                 left as it is, and no asset is written until the \
                 engine starts with one that can be."
            ),
        ));
    }
}
