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
use bevy_ecs::system::ResMut;
use bevy_ecs::world::World;
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
            open_clients,
            listen,
        )
            .chain(),
    )
    .add_systems(
        Startup,
        (
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
fn open_sandbox(world: &mut World) -> Result {
    let sandbox =
        Sandbox::open(world.resource::<Options>().isolation)?;
    sandbox.probe()?;
    world.insert_resource(Confined(sandbox));
    world.insert_resource(Shells::local());
    Ok(())
}

/// Opens the realm the options name, made if it is new, with its
/// settings and the folders agents read even when empty.
fn open_realm(world: &mut World) -> Result {
    let options = world.resource::<Options>();
    let (home, faux) = (options.home.clone(), options.faux.is_some());
    let mut realm = Realm::open(&home, &chief_name(), clock::now())?;
    let overrides = Overrides::from_env(faux);
    let saved = settings::stored(realm.store().connection())?;
    let layout = realm.layout().clone();
    crate::knowledge::library::open(&layout, clock::now())?;
    for folder in [layout.tools(), layout.images()] {
        std::fs::create_dir_all(&folder).with_context(|| {
            format!("making {}", folder.display())
        })?;
    }
    world.insert_resource(realm);
    world.insert_resource(Env {
        settings: overrides.apply(saved),
        overrides,
    });
    Ok(())
}

/// Opens the realm's credentials, and the remote workspace they
/// name, if any.
fn open_credentials(world: &mut World) -> Result {
    let root = world.resource::<Realm>().layout().root().to_owned();
    let credentials = Credentials::open(&root)?;
    let workspace = credentials
        .workspace()
        .cloned()
        .map(Transport::new)
        .transpose()?;
    world.insert_resource(Workspace(workspace));
    world.insert_resource(credentials);
    Ok(())
}

fn open_providers(world: &mut World) -> Result {
    let layout = world.resource::<Realm>().layout().clone();
    let faux = world.resource::<Env>().overrides.faux;
    world.insert_resource(Providers::open(&layout, faux)?);
    Ok(())
}

/// Starts the jobs runner on a thread of its own.
fn start_jobs_runner(world: &mut World) -> Result {
    let layout = world.resource::<Realm>().layout().clone();
    let inputs = world.resource::<InputSender>().clone();
    let writing = RepositoryWriteLock::default();
    let runner = Runner::start(&layout, inputs, writing.clone())?;
    world.insert_resource(runner);
    world.insert_resource(writing);
    Ok(())
}

/// Readies the harness the runtimes run, and the runtime new
/// conversations go to.
fn open_harness(world: &mut World) -> Result {
    let pi = Pi::from_env(world.resource::<Options>().faux.clone())?;
    let inputs = world.resource::<InputSender>().clone();
    runtimes::of_harness(
        world.resource_mut::<Realm>().store().connection(),
        agents::HARNESS,
    )?;
    world.insert_resource(Harness::new(pi, inputs));
    Ok(())
}

/// Watches the folder of the tools added by hand, so they load again
/// while the server runs; without a watch, they load only now.
fn watch_added_tools(world: &mut World) -> Result {
    let tools = world.resource::<Realm>().layout().tools();
    let inputs = world.resource::<InputSender>().clone();
    let watching = added::watch(&tools, inputs)
        .inspect_err(|error| {
            crate::diagnostic::tell(
                crate::diagnostic::Secrets::Fixed,
                format!(
                    "the tools added by hand load only at start: \
                     {error}"
                ),
            );
        })
        .ok();
    world.insert_resource(AddedToolsWatch(watching));
    Ok(())
}

fn open_clients(world: &mut World) -> Result {
    let inputs = world.resource::<InputSender>().clone();
    world.insert_resource(Clients::new(inputs, clock::now())?);
    Ok(())
}

/// Listens for clients on the port the options name. A client that
/// connects now waits for the first update to end.
fn listen(world: &mut World) -> Result {
    let port = world.resource::<Options>().port;
    let listener =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let inputs = world.resource::<InputSender>().clone();
    world.insert_resource(Listening::start(listener, inputs)?);
    Ok(())
}

/// Publishes the server's address in the realm's `server.json` once
/// its first update is over, so a client finds it only when the
/// start is done; it stays while the server serves.
#[expect(
    clippy::print_stderr,
    reason = "the banner is the server's output for whoever started \
              it"
)]
fn publish_address(world: &mut World) -> Result {
    let url =
        format!("ws://{}", world.resource::<Listening>().address);
    let confinement =
        world.run_system_cached(|core: Core| core.confinement())?;
    let layout = world.resource::<Realm>().layout().clone();
    let started = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let address = json!({
        "url": url,
        "pid": std::process::id(),
        "started": started,
        "confinement": confinement,
    });
    write_whole(&layout.server(), &serde_json::to_vec(&address)?)?;
    world.insert_resource(Published(layout.server()));
    eprintln!(
        "hamlet_server: holding {}; serving at {url}; Ctrl-C stops",
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
