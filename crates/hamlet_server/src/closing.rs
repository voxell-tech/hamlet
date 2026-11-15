//! How the server stops (S3), as the states it goes through.
//!
//! A stop signal moves it from `Open` to `Closing`: it takes no new
//! client, every connection closes, and every sign-in ends. It stays
//! `Closing` while inputs still come, until the connections are gone
//! and no credential rotation begun is held, or `ROTATING` is over,
//! so no refreshed token is lost. Then it is `Stopping`: the tool
//! runs end and the runtimes stop, each given `STOPPING`; once they
//! have, the app exits.

use std::time::{Duration, Instant};

use bevy_app::{App, AppExit, PreUpdate};
use bevy_ecs::entity::Entity;
use bevy_ecs::error::Result;
use bevy_ecs::message::{Message, MessageWriter};
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_ecs::world::World;
use bevy_state::app::{AppExtStates, StatesPlugin};
use bevy_state::condition::in_state;
use bevy_state::state::{NextState, OnEnter, State, States};

use crate::calls::ToolRun;
use crate::clients::Listening;
use crate::clients::connection::GRACE;
use crate::core::{Core, Leftovers};
use crate::knowledge::runner::Runner;
use crate::server::{AddHandler, InputSender};
use crate::talk::leases::Authority;
use crate::talk::runtimes::Runtimes;
use crate::timers::AddTimer;

/// The time a closing server waits for a credential rotation begun.
const ROTATING: Duration = Duration::from_secs(15);
/// The time a stopping server gives each runtime to stop.
const STOPPING: Duration = Duration::from_secs(30);

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(StatesPlugin)
        .init_state::<Serving>()
        .add_message::<StopAsked>()
        .add_message::<ClientsClosed>()
        .add_systems(
            PreUpdate,
            (
                ask_to_close.run_if(on_message::<StopAsked>),
                note_clients_closed
                    .run_if(on_message::<ClientsClosed>),
                stop_once_settled.run_if(in_state(Serving::Closing)),
            )
                .chain(),
        )
        .add_systems(OnEnter(Serving::Closing), close)
        .add_systems(OnEnter(Serving::Stopping), stop)
        .add_handler::<RuntimesStopped, _>(exit)
        .add_timer(settled_at, wake);
}

/// Whether the server serves, closes or stops.
#[derive(
    States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug,
)]
pub(crate) enum Serving {
    #[default]
    Open,
    Closing,
    Stopping,
}

/// A signal asked the server to stop.
#[derive(Message)]
pub(crate) struct StopAsked;

/// The connections closed, or were given up on after `GRACE`.
#[derive(Message)]
struct ClientsClosed;

/// The runtimes stopped.
#[derive(Message)]
struct RuntimesStopped;

/// What closing waits for: the connections to close, and any
/// credential rotation begun until `rotated_by`.
#[derive(Resource)]
struct Closing {
    clients_closed: bool,
    rotated_by: Instant,
}

/// A stop signal closes an open server.
fn ask_to_close(
    serving: Res<State<Serving>>,
    mut next: ResMut<NextState<Serving>>,
) {
    if *serving.get() == Serving::Open {
        next.set(Serving::Closing);
    }
}

/// Takes no new client, closes every connection, giving them `GRACE`
/// to go, and ends every sign-in.
fn close(
    mut core: Core,
    inputs: Res<InputSender>,
    mut commands: Commands,
) -> Result {
    commands.remove_resource::<Listening>();
    core.clients.close_connections();
    let mut connections =
        std::mem::take(&mut core.clients.connection_tasks);
    let inputs = inputs.clone();
    tokio::spawn(async move {
        let closed = async {
            while connections.join_next().await.is_some() {}
        };
        let _ = tokio::time::timeout(GRACE, closed).await;
        connections.abort_all();
        let _ = inputs.send(ClientsClosed);
    });
    core.providers.close_all();
    core.send_providers()?;
    commands.insert_resource(Closing {
        clients_closed: false,
        rotated_by: Instant::now() + ROTATING,
    });
    Ok(())
}

fn note_clients_closed(mut closing: ResMut<Closing>) {
    closing.clients_closed = true;
}

/// Whether closing is over: the connections are gone, and no
/// rotation is held or the time for it is over.
fn is_settled(closing: &Closing, leases: &Authority) -> bool {
    closing.clients_closed
        && (!leases.is_holding()
            || Instant::now() >= closing.rotated_by)
}

/// Stops a closing server once closing is over.
fn stop_once_settled(
    closing: Res<Closing>,
    leases: Res<Authority>,
    mut next: ResMut<NextState<Serving>>,
) {
    if is_settled(&closing, &leases) {
        next.set(Serving::Stopping);
    }
}

/// When the runner wakes so that `PreUpdate` sees closing is over:
/// at once when it is, or when the time for rotations ends; never
/// while the connections still close, which wake it themselves.
fn settled_at(
    serving: Res<State<Serving>>,
    closing: Option<Res<Closing>>,
    leases: Res<Authority>,
) -> Option<Instant> {
    let closing =
        closing.filter(|_| *serving.get() == Serving::Closing)?;
    if !closing.clients_closed {
        return None;
    }
    Some(if is_settled(&closing, &leases) {
        Instant::now()
    } else {
        closing.rotated_by
    })
}

/// The timer only wakes the runner: `PreUpdate` sees what changed.
fn wake() -> anyhow::Result<()> {
    Ok(())
}

/// Ends every tool run, whose task stops with its entity, and stops
/// the runtimes on a task, which says when they have.
fn stop(
    runs: Query<Entity, With<ToolRun>>,
    mut runtimes: Runtimes,
    inputs: Res<InputSender>,
    mut commands: Commands,
) {
    for run in &runs {
        commands.entity(run).despawn();
    }
    let processes = runtimes.take_processes();
    let inputs = inputs.clone();
    tokio::spawn(async move {
        for process in processes {
            process.stop(STOPPING).await;
        }
        let _ = inputs.send(RuntimesStopped);
    });
}

/// Once the runtimes stopped: what their commands left ends, the
/// jobs runner stops, and the app exits.
fn exit(
    leftovers: Res<Leftovers>,
    mut commands: Commands,
    mut exits: MessageWriter<AppExit>,
) {
    leftovers.end(|_| true);
    commands.remove_resource::<Runner>();
    exits.write(AppExit::Success);
}

/// The check that the server is stopping, when it takes in no
/// runtime's line and runs no timer.
pub(crate) fn is_stopping(world: &World) -> bool {
    world
        .get_resource::<State<Serving>>()
        .is_some_and(|serving| *serving.get() == Serving::Stopping)
}
