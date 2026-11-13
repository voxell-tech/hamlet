//! The harness's runtimes, one entity each: started, taken on or
//! refused at their hello, started again after a failure (S22, S24),
//! and the requests in flight to each.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
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
use tokio::sync::mpsc::UnboundedSender;

use super::process::{Launch, Pi, Process, Received};
use crate::deadline::AddDeadline;
use crate::layout::Layout;
use crate::realm::Realm;

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_deadline(restart_due, start);
}

/// The soonest a failed runtime starts again.
fn restart_due(runtimes: Query<&Runtime>) -> Option<Instant> {
    runtimes
        .iter()
        .filter(|r| r.phase == Phase::Down)
        .filter_map(|r| r.again_at)
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

/// What every runtime must promise.
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
/// How long a runtime runs before its failures are forgotten.
const SETTLED: Duration = Duration::from_secs(60);

/// Where a runtime is in its life.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Phase {
    /// Started, its hello not heard yet.
    Starting,
    Ready,
    /// Failed; it starts again once its wait is over.
    Down,
    /// Refused for good, with words for a chief (S24).
    Refused(String),
}

/// A runtime of the harness, an entity from its first start.
#[derive(Component)]
#[require(InFlight)]
pub(crate) struct Runtime {
    pub id: String,
    pub phase: Phase,
    process: Option<Process>,
    generation: u64,
    failures: u32,
    ready_at: Option<Instant>,
    again_at: Option<Instant>,
}

/// The requests in flight to a runtime, by id, until their replies.
/// A component of the runtime, not entities of their own: the input
/// that sends one sees it at once, where a spawned entity would wait
/// for the input's end.
#[derive(Component, Default)]
pub(crate) struct InFlight(HashMap<String, Pending>);

/// What a request to a runtime was for, until its reply.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pending {
    Create(ConversationId),
    Submit(ConversationId, String),
    Cancel(ConversationId),
    /// The tools a conversation offers, as told.
    Configure(ConversationId, String),
}

/// How the runtimes are started, and the lines sent to them.
#[derive(Resource)]
pub(crate) struct Harness {
    pi: Pi,
    next: u64,
    lines: UnboundedSender<Received>,
    /// The lines sent since the last delivery, by runtime and id:
    /// they go out once the input that sent them is kept.
    outbox: Vec<(Entity, String, String)>,
}

impl Harness {
    pub fn new(pi: Pi, lines: UnboundedSender<Received>) -> Self {
        Self {
            pi,
            next: 0,
            lines,
            outbox: Vec::new(),
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
        (Entity, &'static mut Runtime, &'static mut InFlight),
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

    /// Whether `runtime` was taken on and has not failed since.
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
                    generation: 0,
                    failures: 0,
                    ready_at: None,
                    again_at: None,
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
                    .again_at
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

    /// How many times `runtime` was started, which tells its
    /// processes apart.
    pub fn generation(&self, runtime: Entity) -> Option<u64> {
        self.get(runtime).ok().map(|runtime| runtime.generation)
    }

    /// Whether `received` came from the process running now.
    pub fn is_current(&self, received: &Received) -> bool {
        self.generation(received.runtime) == Some(received.generation)
    }

    /// Checks the hello of `runtime` against what it began with,
    /// keeping its versions the first time. Returns the words of its
    /// refusal.
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
        let refusal = if !hello
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
        match &refusal {
            Some(words) => {
                found.phase = Phase::Refused(words.clone());
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
        Ok(refusal)
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

    /// `runtime` stopped: its requests in flight are dropped, and it
    /// starts again after a wait that grows with each failure.
    pub fn failed(&mut self, runtime: Entity) {
        let Ok((_, mut runtime, mut in_flight)) =
            self.list.get_mut(runtime)
        else {
            return;
        };
        in_flight.0.clear();
        runtime.process = None;
        if matches!(runtime.phase, Phase::Refused(_)) {
            return;
        }
        if runtime.ready_at.is_some_and(|at| at.elapsed() >= SETTLED)
        {
            runtime.failures = 0;
        }
        let wait = FIRST_WAIT
            .saturating_mul(1 << runtime.failures.min(16))
            .min(MOST_WAIT);
        runtime.failures += 1;
        runtime.phase = Phase::Down;
        runtime.ready_at = None;
        runtime.again_at = Some(Instant::now() + wait);
    }

    /// Sends the message `make` builds from the version and a new id,
    /// once the input is kept; returns the id.
    pub fn send(
        &mut self,
        runtime: Entity,
        make: impl FnOnce(Version, String) -> ServerMessage,
    ) -> String {
        self.harness.next += 1;
        let id = format!("s{}", self.harness.next);
        let message = make(VERSION, id.clone());
        let line = serde_json::to_string(&message)
            .expect("a server message is JSON");
        self.harness.outbox.push((runtime, id.clone(), line));
        id
    }

    /// Writes each runtime the lines sent to it.
    pub fn deliver(&mut self) {
        for (runtime, _, line) in
            std::mem::take(&mut self.harness.outbox)
        {
            if let Ok((_, mut runtime, _)) =
                self.list.get_mut(runtime)
                && let Some(process) = runtime.process.as_mut()
            {
                process.write(line);
            }
        }
    }

    /// Drops the lines of an input that failed, and the requests
    /// among them, which nothing will answer.
    pub fn discard(&mut self) {
        for (runtime, id, _) in
            std::mem::take(&mut self.harness.outbox)
        {
            if let Ok((.., mut in_flight)) =
                self.list.get_mut(runtime)
            {
                in_flight.0.remove(&id);
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
        let id = self.send(runtime, make);
        if let Ok((.., mut in_flight)) = self.list.get_mut(runtime) {
            in_flight.0.insert(id.clone(), pending);
        }
        id
    }

    /// The request `re` answers, and the runtime it went to.
    pub fn reply(&mut self, re: &str) -> Option<(Entity, Pending)> {
        self.list.iter_mut().find_map(|(entity, _, mut in_flight)| {
            in_flight.0.remove(re).map(|pending| (entity, pending))
        })
    }

    pub fn is_in_flight(
        &self,
        runtime: Entity,
        pending: &Pending,
    ) -> bool {
        self.list.get(runtime).is_ok_and(|(.., in_flight)| {
            in_flight.0.values().any(|kept| kept == pending)
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
    runtime.generation += 1;
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
        runtime.generation,
        harness.lines.clone(),
    )?;
    runtime.process = Some(process);
    runtime.phase = Phase::Starting;
    Ok(())
}
