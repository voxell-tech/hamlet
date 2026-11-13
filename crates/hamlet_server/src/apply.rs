//! What follows an input once its transaction is kept: the inputs it
//! ended followed, the tools narrowed to each agent's role, the jobs
//! it left run, the stopped turns' tool runs ended and the asked ones
//! started, the runtimes sent what they are owed, and the clients
//! sent the state and then their replies. Each step is worked out
//! again from the database, so one that fails is made good by the
//! next input. An input that fails leaves nothing to follow.

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::error::Result;
use bevy_ecs::event::Event;
use bevy_ecs::message::{Message, MessageReader, Messages};
use bevy_ecs::observer::On;
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::{
    IntoScheduleConfigs, Schedule, ScheduleLabel,
    SingleThreadedExecutor, SystemSet,
};
use bevy_ecs::system::{Commands, Res, ResMut};
use bevy_ecs::world::World;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, ConversationId};

use crate::calls::{Runs, Starting, place};
use crate::clients::{Clients, Connections, check};
use crate::core::{Catalogue, Core, Remote};
use crate::credentials::Credentials;
use crate::diagnostic::{Secrets, tell};
use crate::knowledge::runner::{Runner, Writing, run_due};
use crate::later::Later;
use crate::providers::Providers;
use crate::realm::Realm;
use crate::society::agents;
use crate::society::refusal::Refusal;
use crate::talk::follow::follow;
use crate::talk::runtimes::Runtimes;
use crate::talk::sync::{self, Fitting};
use crate::talk::{Env, conversations};

/// The schedule the runner runs after each input.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Apply;

/// The steps of [`Apply`], in their order.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Step {
    /// The inputs that ended are followed.
    Follow,
    Retool,
    /// What the steps before changed is kept.
    Keep,
    /// The jobs the kept changes left are run.
    Jobs,
    StopCancelled,
    /// The tool runs and the tasks the input asked for start.
    Start,
    Flush,
    Publish,
    Reply,
    /// The runtimes are sent what the input queued for them.
    Send,
    /// The lodges the input owes the remote workspace are pushed.
    Push,
}

/// A call held on a wait whose wait was answered: its runtime is
/// told after the input's changes are sent, so asking again may get
/// further.
#[derive(Message)]
pub(crate) struct Wake {
    pub runtime: String,
    pub key: String,
}

/// A runtime's input ended: its follow, which may end a worker or
/// carry an answer back, and the replies owed for it come once what
/// said so is kept. Written as a message by the input, it is
/// triggered for the observers by [`Apply`].
#[derive(Message, Event, Clone)]
pub(crate) struct InputEnded {
    pub conversation: ConversationId,
    pub request: String,
}

/// What the input's changes leave for [`Apply`].
#[derive(Resource, Default)]
pub(crate) struct Changed {
    /// A change was made, kept with the input.
    pub committed: bool,
    /// An agent or a hamlet changed, so its tools may have.
    pub reshaped: bool,
    /// Jobs were kept for the runner.
    pub jobs: bool,
    /// A runtime answered a request, so what waited on it may go.
    pub answered: bool,
    /// What the clients see changed with no change kept, such as how
    /// moving the lodges goes.
    pub republish: bool,
    /// The fault point a crash test stops at once the input is kept,
    /// such as a society call's change kept with its answer.
    pub fault: Option<&'static str>,
}

pub(crate) fn plugin(app: &mut App) {
    let mut apply = Schedule::new(Apply);
    // One thread: SQLite has one writer, and the gain is the order.
    apply.set_executor(SingleThreadedExecutor::new());
    app.add_schedule(apply)
        .init_resource::<Changed>()
        .add_message::<Wake>()
        .add_message::<InputEnded>()
        .add_observer(follow_input)
        .add_observer(settle_owed)
        .configure_sets(
            Apply,
            (
                Step::Follow,
                Step::Retool,
                Step::Keep,
                Step::Jobs,
                Step::StopCancelled,
                Step::Start,
                Step::Flush,
                Step::Publish,
                Step::Reply,
                Step::Send,
                Step::Push,
            )
                .chain(),
        )
        .add_systems(
            Apply,
            (
                end_inputs.in_set(Step::Follow),
                retool.in_set(Step::Retool).run_if(reshaped),
                keep.in_set(Step::Keep),
                jobs.in_set(Step::Jobs).run_if(jobs_kept),
                stop_cancelled
                    .in_set(Step::StopCancelled)
                    .run_if(committed),
                (crate::calls::start, crate::later::start)
                    .in_set(Step::Start),
                (flush.run_if(owed), wake)
                    .chain()
                    .in_set(Step::Flush),
                publish.in_set(Step::Publish).run_if(shown),
                reply.in_set(Step::Reply),
                send.in_set(Step::Send),
                (push, forget).chain().in_set(Step::Push),
            ),
        );
}

/// Drops what a failed input left to follow it: its transaction was
/// rolled back, so nothing it changed is so, and nothing it asked
/// for runs.
pub(crate) fn discard(world: &mut World) -> anyhow::Result<()> {
    *world.resource_mut::<Changed>() = Changed::default();
    world.resource_mut::<Messages<Wake>>().clear();
    world.resource_mut::<Messages<InputEnded>>().clear();
    world.resource_mut::<Clients>().discard();
    world.resource_mut::<Later>().discard();
    world.run_system_cached(|mut runtimes: Runtimes| {
        runtimes.discard();
    })?;
    let starting = world
        .query_filtered::<Entity, With<Starting>>()
        .iter(world)
        .collect::<Vec<_>>();
    for run in starting {
        world.despawn(run);
    }
    Ok(())
}

fn committed(changed: Res<Changed>) -> bool {
    changed.committed
}

fn jobs_kept(changed: Res<Changed>) -> bool {
    changed.jobs
}

/// Whether what the clients see may have changed.
fn shown(changed: Res<Changed>) -> bool {
    changed.committed || changed.republish
}

fn reshaped(changed: Res<Changed>) -> bool {
    changed.reshaped
}

/// Whether a runtime may be owed something new.
fn owed(changed: Res<Changed>) -> bool {
    changed.committed || changed.answered
}

/// Triggers each input that ended for its observers.
fn end_inputs(
    mut ended: MessageReader<InputEnded>,
    mut commands: Commands,
) {
    for input in ended.read() {
        commands.trigger(input.clone());
    }
}

/// Follows an input that ended, and wakes what its follow freed.
fn follow_input(ended: On<InputEnded>, mut core: Core) -> Result {
    crate::fault::point("followed");
    let followed = core.change(Actor::Realm, |cx, env| {
        follow(cx, env, ended.conversation, &ended.request)
    })?;
    for (runtime, key) in followed.wake {
        core.wake_call(&runtime, &key);
    }
    Ok(())
}

/// Answers the replies owed for an input that ended.
fn settle_owed(ended: On<InputEnded>, mut clients: ResMut<Clients>) {
    let owed = clients.owed.extract_if(.., |owed| {
        owed.conversation == ended.conversation
            && owed.request == ended.request
    });
    let answers = owed
        .map(|owed| {
            let answer = check::reply(
                &owed.id,
                Ok::<_, Refusal>(String::new()),
            );
            (owed.client, answer)
        })
        .collect::<Vec<_>>();
    for (client, answer) in answers {
        clients.send(client, answer);
    }
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
            let tools = place::tools(db, agent, kept.kind, &names)?;
            if kept.tools()?.as_ref() != Some(&tools) {
                conversations::set_tools(db, kept.id, &tools)?;
            }
        }
    }
    Ok(())
}

/// Keeps what the follows and the retooling changed. A commit that
/// fails leaves nothing of them.
fn keep(mut realm: ResMut<Realm>) -> Result {
    let db = realm.store().connection();
    if let Err(error) = hamlet_store::commit(db) {
        hamlet_store::unwind(db)?;
        return Err(error.into());
    }
    Ok(())
}

/// Runs the jobs the kept changes left, and wakes the runner for what
/// they leave in turn. Their effects come before anything that reads
/// them; one that fails is run again by the runner.
fn jobs(
    mut realm: ResMut<Realm>,
    writing: Res<Writing>,
    runner: Res<Runner>,
    mut remote: Remote,
) {
    crate::fault::point("jobs");
    let layout = realm.layout().clone();
    let db = realm.store().connection();
    match run_due(db, &layout, &writing) {
        Ok(done) => remote.owe_pushes(db, &done),
        Err(error) => {
            tell(Secrets::Fixed, format!("running jobs: {error:#}"));
        }
    }
    runner.wake();
}

/// Stops the tool runs of every conversation being stopped: their
/// entities go, and their tasks with them.
fn stop_cancelled(
    mut realm: ResMut<Realm>,
    runs: Runs,
    mut commands: Commands,
) -> Result {
    let db = realm.store().connection();
    for (entity, running) in &runs {
        if conversations::get(db, running.conversation)?.cancelling {
            commands.entity(entity).despawn();
        }
    }
    Ok(())
}

/// Sends every ready runtime what it is owed.
fn flush(
    mut realm: ResMut<Realm>,
    env: Res<Env>,
    mut runtimes: Runtimes,
    providers: Res<Providers>,
    mut fitting: ResMut<Fitting>,
) -> Result {
    let images = realm.layout().images();
    let all = runtimes.all();
    let mut owed = sync::Owed {
        db: realm.store().connection(),
        env: &env,
        runtimes: &mut runtimes,
        providers: &providers,
        fitting: &mut fitting,
        images: &images,
    };
    for runtime in all {
        sync::flush(&mut owed, runtime, false)?;
    }
    Ok(())
}

fn publish(mut core: Core) -> Result {
    core.send_state()?;
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

/// Tells each runtime of the held calls whose waits were answered.
fn wake(mut wakes: MessageReader<Wake>, mut runtimes: Runtimes) {
    for wake in wakes.read() {
        runtimes.answered(&wake.runtime, &wake.key);
    }
}

/// Writes each runtime the lines queued for it.
fn send(mut runtimes: Runtimes) {
    runtimes.deliver();
}

/// Starts pushing the lodges the input owes the remote workspace.
fn push(
    mut realm: ResMut<Realm>,
    writing: Res<Writing>,
    credentials: Res<Credentials>,
    mut remote: Remote,
) -> Result {
    remote.push_owed(&mut realm, &writing, &credentials.secrets())?;
    Ok(())
}

/// The input is followed up: the next starts afresh. No frame turns
/// the messages over here, so the last step does.
fn forget(
    mut changed: ResMut<Changed>,
    mut wakes: ResMut<Messages<Wake>>,
    mut ended: ResMut<Messages<InputEnded>>,
) {
    *changed = Changed::default();
    wakes.update();
    ended.update();
}
