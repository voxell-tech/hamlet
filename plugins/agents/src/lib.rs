//! The agents layer: the model loop, the tools, the sessions, the
//! oversight that holds a tool call for the chief and the read models
//! of the agents. It needs no window and nothing else is under it.

use bevy::app::App;
use bevy::ecs::resource::Resource;
use hamlet_agents_api::{agents, convo, papers, roll};
use hamlet_core::core::{
    self, HomeDir, Model, Sandboxing, Settings, Tokio,
};
use hamlet_core::{catalogue, oversight, session};
use hamlet_home::Home;
use hamlet_model::config::Config;
use hamlet_sandbox::Sandbox;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};
use serde_json::Value;

/// Adds the Agents plugin. The app inserts [`Launch`] before it.
pub fn plugin(app: &mut App) {
    let Launch {
        home,
        sandbox,
        tokio,
        config,
        replay,
    } = app
        .world_mut()
        .remove_resource::<Launch>()
        .expect("insert `Launch` before the agents plugin");
    let model = Model::new(&config, &home.root, replay);
    app.insert_resource(Tokio(tokio))
        .insert_resource(HomeDir(home))
        .insert_resource(Sandboxing(sandbox))
        .insert_resource(model)
        .insert_resource(Settings {
            config,
            error: None,
        })
        .add_plugins((
            core::plugin,
            hamlet_core::agents::plugin,
            catalogue::plugin,
            session::plugin,
            // With the sessions, so that no tool runs ungated.
            oversight::plugin,
            convo::plugin,
            agents::plugin,
            papers::plugin,
            roll::plugin,
        ))
        .add_hamlet_plugin::<AgentsPlugin>();
}

/// How the agents layer starts. The plugin takes it out of the world
/// when it is added.
#[derive(Resource)]
pub struct Launch {
    pub home: Home,
    pub sandbox: &'static dyn Sandbox,
    /// The runtime the model streams and the subprocesses run on.
    pub tokio: tokio::runtime::Handle,
    pub config: Config,
    /// Recorded assistant messages that answer in a model's place.
    pub replay: Option<Vec<Value>>,
}

/// The plugin that runs the agents.
pub struct AgentsPlugin;

impl HamletPlugin for AgentsPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("agents").named("Agents").essential()
    }

    fn entry(_: &mut Entry) {}
}
