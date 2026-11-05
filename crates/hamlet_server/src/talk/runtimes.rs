//! The harness's runtimes: started, taken on or refused at their
//! hello, started again after a failure (S22, S24), and the requests
//! in flight to each.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
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
use crate::layout::Layout;

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

pub(crate) struct Runtime {
    pub id: String,
    pub harness: String,
    pub phase: Phase,
    process: Option<Process>,
    generation: u64,
    failures: u32,
    ready_at: Option<Instant>,
    again_at: Option<Instant>,
}

/// What a request to a runtime was for, until its reply.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pending {
    Create(ConversationId),
    Submit(ConversationId, String),
    Cancel(ConversationId),
    /// The tools a conversation offers, as told.
    Configure(ConversationId, String),
    History(ConversationId),
}

pub(crate) struct Runtimes {
    pi: Pi,
    pub list: Vec<Runtime>,
    next: u64,
    pending: HashMap<String, (usize, Pending)>,
    lines: UnboundedSender<Received>,
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

impl Runtimes {
    pub fn new(pi: Pi, lines: UnboundedSender<Received>) -> Self {
        Self {
            pi,
            list: Vec::new(),
            next: 0,
            pending: HashMap::new(),
            lines,
        }
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.list.iter().position(|runtime| runtime.id == id)
    }

    /// Starts every runtime the realm keeps that is not running yet.
    pub fn start(
        &mut self,
        db: &mut Db,
        layout: &Layout,
    ) -> Result<()> {
        let kept = runtime::table
            .order(runtime::id)
            .select((runtime::id, runtime::harness, runtime::begun))
            .load::<(String, String, bool)>(db)?;
        for (id, harness, begun) in kept {
            let index = match self.index_of(&id) {
                Some(index) => index,
                None => {
                    self.list.push(Runtime {
                        id,
                        harness,
                        phase: Phase::Down,
                        process: None,
                        generation: 0,
                        failures: 0,
                        ready_at: None,
                        again_at: None,
                    });
                    self.list.len() - 1
                }
            };
            if self.list[index].phase == Phase::Down
                && self.list[index]
                    .again_at
                    .is_none_or(|at| at <= Instant::now())
            {
                self.launch(index, layout, !begun)?;
            }
        }
        Ok(())
    }

    fn launch(
        &mut self,
        index: usize,
        layout: &Layout,
        fresh: bool,
    ) -> Result<()> {
        let runtime = &mut self.list[index];
        runtime.generation += 1;
        let folder = layout.harnesses().join(&runtime.id);
        std::fs::create_dir_all(&folder)?;
        let store = Pi::store(&folder);
        let process = Process::start(
            &Launch {
                pi: &self.pi,
                id: &runtime.id,
                store: &store,
                fresh,
            },
            index,
            runtime.generation,
            self.lines.clone(),
        )?;
        runtime.process = Some(process);
        runtime.phase = Phase::Starting;
        Ok(())
    }

    /// How many times runtime `index` was started, which tells its
    /// processes apart.
    pub fn generation(&self, index: usize) -> Option<u64> {
        self.list.get(index).map(|runtime| runtime.generation)
    }

    /// Whether `received` came from the process running now.
    pub fn is_current(&self, received: &Received) -> bool {
        self.list
            .get(received.runtime)
            .is_some_and(|r| r.generation == received.generation)
    }

    /// Checks the hello of runtime `index` against what it began
    /// with, keeping its versions the first time. Returns the words
    /// of its refusal.
    pub fn hello(
        &mut self,
        db: &mut Db,
        index: usize,
        hello: &Hello,
    ) -> Result<Option<String>> {
        let id = self.list[index].id.clone();
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
                self.list[index].phase =
                    Phase::Refused(words.clone());
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
                let runtime = &mut self.list[index];
                runtime.phase = Phase::Ready;
                runtime.ready_at = Some(Instant::now());
            }
        }
        Ok(refusal)
    }

    /// The adapter and harness versions runtime `index` began with.
    pub fn versions(
        &self,
        db: &mut Db,
        index: usize,
    ) -> Result<(String, String)> {
        let (adapter, version) = runtime::table
            .find(&self.list[index].id)
            .select((runtime::adapter, runtime::version))
            .first::<(Option<String>, Option<String>)>(db)?;
        Ok((adapter.unwrap_or_default(), version.unwrap_or_default()))
    }

    /// Runtime `index` stopped: its requests in flight are dropped,
    /// and it starts again after a wait that grows with each failure.
    pub fn failed(&mut self, index: usize) {
        self.pending.retain(|_, (at, _)| *at != index);
        let runtime = &mut self.list[index];
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

    /// The soonest a failed runtime starts again.
    pub fn next_start(&self) -> Option<Instant> {
        self.list
            .iter()
            .filter(|r| r.phase == Phase::Down)
            .filter_map(|r| r.again_at)
            .min()
    }

    /// Sends the message `make` builds from the version and a new id,
    /// returning the id.
    pub async fn send(
        &mut self,
        index: usize,
        make: impl FnOnce(Version, String) -> ServerMessage,
    ) -> String {
        self.next += 1;
        let id = format!("s{}", self.next);
        let message = make(VERSION, id.clone());
        let line = serde_json::to_string(&message)
            .expect("a server message is JSON");
        if let Some(process) = self.list[index].process.as_mut() {
            process.write(&line).await;
        }
        id
    }

    /// Sends a request whose reply is awaited as `pending`.
    pub async fn request(
        &mut self,
        index: usize,
        pending: Pending,
        make: impl FnOnce(Version, String) -> ServerMessage,
    ) -> String {
        let id = self.send(index, make).await;
        self.pending.insert(id.clone(), (index, pending));
        id
    }

    /// The request `re` answers.
    pub fn reply(&mut self, re: &str) -> Option<(usize, Pending)> {
        self.pending.remove(re)
    }

    pub fn is_in_flight(
        &self,
        index: usize,
        pending: &Pending,
    ) -> bool {
        self.pending
            .values()
            .any(|(at, kept)| *at == index && kept == pending)
    }

    /// Stops every runtime, giving each `grace` to finish.
    pub async fn stop(&mut self, grace: Duration) {
        for runtime in &mut self.list {
            if let Some(process) = runtime.process.take() {
                process.stop(grace).await;
            }
            runtime.phase = Phase::Down;
        }
    }
}
