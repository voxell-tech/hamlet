//! The steps that follow an input once its transaction is kept: the
//! inputs it ended followed, the tools narrowed to each agent's role,
//! the jobs it left run, the stopped turns' tool runs ended and the
//! asked ones started, the runtimes sent what they are owed, and the
//! clients sent the state and then their replies. Each step is worked
//! out again from the database, so one that fails is made good by the
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

use crate::background::BackgroundWork;
use crate::calls::{ToolRuns, UnstartedToolRun, place};
use crate::clients::{Connections, OutgoingClientMessage, check};
use crate::core::{Catalogue, Core, RemoteWorkspace};
use crate::credentials::Credentials;
use crate::diagnostic::{Secrets, tell};
use crate::knowledge::runner::{
    RepositoryWriteLock, Runner, run_due,
};
use crate::providers::Providers;
use crate::realm::Realm;
use crate::society::agents;
use crate::society::refusal::Refusal;
use crate::talk::follow::follow;
use crate::talk::inputs::Resend;
use crate::talk::runtimes::Runtimes;
use crate::talk::sync::{self, ImageFitting};
use crate::talk::{Env, conversations};

pub(crate) fn plugin(app: &mut App) {
    let mut after_input = Schedule::new(AfterInput);
    // One thread: SQLite has one writer, and the gain is the order.
    after_input.set_executor(SingleThreadedExecutor::new());
    app.add_schedule(after_input)
        .init_resource::<InputEffects>()
        .add_message::<WakeHeldCall>()
        .add_message::<InputEnded>()
        .add_observer(follow_ended_input)
        .add_observer(send_owed_replies)
        .configure_sets(
            AfterInput,
            (
                AfterInputStep::FollowEndedInputs,
                AfterInputStep::NarrowToolsToRoles,
                AfterInputStep::CommitInput,
                AfterInputStep::RunKeptJobs,
                AfterInputStep::StopCancelledToolRuns,
                AfterInputStep::StartToolRunsAndBackgroundWork,
                AfterInputStep::SendRuntimesWhatTheyAreOwed,
                AfterInputStep::PublishState,
                AfterInputStep::SendClientMessages,
                AfterInputStep::WriteRuntimeLines,
                AfterInputStep::PushOwedLodges,
            )
                .chain(),
        )
        .add_systems(
            AfterInput,
            (
                trigger_ended_inputs
                    .in_set(AfterInputStep::FollowEndedInputs),
                narrow_tools_to_roles
                    .in_set(AfterInputStep::NarrowToolsToRoles)
                    .run_if(has_agents_or_hamlets_changed),
                commit_input.in_set(AfterInputStep::CommitInput),
                run_kept_jobs
                    .in_set(AfterInputStep::RunKeptJobs)
                    .run_if(has_kept_jobs),
                stop_cancelled_tool_runs
                    .in_set(AfterInputStep::StopCancelledToolRuns)
                    .run_if(has_kept_change),
                (crate::calls::start, crate::background::start)
                    .in_set(
                    AfterInputStep::StartToolRunsAndBackgroundWork,
                ),
                (
                    send_runtimes_what_they_are_owed
                        .run_if(may_owe_runtimes),
                    wake_held_calls,
                )
                    .chain()
                    .in_set(
                        AfterInputStep::SendRuntimesWhatTheyAreOwed,
                    ),
                publish_state
                    .in_set(AfterInputStep::PublishState)
                    .run_if(has_shown_change),
                send_client_messages
                    .in_set(AfterInputStep::SendClientMessages),
                write_runtime_lines
                    .in_set(AfterInputStep::WriteRuntimeLines),
                (push_owed_lodges, reset_for_next_input)
                    .chain()
                    .in_set(AfterInputStep::PushOwedLodges),
            ),
        );
}

/// The schedule the runner runs after each input.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AfterInput;

/// The steps of [`AfterInput`], in their order.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AfterInputStep {
    /// The inputs that ended are followed.
    FollowEndedInputs,
    NarrowToolsToRoles,
    /// The changes of the steps before are kept.
    CommitInput,
    /// The jobs the kept changes left are run.
    RunKeptJobs,
    StopCancelledToolRuns,
    /// The tool runs and the tasks the input asked for start.
    StartToolRunsAndBackgroundWork,
    SendRuntimesWhatTheyAreOwed,
    PublishState,
    SendClientMessages,
    /// The runtimes are sent what the input queued for them.
    WriteRuntimeLines,
    /// The lodges the input owes the remote workspace are pushed.
    PushOwedLodges,
}

/// A call held on a wait whose wait was answered: its runtime is
/// told after the input's changes are sent, so asking again may get
/// further.
#[derive(Message)]
pub(crate) struct WakeHeldCall {
    pub runtime: String,
    pub key: String,
}

/// A runtime's input ended: its follow, which may end a worker or
/// carry an answer back, and the replies owed for it come once what
/// said so is kept. Written as a message by the input, it is
/// triggered for the observers by [`AfterInput`].
#[derive(Message, Event, Clone)]
pub(crate) struct InputEnded {
    pub conversation: ConversationId,
    pub request: String,
}

/// The input's changes, left for [`AfterInput`].
#[derive(Resource, Default)]
pub(crate) struct InputEffects {
    /// A change was made, kept with the input.
    pub change_kept: bool,
    /// An agent or a hamlet changed, so its tools may have.
    pub agents_or_hamlets_changed: bool,
    /// Jobs were kept for the runner.
    pub jobs_kept: bool,
    /// A runtime answered a request, so what waited on it may go.
    pub runtime_answered: bool,
    /// The clients' view changed with no change kept, such as the
    /// progress of moving the lodges.
    pub state_changed_outside_change: bool,
    /// The fault point a crash test stops at once the input is kept,
    /// such as a society call's change kept with its answer.
    pub crash_test_fault_point: Option<&'static str>,
}

/// Drops what a failed input left to follow it: its transaction was
/// rolled back, so nothing it changed is so, and nothing it asked
/// for runs.
pub(crate) fn discard(world: &mut World) -> anyhow::Result<()> {
    *world.resource_mut::<InputEffects>() = InputEffects::default();
    world.resource_mut::<Messages<WakeHeldCall>>().clear();
    world.resource_mut::<Messages<InputEnded>>().clear();
    crate::clients::discard(
        &mut world.resource_mut::<Messages<OutgoingClientMessage>>(),
    );
    world.resource_mut::<BackgroundWork>().discard();
    world.run_system_cached(|mut runtimes: Runtimes| {
        runtimes.discard();
    })?;
    let starting = world
        .query_filtered::<Entity, With<UnstartedToolRun>>()
        .iter(world)
        .collect::<Vec<_>>();
    for run in starting {
        world.despawn(run);
    }
    Ok(())
}

fn has_kept_change(effects: Res<InputEffects>) -> bool {
    effects.change_kept
}

fn has_kept_jobs(effects: Res<InputEffects>) -> bool {
    effects.jobs_kept
}

/// Run condition for a change the clients may see.
fn has_shown_change(effects: Res<InputEffects>) -> bool {
    effects.change_kept || effects.state_changed_outside_change
}

fn has_agents_or_hamlets_changed(effects: Res<InputEffects>) -> bool {
    effects.agents_or_hamlets_changed
}

/// Run condition for something new a runtime may be owed.
fn may_owe_runtimes(effects: Res<InputEffects>) -> bool {
    effects.change_kept || effects.runtime_answered
}

/// Triggers each input that ended for its observers.
fn trigger_ended_inputs(
    mut ended: MessageReader<InputEnded>,
    mut commands: Commands,
) {
    for input in ended.read() {
        commands.trigger(input.clone());
    }
}

/// Follows an input that ended, and wakes what its follow freed.
fn follow_ended_input(
    ended: On<InputEnded>,
    mut core: Core,
) -> Result {
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
fn send_owed_replies(
    ended: On<InputEnded>,
    mut connections: Connections,
) {
    for (client, id) in
        connections.settle(ended.conversation, &ended.request)
    {
        let answer =
            check::reply(&id, Ok::<_, Refusal>(String::new()));
        connections.send(client, answer);
    }
}

/// Narrows every living agent's tools to its role.
pub(crate) fn narrow_tools_to_roles(
    mut realm: ResMut<Realm>,
    catalogue: Res<Catalogue>,
) -> Result {
    narrow_all_tools_to_roles(
        realm.store().connection(),
        &catalogue,
    )?;
    Ok(())
}

/// Narrows the tools of every living agent's conversations to its
/// role now; a runtime is told before the next input.
pub(crate) fn narrow_all_tools_to_roles(
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
fn commit_input(mut realm: ResMut<Realm>) -> Result {
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
fn run_kept_jobs(
    mut realm: ResMut<Realm>,
    writing: Res<RepositoryWriteLock>,
    runner: Res<Runner>,
    mut remote: RemoteWorkspace,
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
fn stop_cancelled_tool_runs(
    mut realm: ResMut<Realm>,
    runs: ToolRuns,
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
fn send_runtimes_what_they_are_owed(
    mut realm: ResMut<Realm>,
    env: Res<Env>,
    mut runtimes: Runtimes,
    providers: Res<Providers>,
    mut fitting: ImageFitting,
) -> Result {
    let images = realm.layout().images();
    let all = runtimes.all();
    let mut owed = sync::RuntimeDues {
        db: realm.store().connection(),
        env: &env,
        runtimes: &mut runtimes,
        providers: &providers,
        fitting: &mut fitting,
        images: &images,
    };
    for runtime in all {
        sync::flush(&mut owed, runtime, Resend::Nothing)?;
    }
    Ok(())
}

fn publish_state(mut core: Core) -> Result {
    core.send_state()?;
    Ok(())
}

/// Sends the clients what the input queued for them, after the state
/// it left.
fn send_client_messages(
    mut connections: Connections,
    credentials: Res<Credentials>,
) {
    connections.deliver(&credentials);
}

/// Tells each runtime of the held calls whose waits were answered.
fn wake_held_calls(
    mut wakes: MessageReader<WakeHeldCall>,
    mut runtimes: Runtimes,
) {
    for wake in wakes.read() {
        runtimes.answered(&wake.runtime, &wake.key);
    }
}

/// Writes each runtime the lines queued for it.
fn write_runtime_lines(mut runtimes: Runtimes) {
    runtimes.deliver();
}

/// Starts pushing the lodges the input owes the remote workspace.
fn push_owed_lodges(
    mut realm: ResMut<Realm>,
    writing: Res<RepositoryWriteLock>,
    credentials: Res<Credentials>,
    mut remote: RemoteWorkspace,
) -> Result {
    remote.push_owed(&mut realm, &writing, &credentials.secrets())?;
    Ok(())
}

/// Clears the followed input's state so the next starts afresh, and
/// turns the messages over, which no frame does here.
fn reset_for_next_input(
    mut effects: ResMut<InputEffects>,
    mut wakes: ResMut<Messages<WakeHeldCall>>,
    mut ended: ResMut<Messages<InputEnded>>,
) {
    *effects = InputEffects::default();
    wakes.update();
    ended.update();
}
