//! What follows an input's changes, once its handler is done: the
//! tools narrowed to each agent's role, the stopped turns' tool runs
//! ended, the runtimes sent what they are owed, the clients sent the
//! state and then their replies. Each step is worked out again from
//! the database, so one that fails is made good by the next input.

use bevy_app::App;
use bevy_ecs::error::Result;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::{
    IntoScheduleConfigs, Schedule, ScheduleLabel,
    SingleThreadedExecutor, SystemSet,
};
use bevy_ecs::system::{Res, ResMut};
use hamlet_store::Db;

use crate::calls::{Calls, place};
use crate::clients::{Clients, Connections};
use crate::core::{Catalogue, Core};
use crate::credentials::Credentials;
use crate::providers::Providers;
use crate::realm::Realm;
use crate::society::agents;
use crate::talk::runtimes::Runtimes;
use crate::talk::sync::{self, Fitting};
use crate::talk::{Env, conversations};
use crate::work::catalogue::SOCIETY;

/// The schedule the runner runs after each input.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Apply;

/// The steps of [`Apply`], in their order.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Step {
    Retool,
    StopCancelled,
    Flush,
    Publish,
    Reply,
}

/// What the input's changes leave for [`Apply`].
#[derive(Resource, Default)]
pub(crate) struct Changed {
    /// A change was kept.
    pub committed: bool,
    /// An agent or a hamlet changed, so its tools may have.
    pub reshaped: bool,
    /// A runtime answered a request, so what waited on it may go.
    pub answered: bool,
}

pub(crate) fn plugin(app: &mut App) {
    let mut apply = Schedule::new(Apply);
    // One thread: SQLite has one writer, and the gain is the order.
    apply.set_executor(SingleThreadedExecutor::new());
    app.add_schedule(apply)
        .init_resource::<Changed>()
        .configure_sets(
            Apply,
            (
                Step::Retool,
                Step::StopCancelled,
                Step::Flush,
                Step::Publish,
                Step::Reply,
            )
                .chain(),
        )
        .add_systems(
            Apply,
            (
                retool.in_set(Step::Retool).run_if(reshaped),
                stop_cancelled
                    .in_set(Step::StopCancelled)
                    .run_if(committed),
                flush.in_set(Step::Flush).run_if(owed),
                publish.in_set(Step::Publish).run_if(committed),
                (reply, forget).chain().in_set(Step::Reply),
            ),
        );
}

fn committed(changed: Res<Changed>) -> bool {
    changed.committed
}

fn reshaped(changed: Res<Changed>) -> bool {
    changed.reshaped
}

/// Whether a runtime may be owed something new.
fn owed(changed: Res<Changed>) -> bool {
    changed.committed || changed.answered
}

fn retool(
    mut realm: ResMut<Realm>,
    catalogue: Res<Catalogue>,
) -> Result {
    retool_all(realm.store().connection(), &catalogue)?;
    Ok(())
}

/// Narrows the tools of every living agent's conversations to its
/// role now; a runtime is told before the next input.
pub(crate) fn retool_all(
    db: &mut Db,
    catalogue: &Catalogue,
) -> anyhow::Result<()> {
    let names = catalogue.names();
    for agent in agents::living(db)? {
        for kept in conversations::of_agent(db, agent)? {
            let tools =
                place::tools(db, agent, kept.kind, &names, &SOCIETY)?;
            if kept.tools()?.as_ref() != Some(&tools) {
                conversations::set_tools(db, kept.id, &tools)?;
            }
        }
    }
    Ok(())
}

/// Stops the tool runs of every conversation being stopped.
fn stop_cancelled(
    mut realm: ResMut<Realm>,
    mut calls: ResMut<Calls>,
) -> Result {
    let db = realm.store().connection();
    for conversation in calls.conversations() {
        if conversations::get(db, conversation)?.cancelling {
            calls.stop(conversation);
        }
    }
    Ok(())
}

/// Sends every ready runtime what it is owed.
fn flush(
    mut realm: ResMut<Realm>,
    env: Res<Env>,
    mut runtimes: ResMut<Runtimes>,
    providers: Res<Providers>,
    mut fitting: ResMut<Fitting>,
) -> Result {
    let images = realm.layout().images();
    let mut owed = sync::Owed {
        db: realm.store().connection(),
        env: &env,
        runtimes: &mut runtimes,
        providers: &providers,
        fitting: &mut fitting,
        images: &images,
    };
    for index in 0..owed.runtimes.list.len() {
        sync::flush(&mut owed, index, false)?;
    }
    Ok(())
}

fn publish(mut core: Core) -> Result {
    core.publish()?;
    Ok(())
}

/// Sends the clients what the input queued for them, after the state
/// it left.
fn reply(
    mut clients: ResMut<Clients>,
    mut connections: Connections,
    credentials: Res<Credentials>,
) {
    clients.deliver(&mut connections, &credentials);
}

fn forget(mut changed: ResMut<Changed>) {
    *changed = Changed::default();
}
