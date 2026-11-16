//! How the server comes up, in its first update.
//!
//! `PreStartup` opens what it serves, in this order: the sandbox,
//! the realm and its settings, the credentials and the remote
//! workspace, the providers, the jobs runner, the harness, the watch
//! on the tools added by hand, the clients, and last the listener.
//! One that fails stops the start.
//!
//! `Startup` then starts it: the runtimes start, the tools added by
//! hand load, each agent's tools are narrowed to its role, the
//! providers are asked what they serve, and the turns the last stop
//! left are followed. `PostUpdate` keeps what they changed, and at
//! the end of `Last` the server's address is published.

use std::net::Ipv4Addr;
use std::path::PathBuf;

use anyhow::Context as _;
use bevy_app::{App, Last, PreStartup, Startup};
use bevy_ecs::error::Result;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::{not, resource_exists};
use bevy_ecs::system::{Commands, Res, ResMut};
use bevy_log::{info, warn};
use hamlet_sandbox::Sandbox;
use hamlet_store::ids::Actor;
use serde_json::json;

use crate::clients::{Clients, Listening};
use crate::core::{Confined, Core, Workspace};
use crate::credentials::{Credentials, write_whole};
use crate::knowledge::runner::{RepositoryWriteLock, Runner};
use crate::options::Options;
use crate::providers::Providers;
use crate::realm::Realm;
use crate::server::InputSender;
use crate::settings::{self, Overrides};
use crate::society::agents;
use crate::talk::follow::follow;
use crate::talk::process::Pi;
use crate::talk::runtimes::{self, Harness, Runtimes};
use crate::talk::{Env, inputs};
use crate::terminals::{Terminals, kept};
use crate::work::added::{self, AddedToolsWatch};
use crate::work::tools::shell::Shells;
use crate::workspace::Transport;
use crate::{ServerSystems, clock};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        PreStartup,
        (
            open_sandbox,
            open_realm,
            open_credentials,
            open_providers,
            start_jobs_runner,
            open_harness,
            watch_added_tools,
            open_terminals,
            open_clients,
            listen,
        )
            .chain(),
    )
    .add_systems(
        Startup,
        (
            kept::restore,
            start_runtimes,
            added::reload,
            crate::follow_up::narrow_tools_to_roles,
            ask_providers,
            follow_unfollowed,
        )
            .chain(),
    )
    .add_systems(
        Last,
        publish_address
            .after(ServerSystems::ForgetInput)
            .run_if(not(resource_exists::<Published>)),
    );
}

/// Probes the sandbox the agents' commands run in on this machine.
fn open_sandbox(
    options: Res<Options>,
    mut commands: Commands,
) -> Result {
    let sandbox = Sandbox::open(options.isolation)?;
    sandbox.probe()?;
    commands.insert_resource(Confined(sandbox));
    commands.insert_resource(Shells::local());
    Ok(())
}

/// Opens the realm the options name, made if it is new, with its
/// settings and the folders agents read even when empty.
fn open_realm(
    options: Res<Options>,
    mut commands: Commands,
) -> Result {
    let mut realm =
        Realm::open(&options.home, &chief_name(), clock::now())?;
    let overrides = Overrides::from_env(options.faux.is_some());
    let saved = settings::stored(realm.store().connection())?;
    let layout = realm.layout().clone();
    crate::knowledge::library::open(&layout, clock::now())?;
    for folder in [layout.tools(), layout.images()] {
        std::fs::create_dir_all(&folder).with_context(|| {
            format!("making {}", folder.display())
        })?;
    }
    commands.insert_resource(realm);
    commands.insert_resource(Env {
        settings: overrides.apply(saved),
        overrides,
    });
    Ok(())
}

/// Opens the realm's credentials, and the remote workspace they
/// name, if any.
fn open_credentials(
    realm: Res<Realm>,
    mut commands: Commands,
) -> Result {
    let credentials = Credentials::open(realm.layout().root())?;
    let workspace = credentials
        .workspace()
        .cloned()
        .map(Transport::new)
        .transpose()?;
    commands.insert_resource(Workspace(workspace));
    commands.insert_resource(credentials);
    Ok(())
}

fn open_providers(
    realm: Res<Realm>,
    env: Res<Env>,
    mut commands: Commands,
) -> Result {
    let providers =
        Providers::open(realm.layout(), env.overrides.faux)?;
    commands.insert_resource(providers);
    Ok(())
}

/// Starts the jobs runner on a thread of its own.
fn start_jobs_runner(
    realm: Res<Realm>,
    inputs: Res<InputSender>,
    mut commands: Commands,
) -> Result {
    let writing = RepositoryWriteLock::default();
    let runner = Runner::start(
        realm.layout(),
        inputs.clone(),
        writing.clone(),
    )?;
    commands.insert_resource(runner);
    commands.insert_resource(writing);
    Ok(())
}

/// Readies the harness the runtimes run, and the runtime new
/// conversations go to.
fn open_harness(
    options: Res<Options>,
    mut realm: ResMut<Realm>,
    inputs: Res<InputSender>,
    mut commands: Commands,
) -> Result {
    let pi = Pi::from_env(options.faux.clone())?;
    runtimes::of_harness(
        realm.store().connection(),
        agents::HARNESS,
    )?;
    commands.insert_resource(Harness::new(pi, inputs.clone()));
    Ok(())
}

/// Watches the folder of the tools added by hand, so they load again
/// while the server runs; without a watch, they load only now.
fn watch_added_tools(
    realm: Res<Realm>,
    inputs: Res<InputSender>,
    mut commands: Commands,
) {
    let watching =
        added::watch(&realm.layout().tools(), inputs.clone())
            .inspect_err(|error| {
                warn!(
                    "the tools added by hand load only at start: \
                     {error}"
                );
            })
            .ok();
    commands.insert_resource(AddedToolsWatch(watching));
}

/// Opens the realm's terminals, none yet: the kept ones open again
/// once everything has started.
fn open_terminals(realm: Res<Realm>, mut commands: Commands) {
    commands
        .insert_resource(Terminals::new(realm.layout().terminals()));
}

fn open_clients(
    inputs: Res<InputSender>,
    mut commands: Commands,
) -> Result {
    commands
        .insert_resource(Clients::new(inputs.clone(), clock::now())?);
    Ok(())
}

/// Listens for clients on the port the options name. A client that
/// connects now waits for the first update to end.
fn listen(
    options: Res<Options>,
    inputs: Res<InputSender>,
    mut commands: Commands,
) -> Result {
    let listener = std::net::TcpListener::bind((
        Ipv4Addr::LOCALHOST,
        options.port,
    ))?;
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    commands
        .insert_resource(Listening::start(listener, inputs.clone())?);
    Ok(())
}

/// Publishes the server's address in the realm's `server.json` once
/// its first update is over, so a client finds it only when the
/// start is done; it stays while the server serves.
fn publish_address(
    listening: Res<Listening>,
    mut core: Core,
) -> Result {
    let url = format!("ws://{}", listening.address);
    let confinement = core.confinement();
    let layout = core.realm.layout().clone();
    let started = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let address = json!({
        "url": url,
        "pid": std::process::id(),
        "started": started,
        "confinement": confinement,
    });
    write_whole(&layout.server(), &serde_json::to_vec(&address)?)?;
    core.commands.insert_resource(Published(layout.server()));
    info!(
        "holding {}; serving at {url}; Ctrl-C stops",
        hamlet_os::fs::path::text(layout.root())
    );
    Ok(())
}

/// The published address, removed when the server stops.
#[derive(Resource)]
struct Published(PathBuf);

impl Drop for Published {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The chief's name (C7): `HAMLET_USER` when set, else the account
/// the server runs as, else "chief".
fn chief_name() -> String {
    ["HAMLET_USER", "USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|name| {
            std::env::var(name).ok().filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| "chief".into())
}

fn start_runtimes(
    realm: ResMut<Realm>,
    runtimes: Runtimes,
) -> Result {
    runtimes::start(realm, runtimes)?;
    Ok(())
}

fn ask_providers(mut core: Core) -> Result {
    core.start_providers()?;
    Ok(())
}

/// Follows each input the last stop left unfollowed; no runtime waits
/// on anything from before it.
fn follow_unfollowed(mut core: Core) -> Result {
    let unfollowed =
        inputs::unfollowed(core.realm.store().connection())?;
    for input in unfollowed {
        core.change(Actor::Realm, |cx, env| {
            follow(cx, env, input.conversation, &input.request)
        })?;
    }
    Ok(())
}
