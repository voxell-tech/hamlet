//! The agents layer: the model loop, the tools, the sessions and the
//! read models of the agents. It needs no window and nothing else is
//! under it.

use bevy::app::App;
use bevy::ecs::resource::Resource;
use hamlet_agents_api::{agents, convo, papers, roll};
use hamlet_core::core::CorePlugin;
use hamlet_core::session::SessionPlugin;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};

/// Adds the Agents plugin. The app inserts [`Launch`] before it.
pub fn plugin(app: &mut App) {
    let Launch(core) = app
        .world_mut()
        .remove_resource::<Launch>()
        .expect("insert `Launch` before the agents plugin");
    app.add_plugins((
        core,
        SessionPlugin,
        convo::plugin,
        agents::plugin,
        papers::plugin,
        roll::plugin,
    ))
    .add_hamlet_plugin::<AgentsPlugin>();
}

/// How the agents layer starts: the home, the sandbox, the runtime,
/// the settings and the recorded answers if any. The plugin takes it
/// out of the world when it is added.
#[derive(Resource)]
pub struct Launch(pub CorePlugin);

/// The plugin that runs the agents.
pub struct AgentsPlugin;

impl HamletPlugin for AgentsPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("agents").named("Agents").essential()
    }

    fn entry(_: &mut Entry) {}
}
