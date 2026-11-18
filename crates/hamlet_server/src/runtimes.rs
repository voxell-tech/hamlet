//! The harness runtime processes and what each is sent (S22, S24).

pub(crate) mod handlers;
pub(crate) mod leases;
pub(crate) mod process;
pub(crate) mod reconciliation;

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::relationship::RelationshipTarget;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, ResMut, SystemParam};
use diesel::prelude::*;
use hamlet_protocol::Version;
use hamlet_protocol::runtime::{
    Capability, Hello, HelloStore, ServerMessage, VERSION,
};
use hamlet_store::Db;
use hamlet_store::ids::ConversationId;
use hamlet_store::schema::runtime;

use self::process::{Launch, Pi, Process, Received};
use crate::app::input::InputSender;
use crate::app::timers::AddTimer;
use crate::realm::Realm;
use crate::realm::layout::Layout;

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.init_resource::<leases::Authority>()
        .add_timer(restart_due, start)
        .add_plugins((handlers::plugin, reconciliation::plugin));
}

/// The soonest a failed runtime starts again.
fn restart_due(runtimes: Query<&Runtime>) -> Option<Instant> {
    runtimes
        .iter()
        .filter(|r| r.phase == Phase::Down)
        .filter_map(|r| r.restart_at)
        .min()
}

/// Starts the runtimes the realm keeps that are not running, once a
/// failed one's wait is over.
pub(crate) fn start(
    mut realm: ResMut<Realm>,
    mut runtimes: Runtimes,
) -> Result<()> {
    let layout = realm.layout().clone();
    runtimes.start(realm.store().connection(), &layout)
}

/// The capabilities every runtime must promise.
const NEEDED: [Capability; 6] = [
    Capability::Operations,
    Capability::Attempts,
    Capability::Admission,
    Capability::Cancel,
    Capability::Recovery,
    Capability::Updates,
];

/// The first wait before a failed runtime starts again; it doubles.
const FIRST_WAIT: Duration = Duration::from_millis(250);
const MOST_WAIT: Duration = Duration::from_secs(30);
/// The time a runtime runs before its failures are forgotten.
const SETTLED: Duration = Duration::from_secs(60);

/// A runtime's stage in its life.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Phase {
    /// Started, its hello not heard yet.
    Starting,
    Ready,
    /// Failed; it starts again once its wait is over.
    Down,
    /// Rejected for good, with words for a chief (S24).
    Rejected(String),
}

/// A runtime of the harness, an entity from its first start.
#[derive(Component)]
pub(crate) struct Runtime {
    pub id: String,
    pub phase: Phase,
    process: Option<Process>,
    process_generation: u64,
    consecutive_failures: u32,
    ready_at: Option<Instant>,
    restart_at: Option<Instant>,
}

/// A line to a runtime, until it is written or, for a request,
/// answered.
#[derive(Component)]
pub(crate) struct RuntimeLine {
    /// The count of lines sent before it and with it, which is the
    /// order they are written in.
    send_order: u64,
    line_id: String,
    /// The line's text, until it is written once the input is kept.
    unwritten_text: Option<String>,
    /// What a request's reply answers, until it comes.
    awaited_reply: Option<Pending>,
}

/// The runtime a line goes to.
#[derive(Component)]
#[relationship(relationship_target = RuntimeLines)]
pub(crate) struct SentToRuntime(pub Entity);

/// The lines to a runtime not written or not answered yet, which go
/// with it.
#[derive(Component)]
#[relationship_target(relationship = SentToRuntime, linked_spawn)]
pub(crate) struct RuntimeLines(Vec<Entity>);

/// The purpose of a request to a runtime, kept until its reply.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pending {
    Create(ConversationId),
    Submit(ConversationId, String),
    Cancel(ConversationId),
    /// The tools a conversation offers, as told.
    Configure(ConversationId, String),
}

/// The runtimes' launch settings, and the count of lines sent to
/// them.
#[derive(Resource)]
pub(crate) struct Harness {
    pi: Pi,
    lines_sent: u64,
    /// Where the runtimes' lines go, as inputs.
    inputs: InputSender,
}

impl Harness {
    pub fn new(pi: Pi, inputs: InputSender) -> Self {
        Self {
            pi,
            lines_sent: 0,
            inputs,
        }
    }
}

/// The runtime new conversations of `harness` go to, made once.
pub(crate) fn of_harness(
    db: &mut Db,
    harness: &str,
) -> Result<String> {
    if let Some(id) = runtime::table
        .filter(runtime::harness.eq(harness))
        .order(runtime::id)
        .select(runtime::id)
        .first::<String>(db)
        .optional()?
    {
        return Ok(id);
    }
    let id = format!("{harness}-{}", uuid::Uuid::new_v4().simple());
    diesel::insert_into(runtime::table)
        .values((runtime::id.eq(&id), runtime::harness.eq(harness)))
        .execute(db)?;
    Ok(id)
}

/// The runtimes as the handlers reach them.
#[derive(SystemParam)]
pub(crate) struct Runtimes<'w, 's> {
    harness: ResMut<'w, Harness>,
    list: Query<
        'w,
        's,
        (Entity, &'static mut Runtime, Option<&'static RuntimeLines>),
    >,
    lines: Query<
        'w,
        's,
        (Entity, &'static SentToRuntime, &'static mut RuntimeLine),
    >,
    commands: Commands<'w, 's>,
}

impl Runtimes<'_, '_> {
    /// Every runtime.
    pub fn all(&self) -> Vec<Entity> {
        self.list.iter().map(|(entity, ..)| entity).collect()
    }

    /// The runtime kept as `id`.
    pub fn find(&self, id: &str) -> Option<Entity> {
        self.list
            .iter()
            .find(|(_, runtime, _)| runtime.id == id)
            .map(|(entity, ..)| entity)
    }

    /// The id `runtime` is kept as.
    pub fn id(&self, runtime: Entity) -> Result<String> {
        Ok(self.get(runtime)?.id.clone())
    }

    /// The readiness of `runtime`: taken on and not failed since.
    pub fn is_ready(&self, runtime: Entity) -> bool {
        self.get(runtime).is_ok_and(|r| r.phase == Phase::Ready)
    }

    fn get(&self, runtime: Entity) -> Result<&Runtime> {
        let (_, found, _) =
            self.list.get(runtime).context("the runtime is gone")?;
        Ok(found)
    }

    /// Starts every runtime the realm keeps that is not running yet;
    /// one started for the first time becomes an entity.
    pub fn start(
        &mut self,
        db: &mut Db,
        layout: &Layout,
    ) -> Result<()> {
        let kept = runtime::table
            .order(runtime::id)
            .select((runtime::id, runtime::begun))
            .load::<(String, bool)>(db)?;
        for (id, begun) in kept {
            let Some(entity) = self.find(&id) else {
                let entity = self.commands.spawn_empty().id();
                let mut runtime = Runtime {
                    id,
                    phase: Phase::Down,
                    process: None,
                    process_generation: 0,
                    consecutive_failures: 0,
                    ready_at: None,
                    restart_at: None,
                };
                launch(
                    &self.harness,
                    &mut runtime,
                    entity,
                    layout,
                    !begun,
                )?;
                self.commands.entity(entity).insert(runtime);
                continue;
            };
            let (_, mut runtime, _) = self.list.get_mut(entity)?;
            if runtime.phase == Phase::Down
                && runtime
                    .restart_at
                    .is_none_or(|at| at <= Instant::now())
            {
                launch(
                    &self.harness,
                    &mut runtime,
                    entity,
                    layout,
                    !begun,
                )?;
            }
        }
        Ok(())
    }

    /// The count of times `runtime` was started, which tells its
    /// processes apart.
    pub fn generation(&self, runtime: Entity) -> Option<u64> {
        self.get(runtime)
            .ok()
            .map(|runtime| runtime.process_generation)
    }

    /// The currency of `received`: it came from the process running
    /// now.
    pub fn is_current(&self, received: &Received) -> bool {
        self.generation(received.runtime) == Some(received.generation)
    }

    /// Checks the hello of `runtime` against what it began with,
    /// keeping its versions the first time. Returns the words of its
    /// rejection.
    pub fn hello(
        &mut self,
        db: &mut Db,
        runtime: Entity,
        hello: &Hello,
    ) -> Result<Option<String>> {
        let (_, mut found, _) = self.list.get_mut(runtime)?;
        let id = found.id.clone();
        let (begun, adapter, version) = runtime::table
            .find(&id)
            .select((
                runtime::begun,
                runtime::adapter,
                runtime::version,
            ))
            .first::<(bool, Option<String>, Option<String>)>(db)?;
        let rejection = if !hello
            .versions
            .iter()
            .any(|version| VERSION.is_compatible(version))
        {
            let spoken = hello
                .versions
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!(
                "The runtime {id} speaks protocol versions \
                 {spoken}, none compatible with {VERSION}."
            ))
        } else if hello.runtime.as_deref() != Some(&id) {
            Some(format!(
                "The runtime's store does not belong to {id}."
            ))
        } else if hello.store == HelloStore::Missing {
            Some(format!("The store of the runtime {id} is missing."))
        } else if let Some(missing) =
            NEEDED.iter().find(|c| !hello.capabilities.contains(c))
        {
            Some(format!(
                "The runtime {id} cannot promise {missing:?}, which \
                 Hamlet needs."
            ))
        } else if begun
            && (adapter.as_deref() != Some(&hello.adapter)
                || version.as_deref() != Some(&hello.harness))
        {
            Some(format!(
                "The runtime {id} reports other versions than it \
                 began with."
            ))
        } else {
            None
        };
        match &rejection {
            Some(words) => {
                found.phase = Phase::Rejected(words.clone());
            }
            None => {
                if !begun {
                    diesel::update(runtime::table.find(&id))
                        .set((
                            runtime::begun.eq(true),
                            runtime::adapter.eq(&hello.adapter),
                            runtime::version.eq(&hello.harness),
                        ))
                        .execute(db)?;
                }
                found.phase = Phase::Ready;
                found.ready_at = Some(Instant::now());
            }
        }
        Ok(rejection)
    }

    /// The adapter and harness versions `runtime` began with.
    pub fn versions(
        &self,
        db: &mut Db,
        runtime: Entity,
    ) -> Result<(String, String)> {
        let (adapter, version) = runtime::table
            .find(&self.get(runtime)?.id)
            .select((runtime::adapter, runtime::version))
            .first::<(Option<String>, Option<String>)>(db)?;
        Ok((adapter.unwrap_or_default(), version.unwrap_or_default()))
    }

    /// `runtime` stopped: its lines are dropped, and it starts again
    /// after a wait that grows with each failure.
    pub fn failed(&mut self, entity: Entity) {
        let Ok((_, mut runtime, _)) = self.list.get_mut(entity)
        else {
            return;
        };
        self.commands
            .entity(entity)
            .despawn_related::<RuntimeLines>();
        runtime.process = None;
        if matches!(runtime.phase, Phase::Rejected(_)) {
            return;
        }
        if runtime.ready_at.is_some_and(|at| at.elapsed() >= SETTLED)
        {
            runtime.consecutive_failures = 0;
        }
        let wait = FIRST_WAIT
            .saturating_mul(1 << runtime.consecutive_failures.min(16))
            .min(MOST_WAIT);
        runtime.consecutive_failures += 1;
        runtime.phase = Phase::Down;
        runtime.ready_at = None;
        runtime.restart_at = Some(Instant::now() + wait);
    }

    /// Sends the message `make` builds from the version and a new id,
    /// once the input is kept; returns the id.
    pub fn send(
        &mut self,
        runtime: Entity,
        make: impl FnOnce(Version, String) -> ServerMessage,
    ) -> String {
        self.line(runtime, None, make)
    }

    /// Spawns a line to `runtime` from the message `make` builds,
    /// awaiting a reply as `pending` when it is a request; returns
    /// its id.
    fn line(
        &mut self,
        runtime: Entity,
        pending: Option<Pending>,
        make: impl FnOnce(Version, String) -> ServerMessage,
    ) -> String {
        self.harness.lines_sent += 1;
        let number = self.harness.lines_sent;
        let id = format!("s{number}");
        let message = make(VERSION, id.clone());
        let text = serde_json::to_string(&message)
            .expect("a server message is JSON");
        if self.list.contains(runtime) {
            self.commands.spawn((
                SentToRuntime(runtime),
                RuntimeLine {
                    send_order: number,
                    line_id: id.clone(),
                    unwritten_text: Some(text),
                    awaited_reply: pending,
                },
            ));
        }
        id
    }

    /// Writes each runtime the lines sent to it, in the order they
    /// were sent. A line with no reply to await goes once written,
    /// and one to a runtime with no process goes unwritten.
    pub fn deliver(&mut self) {
        let mut unsent = self
            .lines
            .iter_mut()
            .filter_map(|(line, to, mut found)| {
                let text = found.unwritten_text.take()?;
                let done = found.awaited_reply.is_none();
                Some((found.send_order, line, to.0, text, done))
            })
            .collect::<Vec<_>>();
        unsent.sort_unstable_by_key(|(number, ..)| *number);
        for (_, line, runtime, text, done) in unsent {
            let written = if let Ok((_, mut runtime, _)) =
                self.list.get_mut(runtime)
                && let Some(process) = runtime.process.as_mut()
            {
                process.write(text);
                true
            } else {
                false
            };
            if done || !written {
                self.commands.entity(line).despawn();
            }
        }
    }

    /// Drops the lines of an input that failed, and the requests
    /// among them, which nothing will answer.
    pub fn discard(&mut self) {
        for (line, _, found) in &self.lines {
            if found.unwritten_text.is_some() {
                self.commands.entity(line).despawn();
            }
        }
    }

    /// Tells the runtime kept as `id` that the wait of its call `key`
    /// was answered.
    pub fn answered(&mut self, id: &str, key: &str) {
        let Some(runtime) = self.find(id) else {
            return;
        };
        let key = key.to_owned();
        self.send(runtime, |v, id| {
            ServerMessage::Answered(
                hamlet_protocol::runtime::Answered { id, key, v },
            )
        });
    }

    /// Sends `runtime` a request whose reply is awaited as `pending`.
    pub fn request(
        &mut self,
        runtime: Entity,
        pending: Pending,
        make: impl FnOnce(Version, String) -> ServerMessage,
    ) -> String {
        self.line(runtime, Some(pending), make)
    }

    /// The request that `request_id` answers, and the runtime it went
    /// to. The request is answered at once, and its line goes.
    pub fn reply(
        &mut self,
        request_id: &str,
    ) -> Option<(Entity, Pending)> {
        let (line, to, mut found) =
            self.lines.iter_mut().find(|(.., found)| {
                found.line_id == request_id
                    && found.awaited_reply.is_some()
            })?;
        let pending = found.awaited_reply.take()?;
        self.commands.entity(line).despawn();
        Some((to.0, pending))
    }

    /// The inputs sent to a runtime whose admission was not heard.
    pub fn submitting(&self) -> Vec<(ConversationId, String)> {
        self.lines
            .iter()
            .filter_map(|(.., line)| match &line.awaited_reply {
                Some(Pending::Submit(conversation, request)) => {
                    Some((*conversation, request.clone()))
                }
                _ => None,
            })
            .collect()
    }

    pub fn is_in_flight(
        &self,
        runtime: Entity,
        pending: &Pending,
    ) -> bool {
        let Ok((.., Some(lines))) = self.list.get(runtime) else {
            return false;
        };
        lines.iter().any(|line| {
            self.lines.get(line).is_ok_and(|(.., found)| {
                found.awaited_reply.as_ref() == Some(pending)
            })
        })
    }

    /// Takes every runtime's process, to stop, leaving each down.
    pub fn take_processes(&mut self) -> Vec<Process> {
        self.list
            .iter_mut()
            .filter_map(|(_, mut runtime, _)| {
                runtime.phase = Phase::Down;
                runtime.process.take()
            })
            .collect()
    }
}

/// Starts `runtime`'s process, whose lines come tagged with `entity`.
fn launch(
    harness: &Harness,
    runtime: &mut Runtime,
    entity: Entity,
    layout: &Layout,
    fresh: bool,
) -> Result<()> {
    runtime.process_generation += 1;
    let folder = layout.harnesses().join(&runtime.id);
    std::fs::create_dir_all(&folder)?;
    let store = Pi::store(&folder);
    let process = Process::start(
        &Launch {
            pi: &harness.pi,
            id: &runtime.id,
            store: &store,
            fresh,
        },
        entity,
        runtime.process_generation,
        harness.inputs.clone(),
    )?;
    runtime.process = Some(process);
    runtime.phase = Phase::Starting;
    Ok(())
}
