//! The agents service. It runs the agent core (sessions, workers,
//! oversight, the catalogue, the treasury) in the engine's world, on
//! a runtime of its own, with the engine's home as the agents' home.
//! Plugins reach it with `AgentsRequest`s, each behind an agents
//! capability, and watch it for `AgentsEvent`s. Keys, tokens and
//! sign-ins never leave it.

mod answer;
mod archive;
mod conversation;
mod focus;
mod model_choice;
mod needs;
mod roster;
mod setup;
mod views;
mod watch;

#[cfg(test)]
mod tests;

use std::time::Duration;

use bevy::app::{App, Startup};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res};
use hamlet_core::agents::{self, WatchEvery};
use hamlet_core::core::{CorePlugin, Settings, TestConnection};
use hamlet_core::oversight::OversightPlugin;
use hamlet_core::session::{self, SessionKey, SessionPlugin};
use hamlet_engine_api::agents::{Recipient, Selected};
use hamlet_engine_core::home::KEYCHAIN_SERVICE;
use hamlet_engine_core::{Home, LogLine};
use hamlet_home::limits;
use hamlet_model::config::{Config, use_keychain_service};
use hamlet_rules::hamlet::HOME;
use tokio::runtime::Runtime;

pub use crate::answer::AgentsWorldExt;
pub use crate::setup::AgentsConfigExt;

/// The channel the chief's sessions are kept under.
const CHANNEL: &str = "app";

/// The time between two looks at the hamlets' files.
const WATCH_EVERY: Duration = Duration::from_millis(200);

pub fn plugin(app: &mut App) {
    app.add_plugins(answer::plugin);
    match start(app) {
        Ok(()) => {
            app.add_plugins((setup::plugin, watch::plugin))
                .add_plugins((
                    focus::plugin,
                    conversation::plugin,
                    roster::plugin,
                    needs::plugin,
                ))
                .add_systems(Startup, first_test);
        }
        Err(why) => {
            app.insert_resource(Unavailable(why))
                .add_systems(Startup, tell_unavailable);
        }
    }
}

/// The runtime the agents' models and tools run on.
#[derive(Resource)]
struct AgentsRuntime(
    #[expect(dead_code, reason = "kept alive")] Runtime,
);

/// Why the agents cannot run.
#[derive(Resource)]
struct Unavailable(String);

/// The chief who speaks through the engine, and the window's focus:
/// the open session, which the status tells of, who it is with, and
/// the thing selected.
#[derive(Resource)]
pub struct Chief {
    pub name: String,
    pub session: Entity,
    pub with: Recipient,
    pub selected: Selected,
}

/// Opens the agents' home in the engine's, and runs the agent core
/// in `app`.
fn start(app: &mut App) -> Result<(), String> {
    let failed = |why: &dyn std::fmt::Display| format!("{why:#}");
    use_keychain_service(KEYCHAIN_SERVICE);
    let root = app.world().resource::<Home>().root().to_path_buf();
    let home = hamlet_home::Home::open(root, None)
        .map_err(|why| failed(&why))?;
    limits::write_template(&home.root).map_err(|why| failed(&why))?;
    let sandbox =
        hamlet_sandbox::detect(false).map_err(|why| failed(&why))?;
    let config =
        Config::load(&home.root).map_err(|why| failed(&why))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("hamlet agents")
        .enable_all()
        .build()
        .map_err(|why| failed(&why))?;
    let tokio = runtime.handle().clone();
    app.insert_resource(AgentsRuntime(runtime))
        .insert_resource(WatchEvery(WATCH_EVERY))
        .add_plugins((
            CorePlugin {
                home,
                sandbox,
                tokio,
                config,
                replay: None,
            },
            SessionPlugin,
            OversightPlugin,
        ));
    let name = std::env::var("HAMLET_USER")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".into());
    let world = app.world_mut();
    let key = SessionKey::with_mayor(CHANNEL, &name, HOME);
    let session = session::ensure(world, &key);
    if let Some(mayor) = agents::mayor(world) {
        session::gather(world, mayor);
    }
    app.insert_resource(Chief {
        name,
        session,
        with: Recipient::Mayor(HOME.into()),
        selected: Selected::Nothing,
    });
    Ok(())
}

fn first_test(settings: Res<Settings>, mut commands: Commands) {
    commands.trigger(TestConnection(settings.config.clone()));
}

fn tell_unavailable(
    unavailable: Res<Unavailable>,
    mut log: MessageWriter<LogLine>,
) {
    log.write(LogLine::error(
        "agents",
        format!("the agents cannot run: {}", unavailable.0),
    ));
}
