//! Sessions as entities: each has a key, a transcript and a status,
//! and a worker that owns its agent. Channels send a `UserMessage`
//! and observe `SessionOutput`.

mod deliver;
mod gate;
mod key;
mod mail;
mod output;
mod rewind;
mod told;
mod transcript;
mod usage;
mod worker;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use bevy_app::{App, AppExit, Last, Plugin, Update};
use bevy_ecs::prelude::*;
use hamlet_home::Home;
use hamlet_model::log;
use hamlet_rules::commands::Ruling;
use hamlet_rules::hamlet::{HOME, Hamlet};
use hamlet_rules::realm::Realm;
use tokio::sync::mpsc::unbounded_channel;

use self::deliver::deliver;
pub(crate) use self::gate::{ToolStarting, rule};
pub use self::key::{ASKED, MAIL, Seat, SessionKey};
use self::mail::{Letter, Mail, post, reply_head};
pub use self::mail::{gather, reply_in};
use self::output::output;
pub use self::rewind::{Rewind, Rewound};
pub use self::told::prompt_of;
pub use self::transcript::{Began, Entry, Item, Transcript};
pub use self::usage::Usage;
use self::worker::{Inbox, Jobs, Msg, Workers};
use crate::agent::Event as AgentEvent;
use crate::agents::{self, Roster, Synced};
use crate::core::{self, HomeDir};

/// One thing a session produces while it works.
#[derive(Clone, Debug)]
pub enum Reply {
    Event(AgentEvent),
    Notice(String),
    /// The final text of a turn.
    Done(String),
    Error(String),
}

/// The agent a conversation is with.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[relationship(relationship_target = Conversations)]
pub struct TalksTo(pub Entity);

/// The conversations with an agent.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = TalksTo)]
pub struct Conversations(Vec<Entity>);

/// A session's queue and its latest activity.
#[derive(Component, Default)]
pub struct Status {
    /// Messages queued or running.
    pub pending: usize,
    pub last_activity: Option<(Instant, String)>,
}

impl Status {
    /// True when a message is queued or running.
    pub fn working(&self) -> bool {
        self.pending > 0
    }
}

/// A line from a user on some channel, which may be a command.
#[derive(Event, Clone)]
pub struct UserMessage {
    pub key: SessionKey,
    pub user: String,
    pub text: String,
}

/// One thing a session emits, for channels to render.
#[derive(Event)]
pub struct SessionOutput {
    pub session: Entity,
    pub key: SessionKey,
    pub reply: Reply,
}

/// A line a human typed to a session. An observer that deals with it
/// sets `reply` to the words the human is shown, and the line then
/// goes no further.
#[derive(Event)]
pub(crate) struct Typed {
    pub key: SessionKey,
    pub user: String,
    pub text: String,
    /// The command the line is, with what follows its name.
    pub ruling: Option<(Ruling, String)>,
    pub reply: Option<String>,
}

/// A chief gave an agent an instruction. An observer may set
/// `instruction` to an entity that stands for it while it is carried
/// out.
#[derive(Event)]
pub(crate) struct Instructed {
    pub session: Entity,
    /// The agent instructed; the mayor goes by `MAYOR`.
    pub agent: String,
    pub user: String,
    pub text: String,
    pub instruction: Option<Entity>,
}

/// A chief's instruction was given up before its turn began.
#[derive(Event)]
pub(crate) struct TurnDropped {
    /// The entity `Instructed` was given for it.
    pub instruction: Entity,
}

/// A session began a turn.
#[derive(Event)]
pub(crate) struct TurnStarted {
    pub session: Entity,
    /// The entity `Instructed` was given for it, if it is a chief's
    /// instruction.
    pub instruction: Option<Entity>,
    /// The task its spend is counted under.
    pub task: String,
}

/// One queued message has been fully handled, including any
/// reflection after it.
#[derive(Event)]
pub struct JobFinished {
    pub session: Entity,
}

/// Cuts off the running turn of a session and drops the turns
/// queued behind it.
#[derive(Event, Clone)]
pub(crate) struct Stop {
    pub session: Entity,
    pub reason: String,
}

/// The app is about to stop its sessions.
#[derive(Event)]
pub(crate) struct Closing;

/// Present when something answers `ToolStarting` for a chief, so
/// agents may ask a chief questions and argue with instructions.
#[derive(Resource)]
pub(crate) struct Overseen;

/// Sessions, their workers and the letters between agents.
pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = unbounded_channel();
        app.insert_resource(Inbox { tx, rx })
            .init_resource::<Workers>()
            .add_observer(
                |msg: On<UserMessage>, mut commands: Commands| {
                    let msg = msg.event().clone();
                    commands.queue(move |world: &mut World| {
                        deliver(world, msg, None)
                    });
                },
            )
            .add_observer(
                |letter: On<Letter>, mut commands: Commands| {
                    let letter = letter.event().clone();
                    commands.queue(move |world: &mut World| {
                        post(world, letter)
                    });
                },
            )
            .add_observer(
                |rewind: On<Rewind>, mut commands: Commands| {
                    let rewind = rewind.event().clone();
                    commands.queue(move |world: &mut World| {
                        rewind::rewind(world, rewind)
                    });
                },
            )
            .add_observer(|stop: On<Stop>, jobs: Query<&Jobs>| {
                if let Ok(jobs) = jobs.get(stop.event().session) {
                    let reason = stop.event().reason.clone();
                    let _ = jobs.stops.send((reason, jobs.last));
                }
            })
            .add_observer(relate)
            // A turn may have put a skill in the library.
            .add_observer(
                |_: On<JobFinished>, mut commands: Commands| {
                    commands.queue(core::reshelve)
                },
            )
            .add_systems(
                Update,
                drain
                    .run_if(|inbox: Res<Inbox>| !inbox.rx.is_empty())
                    .before(agents::watch),
            )
            .add_systems(Last, stop_on_exit);
    }
}

fn find(world: &mut World, key: &SessionKey) -> Option<Entity> {
    world
        .query::<(Entity, &SessionKey)>()
        .iter(world)
        .find(|(_, k)| *k == key)
        .map(|(e, _)| e)
}

fn realm(world: &World) -> Realm {
    Realm::new(world.resource::<HomeDir>().0.root.clone())
}

/// The agent the session reaches now, its house, and the log of the
/// session in it.
fn place(
    world: &World,
    key: &SessionKey,
) -> Result<(Seat, Home, PathBuf)> {
    let seat = key.seat(&realm(world))?;
    let house = house_of(world, &seat)?;
    let path = key.log_path(&house.sessions());
    Ok((seat, house, path))
}

/// The house of whoever sits at `seat`.
fn house_of(world: &World, seat: &Seat) -> Result<Home> {
    let home = &world.resource::<HomeDir>().0;
    let hamlet = Hamlet::at(&home.root, &seat.hamlet);
    // The first mayor's house is the home the app opened.
    match hamlet.house_dir(&seat.name) {
        None if seat.hamlet == HOME => Ok(home.clone()),
        _ => hamlet.house(&seat.name),
    }
}

/// The hamlet the agent of `key` lives in. The first when nobody is
/// there.
fn hamlet_of(world: &World, key: &SessionKey) -> Hamlet {
    let root = &world.resource::<HomeDir>().0.root;
    match key.seat(&realm(world)) {
        Ok(seat) => Hamlet::at(root, &seat.hamlet),
        Err(_) => Hamlet::new(root.clone()),
    }
}

/// The session's entity, created with its history from the log if it
/// isn't loaded yet.
pub fn ensure(world: &mut World, key: &SessionKey) -> Entity {
    if let Some(e) = find(world, key) {
        return e;
    }
    let session = world.spawn((key.clone(), Status::default())).id();
    // Nobody may be there yet; a message to it says so.
    let _ = reseat(world, session, key);
    let with = agents::ask(world, |roster| agent_of(roster, key));
    if let Some(agent) = with.flatten() {
        world.entity_mut(session).insert(TalksTo(agent));
    }
    session
}

/// Points a session at whoever its key reaches now, with that
/// agent's own history. Returns the log it writes to.
fn reseat(
    world: &mut World,
    session: Entity,
    key: &SessionKey,
) -> Result<PathBuf> {
    let placed = place(world, key);
    let events = match &placed {
        Ok((_, _, path)) => {
            log::read_events(path).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    };
    let usage = Usage::from_log(&events);
    let said = Transcript::from_log(&log::standing(events));
    let began = match &placed {
        Ok((_, _, path)) => Began::of(path, &said),
        Err(_) => Began::default(),
    };
    let mut entity = world.entity_mut(session);
    entity.remove::<(Jobs, Seat)>().insert((said, usage, began));
    let (seat, _, path) = placed?;
    entity.insert(seat);
    Ok(path)
}

/// The agent a session is with: the mayor of its hamlet for an
/// office, the living agent of its name otherwise.
fn agent_of(roster: &Roster, key: &SessionKey) -> Option<Entity> {
    match (key.agent(), key.office()) {
        (Some(name), _) => roster.named(name),
        (None, Some(hamlet)) => roster.mayor_of(hamlet),
        (None, None) => None,
    }
}

/// Points every conversation at the agent it is with now.
fn relate(
    _: On<Synced>,
    roster: Roster,
    sessions: Query<(Entity, &SessionKey, Option<&TalksTo>)>,
    mut commands: Commands,
) {
    for (session, key, with) in &sessions {
        let agent = agent_of(&roster, key);
        match agent {
            _ if agent == with.map(|with| with.0) => {}
            Some(agent) => {
                commands.entity(session).insert(TalksTo(agent));
            }
            None => {
                commands.entity(session).remove::<TalksTo>();
            }
        }
    }
}

/// Opens the sessions with the agent `name` afresh, after its house
/// moved: their workers stop and their transcripts are read again.
fn reopen(world: &mut World, name: &str) {
    let sessions = world
        .query::<(Entity, &SessionKey)>()
        .iter(world)
        .filter(|(_, key)| key.agent() == Some(name))
        .map(|(entity, key)| (entity, key.clone()))
        .collect::<Vec<_>>();
    for (session, key) in sessions {
        if let Err(gone) = reseat(world, session, &key)
            && let Some(mut transcript) =
                world.get_mut::<Transcript>(session)
        {
            transcript.push(Entry::Notice(format!("{gone:#}")));
        }
    }
}

/// The log a session writes to. `None` if nobody is there.
pub fn log_of(world: &World, key: &SessionKey) -> Option<PathBuf> {
    place(world, key).ok().map(|(_, _, path)| path)
}

/// Applies what the workers sent since the last tick.
fn drain(world: &mut World) {
    while let Ok(msg) = world.resource_mut::<Inbox>().rx.try_recv() {
        match msg {
            Msg::Output { session, by, reply } => {
                // A worker the session has moved on from still
                // finishes, in its own house's log.
                let current = world
                    .get::<Seat>(session)
                    .is_none_or(|seat| seat.name == by);
                if current {
                    output(world, session, reply)
                }
            }
            Msg::Gate(starting) => rule(world, starting),
            Msg::Started {
                session,
                instruction,
                task,
            } => world.trigger(TurnStarted {
                session,
                instruction,
                task,
            }),
            Msg::Archived { session, resumed } => {
                let key = world.get::<SessionKey>(session).cloned();
                let said = match key {
                    // The transcript is read from the resumed log.
                    Some(key) if resumed => {
                        let _ = reseat(world, session, &key);
                        "Went back to an earlier session."
                    }
                    _ => {
                        if let Ok(mut entity) =
                            world.get_entity_mut(session)
                        {
                            entity.remove::<Jobs>();
                            // From now, whether or not there was
                            // anything to put away.
                            entity.insert((
                                Transcript::default(),
                                Usage::default(),
                                Began(log::now_millis()),
                            ));
                        }
                        "Started a new session."
                    }
                };
                output(world, session, Reply::Notice(said.into()));
            }
            Msg::Answer {
                to,
                from,
                text,
                task,
            } => {
                // A reply is a message of its task (H12). It is
                // delivered whatever the count, so the task can end.
                let _ = hamlet_of(world, &to).carried(
                    &from,
                    to.address(),
                    &task,
                );
                deliver(
                    world,
                    UserMessage {
                        key: to,
                        text: format!("{}{text}", reply_head(&from)),
                        user: from,
                    },
                    Some(Mail {
                        reply_to: None,
                        task,
                    }),
                )
            }
            Msg::Finished { session } => {
                if let Some(mut s) = world.get_mut::<Status>(session)
                {
                    s.pending = s.pending.saturating_sub(1);
                }
                world.trigger(JobFinished { session });
            }
            Msg::Dropped(instruction) => {
                world.trigger(TurnDropped { instruction })
            }
            Msg::Rewound { session, entry } => {
                rewind::rewound(world, session, entry)
            }
        }
        // Observers see the messages in the order the workers sent
        // them, each with the last one's effects in place.
        world.flush();
    }
}

/// Present when an exit lets the workers finish first.
#[derive(Resource)]
pub(crate) struct FinishOnExit;

/// Stops the workers in the tick that asks to exit, since `App::run`
/// does not hand the world back.
fn stop_on_exit(world: &mut World) {
    if world.resource::<Messages<AppExit>>().is_empty()
        || world.resource::<Workers>().0.is_empty()
    {
        return;
    }
    if world.contains_resource::<FinishOnExit>() {
        eprintln!("hamlet: finishing before exit...");
        shutdown(world);
        return;
    }
    world.trigger(Closing);
    // A turn cut off here shows as aborted when its log is next read.
    for worker in
        std::mem::take(&mut world.resource_mut::<Workers>().0)
    {
        worker.abort();
    }
}

/// The wait between two looks at whether the workers have
/// finished, at shutdown.
const SHUTDOWN_POLL: Duration = Duration::from_millis(20);

/// Stops the workers after letting each finish its queue and reflect
/// on unreflected turns. Keeps answering tool gates meanwhile, so a
/// final reflection can still use tools.
pub(crate) fn shutdown(world: &mut World) {
    let sessions: Vec<Entity> = world
        .query_filtered::<Entity, With<Jobs>>()
        .iter(world)
        .collect();
    for s in sessions {
        world.entity_mut(s).remove::<Jobs>();
    }
    let workers =
        std::mem::take(&mut world.resource_mut::<Workers>().0);
    while workers.iter().any(|w| !w.is_finished()) {
        // Nobody will answer what waits for a chief from here on.
        world.trigger(Closing);
        drain(world);
        std::thread::sleep(SHUTDOWN_POLL);
    }
    drain(world);
}

#[cfg(test)]
mod tests;
