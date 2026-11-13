//! Runs actors in this process. Each actor is one plain SQLite file
//! at `actors/<kind>/<key>.sqlite` under the realm folder. The host
//! finds an actor by its address, starts it, calls it, and stops it.
//!
//! The file's `_hamlet_actor` row marks the actor running from its
//! start until a clean stop, and holds the deadline that stop left:
//! the earliest of the actor's schedule rows, its run wake deadline
//! and the last alarm its core pushed, read once the core has
//! stopped. It also keeps the input a create gave the actor, which
//! each start gives its core. A running actor's alarms live in its
//! core alone, and the host keeps no timer for it. A stopped actor
//! with a deadline has one timer, which starts it from its own file.
//! Boot reads the files of the host's own kinds, begins the start of
//! each actor a kill left marked running or whose deadline has come,
//! without waiting for it, and arms a timer for each other stored
//! deadline. A host locks the folder by its name, so hosts of
//! different names, each with kinds of its own, share one folder.
//!
//! A destroy is stored in the row before the actor's files are
//! removed, so the next start of the actor, a boot's included,
//! finishes a destroy that a kill cut off, and does not start it.
//!
//! Each actor's place in its lifecycle is one [`Life`] in the host's
//! map. One actor's start or stop waits only on that actor's work. A
//! start and a stop each have a deadline; an actor that does not stop
//! by its deadline is given up, and the host starts it no more. A
//! shutdown stops every actor at once, ends the starts under way
//! after a short grace, and the host starts none after it, nor after
//! its last [`Host`] is dropped.

use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use hamlet_actor_core::actor::schedule::{
    RUN_WAKE_AT_META_KEY, SetAsideTimer, decode_run_wake_at,
};
use hamlet_actor_core::error::ActorRuntime;
use hamlet_actor_core::host::{
    ActorHost, RunningActor, StartRequest,
};
use hamlet_actor_core::sqlite::{
    BindParam, ExecuteResult, QueryResult, SqliteBatchStatement,
};
use hamlet_actor_core::{
    ActorFactory, ActorKeySegment, ActorLifecycle, RuntimeBoxFuture,
    ShutdownKind,
};
use hamlet_actor_error::ActorError;
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::{broadcast, oneshot, watch};
use tokio::task::AbortHandle;

pub use self::access::{HostRef, Values};
pub use self::call::Caller;
use self::create::DuplicateKey;
use crate::directory::{Address, Directory, Place};
use crate::event::HostEvent;
use crate::plain_sqlite::{self, PlainSqlite};

mod access;
mod call;
mod create;

/// The one session a local file has. The transaction code compares
/// sessions to detect a lost connection, which a local file cannot
/// have.
const LOCAL_SESSION: u64 = 1;

/// How long a shutdown lets the starts under way go on before it
/// ends them.
const START_GRACE_AT_SHUTDOWN: Duration = Duration::from_secs(1);

/// How long a stop that missed its deadline waits for the actor's
/// file to close. A file whose thread still runs a statement closes
/// only once the statement ends.
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// The host's row in each actor file. Its version ladder is Hamlet's,
/// apart from the core's `_actor_` tables.
const ACTOR_TABLE: &str = "CREATE TABLE IF NOT EXISTS _hamlet_actor \
                           (
	id INTEGER PRIMARY KEY CHECK (id = 0),
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	generation INTEGER NOT NULL,
	wake_at INTEGER,
	running INTEGER NOT NULL DEFAULT 0,
	destroying INTEGER NOT NULL DEFAULT 0,
	input BLOB)";

/// Where one actor is in its lifecycle. The map holds no entry for an
/// actor that is stopped with no deadline: none of its files is open.
///
/// Every transition takes the map's lock, checks the state, writes
/// the next one and releases the lock. The lock is never held across
/// an await, a file operation or a blocking send. The work a
/// transition begins, a start or a stop, runs on a task of its own,
/// which owns the state through [`Owned`] and makes the closing
/// transition when it ends, however it ends. A caller that waits
/// clones the state's receiver under the lock, awaits it after, and
/// reads the map again once it closes.
///
/// | From | Event | To |
/// |---|---|---|
/// | absent | `get_or_start`, a create or a call, the host not shut | `Starting` |
/// | absent | boot reads its file, the host not shut | `Reading` |
/// | `Reading` | boot found a run a kill cut off, or a deadline that has come | `Starting` |
/// | `Reading` | boot found a deadline to come | `Stopped` |
/// | `Reading` | boot found neither, or could not read the file | absent |
/// | `Stopped` | `get_or_start` or a call, the host not shut | `Starting`, its timer aborted |
/// | `Stopped` | its timer fires with the deadline the state holds | `Starting`, from the file it has |
/// | `Stopped` | `Host::shutdown`, or the last `Host` dropped | absent, its timer aborted |
/// | absent, `Stopped` | `Host::stop(Destroy)`, the host not shut | `Destroying`, with no generation, its timer aborted |
/// | `Starting` | its generation asks to sleep or to be destroyed, or reports a crash | `Starting`, with the request kept; a destroy wins over a sleep |
/// | `Starting` | the start ended well | `Running`, or `Stopping`, or `Destroying` with its file marked, for a kept request |
/// | `Starting` | the start failed or panicked, or missed its deadline | absent, the failure told to its waiters |
/// | `Starting` | a start that may not make the actor found none, or a create's found one | absent, its waiters told nothing |
/// | `Running` | its generation asks to sleep or reports a crash, or `Host::stop(Sleep)` | `Stopping` |
/// | `Running` | its generation asks to be destroyed, or `Host::stop(Destroy)` | `Destroying`, its file marked |
/// | `Stopping` | its generation asks to be destroyed, or `Host::stop(Destroy)` | `Destroying`, its file marked |
/// | `Stopping` | its generation asks to sleep or reports a crash, or `Host::stop(Sleep)` | unchanged |
/// | `Destroying` | any request | unchanged |
/// | `Stopping` | the stop ended with a deadline, the host not shut | `Stopped`, its timer armed |
/// | `Stopping`, `Destroying` | the generation did not stop within the stop deadline | `Stuck`, its file closed unmarked |
/// | `Stopping`, `Destroying` | the stop or the removal ended otherwise, however it ended | absent |
/// | `Stuck` | any request | unchanged |
/// | any | a request from a generation the state does not hold | unchanged |
///
/// Callers wait without a transition of their own. A start, a call's
/// included, waits for a `Reading`, `Starting` or `Stopping` actor
/// and then starts it, and fails at once for a `Destroying` or
/// `Stuck` one. A start that waited for a start that failed, or for a
/// stop that missed its deadline, fails with it, rather than start
/// again; one that waited for a start that found no actor reads the
/// state again, so a `get_or_start` behind a call makes the actor the
/// call could not. A create fails at once with `actor.duplicate_key`
/// for a `Stopped`, `Running` or `Stopping` actor, and waits for a
/// `Reading` or `Starting` one as a start does. `Host::stop` waits
/// for a `Reading` or `Starting` actor and then stops it, and waits
/// for a stop under way, failing with it if it missed its deadline; a
/// sleep leaves a `Stopped` actor as it is, and any stop of a `Stuck`
/// one fails at once. `Host::shutdown`
/// sleeps each actor as `Host::stop` does.
enum Life {
    /// Boot reads its file, to learn whether to start it.
    Reading {
        done: Waiters,
    },
    /// Stopped, with the deadline its last stop left. `timer` starts
    /// it then, if the state still holds that deadline.
    Stopped {
        wake_at: i64,
        timer: AbortHandle,
    },
    /// Opening the file and starting the core. `generation` is the id
    /// of the generation it starts; `request` is a stop that
    /// generation asked for before its start ended.
    Starting {
        generation: u64,
        request: Option<ShutdownKind>,
        ready: Waiters,
    },
    Running(Arc<Generation>),
    /// A sleep, or a crash, of this generation is under way.
    Stopping {
        generation: Arc<Generation>,
        done: Waiters,
    },
    /// This generation, if any, stops, then the actor's files are
    /// removed.
    Destroying {
        generation: Option<Arc<Generation>>,
        done: Waiters,
    },
    /// A generation's stop missed the stop deadline. Its code may
    /// still run. Its file keeps its running mark, and a destroy's
    /// mark if one was stored, and is closed, or closes once a
    /// statement it still runs ends. This host starts the actor no
    /// more, so nothing of it runs beside that code; the next boot
    /// starts it, or finishes its destroy.
    Stuck,
}

/// What the waiters of a `Reading`, `Starting`, `Stopping` or
/// `Destroying` state await: the channel closes when the work that
/// owns the state ends, and holds how a start failed, or that a stop
/// missed its deadline.
type Waiters = watch::Receiver<Option<Failed>>;

/// How a start failed, or a stop missed its deadline, for each of its
/// waiters to return.
#[derive(Clone)]
struct Failed {
    error: ActorError,
    /// The failure with its causes, as the work returned it.
    detail: String,
}

impl Failed {
    fn new(error: &anyhow::Error) -> Self {
        Self {
            error: ActorError::extract(error),
            detail: format!("{error:#}"),
        }
    }

    /// The failure, with the start's code.
    fn to_error(&self) -> anyhow::Error {
        anyhow::Error::from(self.error.clone())
            .context(self.detail.clone())
    }
}

impl Life {
    /// The generation that holds the actor's file: from the end of
    /// its start until its stop closed the file.
    fn loaded(&self) -> Option<&Arc<Generation>> {
        match self {
            Self::Reading { .. }
            | Self::Stopped { .. }
            | Self::Starting { .. }
            | Self::Stuck => None,
            Self::Running(generation)
            | Self::Stopping { generation, .. } => Some(generation),
            Self::Destroying { generation, .. } => {
                generation.as_ref()
            }
        }
    }

    /// Whether the state belongs to the generation of id `id`.
    fn is_generation(&self, id: u64) -> bool {
        match self {
            Self::Starting { generation, .. } => *generation == id,
            _ => self
                .loaded()
                .is_some_and(|generation| generation.id == id),
        }
    }

    /// What a waiter on this state awaits: the receiver closes when
    /// the work that owns the state ends.
    fn waiters(&self) -> Option<&Waiters> {
        match self {
            Self::Stopped { .. } | Self::Running(_) | Self::Stuck => {
                None
            }
            Self::Reading { done: waiters }
            | Self::Starting { ready: waiters, .. }
            | Self::Stopping { done: waiters, .. }
            | Self::Destroying { done: waiters, .. } => Some(waiters),
        }
    }
}

/// One start of an actor, from its start transaction until its stop
/// closed the file.
struct Generation {
    /// No other generation of this host has this id. The file counts
    /// its own generations for the core, and a key destroyed and
    /// made again counts from 1 again, so the host never compares
    /// that count.
    id: u64,
    actor: RunningActor,
    file: PlainSqlite,
    /// The last alarm the core pushed in this generation, `None`
    /// until it pushes one, as the core's own record starts.
    alarm: Arc<Mutex<Option<i64>>>,
}

impl Generation {
    /// Stops the generation's core, then marks its file stopped with
    /// the deadline the generation leaves, and closes the file.
    /// Returns what the mark returned, or `None` if the mark did not
    /// begin within `deadline`. The file then keeps its running mark,
    /// and is closed, so nothing the generation still runs reaches
    /// it, unless its thread still runs a statement after
    /// [`CLOSE_GRACE`]: such a file closes once the statement ends.
    async fn stop(
        &self,
        reason: ShutdownKind,
        deadline: Duration,
    ) -> Option<Result<Option<i64>>> {
        // Taken by the mark as it begins, or by the deadline: the
        // first to take it decides whether the file is marked
        // stopped.
        let taken = Arc::new(AtomicBool::new(false));
        let stop = async {
            self.actor.stop(reason).await;
            // The core has stopped and its alarm pushes are
            // acknowledged, so the generation's last alarm and its
            // file are final.
            let alarm = *self.alarm.lock();
            let taken = taken.clone();
            self.file
                .close_with(move |conn| {
                    if taken.swap(true, Ordering::SeqCst) {
                        return Ok(None);
                    }
                    mark_stopped(conn, alarm).map(Some)
                })
                .await
        };
        let mut stop = std::pin::pin!(stop);
        let marked = match tokio::time::timeout(deadline, &mut stop)
            .await
        {
            Ok(marked) => marked,
            // The mark began in time; the file closes once it ends.
            Err(_) if taken.swap(true, Ordering::SeqCst) => {
                stop.await
            }
            Err(_) => {
                let _ = tokio::time::timeout(
                    CLOSE_GRACE,
                    self.file.close(),
                )
                .await;
                return None;
            }
        };
        marked.transpose()
    }
}

/// The work that owns an actor's `Reading`, `Starting`, `Stopping` or
/// `Destroying` state. When it is dropped, however its work ended, it
/// removes the state from the map if the state is still its own, and
/// then wakes the state's waiters.
struct Owned {
    host: Arc<HostInner>,
    actor_id: String,
    /// Wakes the waiters when it drops, after [`Drop::drop`].
    wake: watch::Sender<Option<Failed>>,
    waiters: Waiters,
}

impl Owned {
    fn new(host: &Arc<HostInner>, actor_id: &str) -> Self {
        let (wake, waiters) = watch::channel(None);
        Self {
            host: host.clone(),
            actor_id: actor_id.to_owned(),
            wake,
            waiters,
        }
    }

    /// Tells the waiters that the start this owns failed, or the stop
    /// it owns missed its deadline, with `error`.
    fn fail(&self, error: &anyhow::Error) {
        self.wake.send_replace(Some(Failed::new(error)));
    }

    fn owns(&self, life: &Life) -> bool {
        life.waiters().is_some_and(|waiters| {
            waiters.same_channel(&self.waiters)
        })
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let removed = {
            let mut actors = self.host.actors.lock();
            actors
                .lives
                .get(&self.actor_id)
                .is_some_and(|life| self.owns(life))
                .then(|| actors.lives.remove(&self.actor_id))
        };
        // Dropped after the lock: it may hold the generation's last
        // reference.
        drop(removed);
    }
}

/// A stop that a transition began, to run on a task of its own.
struct Stop {
    owned: Owned,
    /// `None` for a destroy of an actor no generation holds.
    generation: Option<Arc<Generation>>,
    reason: ShutdownKind,
}

impl Stop {
    /// Stops the generation, if any, marks it stopped in its file
    /// with the deadline it leaves, and closes the file, then removes
    /// the actor's files if it is destroyed by then. A sleep becomes
    /// `Stopped`, or leaves the map, once the file is closed; a
    /// destroy leaves the map when `owned` drops at the end. A sleep
    /// whose file could not be marked stopped fails, its file closed
    /// all the same: the file keeps its running mark, so the next
    /// boot starts it again. A generation that does not stop within
    /// the host's stop deadline leaves the actor `Stuck`, and the
    /// stop fails with `actor.shutdown_timeout`, as do its waiters.
    async fn run(self) -> Result<()> {
        let Self {
            owned,
            generation,
            reason,
        } = self;
        let host = &owned.host;
        let marked = match &generation {
            Some(generation) => {
                generation
                    .stop(reason, host.options.stop_deadline)
                    .await
            }
            None => Some(Ok(None)),
        };
        let Some(marked) = marked else {
            let error = stop_missed(&owned.actor_id, &host.options);
            let replaced = {
                let mut actors = host.actors.lock();
                let owns = actors
                    .lives
                    .get(&owned.actor_id)
                    .is_some_and(|life| owned.owns(life));
                owns.then(|| {
                    actors
                        .lives
                        .insert(owned.actor_id.clone(), Life::Stuck)
                })
            };
            drop(replaced);
            owned.fail(&error);
            return Err(error);
        };
        let wake_at = marked.as_ref().ok().copied().flatten();
        // One lock decides and, for a sleep, leaves `Stopping`, so a
        // destroy asked later finds the actor stopped rather than
        // turning a stop that is past its file removal, and a timer
        // is armed only for an actor that is not destroyed, in a host
        // that has not shut down.
        let (destroyed, replaced) = {
            let mut actors = host.actors.lock();
            let destroying = actors
                .lives
                .get(&owned.actor_id)
                .filter(|life| owned.owns(life))
                .map(|life| matches!(life, Life::Destroying { .. }));
            match (destroying, wake_at) {
                (Some(true), _) => (true, None),
                (Some(false), Some(wake_at)) if !actors.shut => (
                    false,
                    actors.lives.insert(
                        owned.actor_id.clone(),
                        Life::Stopped {
                            wake_at,
                            timer: arm(
                                host,
                                &owned.actor_id,
                                wake_at,
                            ),
                        },
                    ),
                ),
                (Some(false), _) => {
                    (false, actors.lives.remove(&owned.actor_id))
                }
                (None, _) => (false, None),
            }
        };
        drop(replaced);
        if destroyed {
            let path = actor_path(&host.dir, &owned.actor_id);
            return tokio::task::spawn_blocking(move || {
                destroy_file(&path)
            })
            .await?;
        }
        marked.map(|_| ()).context("mark the actor stopped")
    }

    /// Runs the stop on a task of its own and logs its error. The
    /// actor itself asked for it, so no caller waits.
    fn spawn_logged(self) {
        let actor_id = self.owned.actor_id.clone();
        tokio::spawn(async move {
            if let Err(error) = self.run().await {
                tracing::error!(
                    ?error,
                    actor_id,
                    "host failed to stop actor"
                );
            }
        });
    }
}

struct HostInner {
    dir: PathBuf,
    /// Held while the host may open actor files: its name's lock,
    /// which keeps a second host of that name out of the folder, and
    /// each of its kinds' locks, which keep another host from
    /// holding that kind. [`Host::shutdown`] lets them go once every
    /// file is closed, since a task of the host may still hold the
    /// rest for a moment. Otherwise they go with the host.
    locks: Mutex<Vec<FolderLock>>,
    directory: Directory,
    actors: Mutex<Actors>,
    session: watch::Sender<u64>,
    events: broadcast::Sender<HostEvent>,
    options: HostOptions,
    /// When the starts under way end, set once the host shuts down.
    starts_end: watch::Sender<Option<tokio::time::Instant>>,
}

/// A lock the host holds on one file: `<name>.lock` in the folder, or
/// `.lock` in a kind's folder. Its drop unlocks the file before it
/// closes it: a child process forked meanwhile shares the
/// open file, and would keep the lock until it runs its program.
struct FolderLock(File);

impl FolderLock {
    /// Locks the file at `path`, made if it is missing. Fails when
    /// another host holds it.
    fn take(path: &Path) -> Result<Self> {
        let file = File::create(path)
            .with_context(|| format!("open {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Self(file)),
            Err(TryLockError::WouldBlock) => {
                bail!(
                    "another host holds the lock {}",
                    path.display()
                )
            }
            Err(TryLockError::Error(error)) => Err(error)
                .with_context(|| format!("lock {}", path.display())),
        }
    }
}

impl Drop for FolderLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// How a [`Host`] runs its actors.
#[derive(Clone, Debug)]
pub struct HostOptions {
    /// How long a start may take, from opening the actor's file
    /// until its core is ready. A start that takes longer fails with
    /// `actor.not_ready`. 60 seconds by default.
    pub start_deadline: Duration,
    /// How long a stop may take, from asking the actor's core to
    /// stop until its file is closed. Past it, the stop fails with
    /// `actor.shutdown_timeout`, and the host gives the actor up:
    /// its file is closed with its running mark, so the next boot
    /// starts it, and this host starts it no more. 45 seconds by
    /// default. Keep it above the longest sleep grace of the host's
    /// kinds, 15 seconds unless a kind sets its own: a stop may take
    /// that long and more without being stuck.
    pub stop_deadline: Duration,
    /// The host's name, a plain file name: `host` by default. The
    /// host locks the folder with `<name>.lock`, so hosts of
    /// different names open one folder at once, and a second host of
    /// one name is refused. Hosts that share a folder run kinds of
    /// their own: each host locks `actors/<kind>/.lock` for each of
    /// its kinds, and a host given a kind another host holds is
    /// refused.
    pub name: String,
    /// What the host gives its actors, by type: none by default. See
    /// [`HostOptions::with_value`].
    pub values: Values,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            start_deadline: Duration::from_secs(60),
            stop_deadline: Duration::from_secs(45),
            name: "host".to_owned(),
            values: Values::default(),
        }
    }
}

/// The host's actors, and whether it has shut down.
struct Actors {
    /// Each actor that is not stopped with no deadline, by id.
    lives: HashMap<String, Life>,
    /// Set by [`Host::shutdown`], or when the last [`Host`] drops.
    /// From then on no transition adds a state: the host starts no
    /// actor and arms no timer.
    shut: bool,
    /// How many generations this host has begun to start. The last
    /// one's id is this count. A generation's handle reaches only
    /// the host that started it, so another host's ids may repeat
    /// these.
    generations: u64,
}

impl Actors {
    /// Makes the actor `Starting`, with a new generation id, for a
    /// start to run with the returned [`Owned`]. Returns the id and
    /// the state it replaced. Fails in a host that has shut down.
    fn begin_start(
        &mut self,
        host: &Arc<HostInner>,
        actor_id: &str,
    ) -> Result<(Owned, u64, Option<Life>)> {
        if self.shut {
            return Err(host_shut_down());
        }
        let owned = Owned::new(host, actor_id);
        self.generations += 1;
        let replaced = self.lives.insert(
            actor_id.to_owned(),
            Life::Starting {
                generation: self.generations,
                request: None,
                ready: owned.waiters.clone(),
            },
        );
        Ok((owned, self.generations, replaced))
    }

    /// Shuts the host: sets `shut`, and removes every `Stopped`
    /// state, its timer aborted.
    fn shut_down(&mut self) {
        self.shut = true;
        self.lives.retain(|_, life| {
            let Life::Stopped { timer, .. } = life else {
                return true;
            };
            timer.abort();
            false
        });
    }
}

impl HostInner {
    /// Shuts the host, as [`Actors::shut_down`] does, and ends the
    /// starts under way after [`START_GRACE_AT_SHUTDOWN`].
    fn shut_down(&self) {
        self.actors.lock().shut_down();
        self.starts_end.send_if_modified(|end| {
            if end.is_some() {
                return false;
            }
            *end = Some(
                tokio::time::Instant::now() + START_GRACE_AT_SHUTDOWN,
            );
            true
        });
    }

    /// Awaits `start` until it ends, its start deadline passes, or
    /// the host shuts down and its grace for starts passes. Fails
    /// with `actor.not_ready` at either of the last two.
    async fn within_start_deadline<T>(
        &self,
        actor_id: &str,
        start: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let deadline =
            tokio::time::Instant::now() + self.options.start_deadline;
        let mut end = self.starts_end.subscribe();
        let mut start = std::pin::pin!(start);
        loop {
            let shut = *end.borrow_and_update();
            let until =
                shut.map_or(deadline, |shut| shut.min(deadline));
            tokio::select! {
                started = &mut start => return started,
                () = tokio::time::sleep_until(until) => {
                    let why = match shut {
                        Some(shut) if shut < deadline => {
                            "the host shut down".to_owned()
                        }
                        _ => format!(
                            "it took over {:?}",
                            self.options.start_deadline
                        ),
                    };
                    return Err(ActorLifecycle::NotReady.build().context(
                        format!("the start of {actor_id} ended: {why}"),
                    ));
                }
                Ok(()) = end.changed() => {}
            }
        }
    }

    /// Asks the actor to stop: the generation of id `generation` when
    /// the actor asks, whichever generation runs when the host asks.
    /// Makes the transition, and returns the stop to run if one
    /// begins. A destroy the host asks for an actor that is stopped
    /// or absent begins too, unless the host has shut down.
    fn ask_stop(
        self: &Arc<Self>,
        actor_id: &str,
        generation: Option<u64>,
        reason: ShutdownKind,
    ) -> Option<Stop> {
        let mut actors = self.actors.lock();
        let actors = &mut *actors;
        let Some(life) = actors
            .lives
            .get_mut(actor_id)
            .filter(|life| !matches!(life, Life::Stopped { .. }))
        else {
            if generation.is_some()
                || !matches!(reason, ShutdownKind::Destroy)
                || actors.shut
            {
                return None;
            }
            let owned = Owned::new(self, actor_id);
            let replaced = actors.lives.insert(
                actor_id.to_owned(),
                Life::Destroying {
                    generation: None,
                    done: owned.waiters.clone(),
                },
            );
            if let Some(Life::Stopped { timer, .. }) = replaced {
                timer.abort();
            }
            return Some(Stop {
                owned,
                generation: None,
                reason,
            });
        };
        if generation.is_some_and(|id| !life.is_generation(id)) {
            return None;
        }
        match life {
            Life::Starting { request, .. } => {
                if generation.is_some()
                    && !matches!(request, Some(ShutdownKind::Destroy))
                {
                    *request = Some(reason);
                }
                None
            }
            Life::Running(running) => {
                let generation = running.clone();
                let owned = Owned::new(self, actor_id);
                *life = stopping(&owned, generation.clone(), reason);
                Some(Stop {
                    owned,
                    generation: Some(generation),
                    reason,
                })
            }
            Life::Stopping { generation, done }
                if matches!(reason, ShutdownKind::Destroy) =>
            {
                *life = destroying(generation.clone(), done.clone());
                None
            }
            Life::Reading { .. }
            | Life::Stopped { .. }
            | Life::Stopping { .. }
            | Life::Destroying { .. }
            | Life::Stuck => None,
        }
    }
}

/// What a host that has shut down answers a start.
fn host_shut_down() -> anyhow::Error {
    ActorLifecycle::Stopping
        .build()
        .context("the host has shut down")
}

/// What a stop of the actor `actor_id` that missed the stop deadline
/// answers, and every later start or stop of that actor in the host.
fn stop_missed(
    actor_id: &str,
    options: &HostOptions,
) -> anyhow::Error {
    ActorLifecycle::ShutdownTimeout.build().context(format!(
        "actor {actor_id} did not stop within {:?}, and this host \
         starts it no more",
        options.stop_deadline
    ))
}

/// The state of `generation` while `owned` stops it for `reason`.
fn stopping(
    owned: &Owned,
    generation: Arc<Generation>,
    reason: ShutdownKind,
) -> Life {
    let done = owned.waiters.clone();
    match reason {
        ShutdownKind::Sleep => Life::Stopping { generation, done },
        ShutdownKind::Destroy => destroying(generation, done),
    }
}

/// The state of an actor whose generation `generation` stops for a
/// destroy that `done` follows. Marks the generation's file destroyed
/// as soon as no transaction is open in it, ahead of the statements
/// its actor sends after, but those of a transaction open now: a kill
/// from then on leaves a file whose next start finishes the destroy.
fn destroying(generation: Arc<Generation>, done: Waiters) -> Life {
    // A file already closed is marked as it is removed.
    let _ = generation.file.when_idle(|conn| {
        mark_destroyed(conn)?;
        crate::fault::point("destroy-marked");
        Ok(())
    });
    Life::Destroying {
        generation: Some(generation),
        done,
    }
}

/// The actors of one realm folder, run in this process. When the last
/// clone drops, the host starts no actor again, as after
/// [`Host::shutdown`], and cancels the timers of stopped actors;
/// their deadlines stay in their files for the next boot. Starts and
/// stops under way end on their own, without a `Host`.
#[derive(Clone)]
pub struct Host(Arc<Handle>);

/// What the clones of one [`Host`] share. The host's own tasks hold
/// its [`HostInner`] alone, so this drops with the last `Host`.
struct Handle(Arc<HostInner>);

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.shut_down();
    }
}

/// The core's handle on the host for one generation of one actor. It
/// reaches only that generation: its SQL goes to the generation's own
/// file, its alarms to the generation's own record, and its stop
/// requests act only while the map holds that generation.
struct LocalHandle {
    host: Weak<HostInner>,
    actor_id: String,
    /// The generation's id, as [`Generation::id`].
    generation: u64,
    file: PlainSqlite,
    alarm: Arc<Mutex<Option<i64>>>,
}

impl Host {
    /// Takes the folder's lock and opens it, with each kind of
    /// `factories` running in this host, with the default
    /// [`HostOptions`]. Fails when another host of the same name
    /// holds the folder's lock, or another host holds one of the
    /// kinds.
    pub fn open(
        dir: &Path,
        factories: HashMap<String, ActorFactory>,
    ) -> Result<Self> {
        Self::open_with(dir, factories, HostOptions::default())
    }

    /// Opens the folder as [`Host::open`] does, with `options`.
    pub fn open_with(
        dir: &Path,
        factories: HashMap<String, ActorFactory>,
        options: HostOptions,
    ) -> Result<Self> {
        plain(&options.name, "host name")?;
        call::check_kinds(&factories)?;
        std::fs::create_dir_all(dir.join("actors"))?;
        let directory = Directory::local(factories);
        let mut locks = vec![FolderLock::take(
            &dir.join(format!("{}.lock", options.name)),
        )?];
        for kind in directory.local_kinds() {
            let folder = dir.join("actors").join(kind);
            std::fs::create_dir_all(&folder).with_context(|| {
                format!("make {}", folder.display())
            })?;
            // A key never starts with a dot, so no actor's file is
            // named so.
            locks.push(FolderLock::take(&folder.join(".lock"))?);
        }
        Ok(Self(Arc::new(Handle(Arc::new(HostInner {
            dir: dir.to_owned(),
            locks: Mutex::new(locks),
            directory,
            actors: Mutex::new(Actors {
                lives: HashMap::new(),
                shut: false,
                generations: 0,
            }),
            session: watch::Sender::new(LOCAL_SESSION),
            events: broadcast::channel(64).0,
            options,
            starts_end: watch::Sender::new(None),
        })))))
    }

    /// The host that every clone of this `Host` reaches.
    fn inner(&self) -> &Arc<HostInner> {
        &self.0.0
    }

    /// Reads the row of each file of this host's kinds, but for the
    /// actors the host holds already. Begins the start of each actor
    /// whose run a kill cut off or whose deadline has come, arms a
    /// timer for each other deadline, and returns the actors whose
    /// start it began, without waiting for the starts: a call to one
    /// of them waits for its start. A start that fails is logged and
    /// left for the next boot, and a file that cannot be read is
    /// reported and passed over, as is one that a destroy removed
    /// since boot listed the files. Boot finishes each destroy that a
    /// kill cut off, and starts no actor for it. A start of an actor
    /// whose file boot reads waits for the read. Fails at once in a
    /// host that has shut down, which may no longer hold the folder:
    /// it reads no file.
    pub async fn boot(&self) -> Result<Vec<String>> {
        let host = self.inner();
        if host.actors.lock().shut {
            return Err(host_shut_down());
        }
        let mut began = Vec::new();
        for (actor_id, actor, path) in self.actor_files()? {
            // While boot reads the file, a start of the actor waits.
            let reading = {
                let mut actors = host.actors.lock();
                if actors.shut {
                    return Err(host_shut_down());
                }
                // A running actor holds its file, and a stopped one
                // has its timer.
                if actors.lives.contains_key(&actor_id) {
                    continue;
                }
                let reading = Owned::new(host, &actor_id);
                actors.lives.insert(
                    actor_id.clone(),
                    Life::Reading {
                        done: reading.waiters.clone(),
                    },
                );
                reading
            };
            let stored = match stored_actor(&path) {
                Ok(Some(stored)) => stored,
                Ok(None) => continue,
                Err(error) => {
                    let error = format!("{error:#}");
                    tracing::warn!(
                        path = %path.display(),
                        error,
                        "boot could not read an actor's file and goes \
                         on without it"
                    );
                    let _ =
                        host.events.send(HostEvent::UnreadableFile {
                            actor,
                            error,
                        });
                    continue;
                }
            };
            let StoredActor {
                wake_at,
                cut_off,
                destroyed,
            } = stored;
            if destroyed {
                match destroy_file(&path) {
                    Ok(()) => tracing::info!(
                        actor_id,
                        "boot finished a destroy that a kill cut off"
                    ),
                    Err(error) => tracing::error!(
                        ?error,
                        actor_id,
                        "boot could not finish a destroy that a \
                         kill cut off"
                    ),
                }
                continue;
            }
            let (owned, generation) = {
                let mut actors = host.actors.lock();
                match wake_at {
                    // Whatever deadline an earlier stop left, a run a
                    // kill cut off starts again, and its core pushes
                    // its alarms anew.
                    _ if cut_off => {}
                    Some(wake_at) if wake_at > now_ms() => {
                        if actors.shut {
                            return Err(host_shut_down());
                        }
                        let timer = arm(host, &actor_id, wake_at);
                        actors.lives.insert(
                            actor_id,
                            Life::Stopped { wake_at, timer },
                        );
                        continue;
                    }
                    Some(_) => {}
                    None => continue,
                }
                let (owned, generation, _) =
                    actors.begin_start(host, &actor_id)?;
                (owned, generation)
            };
            drop(reading);
            began.push(actor_id.clone());
            tokio::spawn(async move {
                let started = if cut_off {
                    start_actor(owned, generation, Open::Existing)
                        .await
                } else {
                    start_when_due(owned, generation).await
                };
                if let Err(error) = started {
                    tracing::error!(
                        ?error,
                        actor_id,
                        "host failed to start an actor at boot"
                    );
                }
            });
        }
        Ok(began)
    }

    /// The actor for `kind` and `key`, started if it is not running.
    /// Returns its id.
    pub async fn get_or_start(
        &self,
        kind: &str,
        key: &str,
    ) -> Result<String> {
        let actor_id = self.inner().located(kind, key)?;
        self.inner()
            .ensure_started(&actor_id, &Open::OrCreate)
            .await?;
        Ok(actor_id)
    }
}

impl HostInner {
    /// The id of the actor of `kind` and `key`. Fails for a kind the
    /// directory does not hold, before anything reaches the folder.
    fn located(&self, kind: &str, key: &str) -> Result<String> {
        let Place::Local(_) = self.directory.place(kind)?;
        actor_id(kind, key)
    }
}

impl Host {
    /// Whether the actor holds its file: from the end of its start
    /// until its stop closed the file.
    pub fn is_running(&self, actor_id: &str) -> bool {
        self.inner()
            .actors
            .lock()
            .lives
            .get(actor_id)
            .is_some_and(|life| life.loaded().is_some())
    }

    /// Follows what happens to the host's actors from now on. A
    /// receiver that falls behind loses the oldest events; the actor
    /// files keep the record.
    pub fn events(&self) -> broadcast::Receiver<HostEvent> {
        self.inner().events.subscribe()
    }

    /// The one-shot timers the actor at `at` set aside after their
    /// last try. Starts the actor from its file if it is not running,
    /// and never makes one.
    pub async fn set_aside_timers(
        &self,
        at: &Address,
    ) -> Result<Vec<SetAsideTimer>> {
        let actor_id = self.inner().located(&at.kind, &at.key)?;
        let generation = self
            .inner()
            .ensure_started(&actor_id, &Open::Existing)
            .await?;
        generation.actor.context().set_aside_timers().await
    }

    /// Puts the set-aside timer `event_id` of the actor at `at` back,
    /// due at `due_at` in ms since the Unix epoch, with its tries
    /// counted from none. Returns false when the actor holds no
    /// set-aside timer of that id. Starts the actor as
    /// [`Host::set_aside_timers`] does.
    pub async fn rearm_timer(
        &self,
        at: &Address,
        event_id: &str,
        due_at: i64,
    ) -> Result<bool> {
        let actor_id = self.inner().located(&at.kind, &at.key)?;
        let generation = self
            .inner()
            .ensure_started(&actor_id, &Open::Existing)
            .await?;
        generation
            .actor
            .context()
            .rearm_set_aside(event_id, due_at)
            .await
    }

    /// Stops the actor and closes its file, or for a destroy removes
    /// its files: its file, its write-ahead file and its
    /// shared-memory file. A start under way is waited for, then
    /// stopped. A stop under way is waited for, and a destroy makes
    /// it a destroy. A sleep leaves a stopped actor as it is, its
    /// file and its timer with it; a destroy removes its files and
    /// cancels its timer, and one repeated after it ended ends the
    /// same way. Fails for an id of no kind of this host, before
    /// anything reaches the folder, and a destroy of a stopped actor
    /// fails in a host that has shut down, which may no longer hold
    /// the folder.
    ///
    /// A stop ends within [`HostOptions::stop_deadline`], whatever
    /// the actor does. An actor that does not stop by then is given
    /// up: the stop, and every caller that waited for it, fails with
    /// `actor.shutdown_timeout`, and so does every later start or
    /// stop of that actor in this host. Its file is closed with its
    /// running mark, and its destroy's mark if one was stored, so the
    /// next boot starts it again, or finishes the destroy. What it
    /// leaves behind: its code runs on, holding the thread it blocks
    /// until it returns, and a statement its file's thread still runs
    /// keeps the file open, locked, until the statement ends. Nothing
    /// ends either sooner but the process's exit, and a runtime's
    /// drop waits for its threads, so a process with such an actor
    /// exits without dropping its runtime.
    pub async fn stop(
        &self,
        actor_id: &str,
        reason: ShutdownKind,
    ) -> Result<()> {
        let (kind, key) =
            actor_id.split_once('/').with_context(|| {
                format!("`{actor_id}` is not kind/key")
            })?;
        self.inner().located(kind, key)?;
        let host = self.inner();
        loop {
            if let Some(stop) = host.ask_stop(actor_id, None, reason)
            {
                return tokio::spawn(stop.run()).await?;
            }
            let (mut waiters, ask_again) = {
                let actors = host.actors.lock();
                match actors.lives.get(actor_id) {
                    None | Some(Life::Stopped { .. }) => match reason
                    {
                        ShutdownKind::Sleep => return Ok(()),
                        ShutdownKind::Destroy if actors.shut => {
                            return Err(host_shut_down());
                        }
                        // It stopped since: ask again.
                        ShutdownKind::Destroy => continue,
                    },
                    // It started since: ask again.
                    Some(Life::Running(_)) => continue,
                    Some(
                        Life::Reading { done: waiters }
                        | Life::Starting { ready: waiters, .. },
                    ) => (waiters.clone(), true),
                    Some(
                        Life::Stopping { done, .. }
                        | Life::Destroying { done, .. },
                    ) => (done.clone(), false),
                    Some(Life::Stuck) => {
                        return Err(stop_missed(
                            actor_id,
                            &host.options,
                        ));
                    }
                }
            };
            let _ = waiters.changed().await;
            if !ask_again {
                return match &*waiters.borrow() {
                    Some(missed) => Err(missed.to_error()),
                    None => Ok(()),
                };
            }
        }
    }

    /// Stops every actor at once, and shuts the host: from then on it
    /// starts no actor, for a call, a boot or a timer. Cancels the
    /// timers of stopped actors; their deadlines stay in their files
    /// for the next boot. A start under way that has not ended after
    /// a grace of one second fails. Waits for every stop, and for a
    /// start or a stop under way, each within the stop deadline, so
    /// it returns once every actor's file is closed, and then lets
    /// the folder go: a new host may open it at once. A stop that
    /// fails ends no other: the error names each actor whose stop
    /// failed. An actor whose stop missed the deadline is named so,
    /// and leaves behind what [`Host::stop`] says: a statement still
    /// running on its file keeps that one file open.
    pub async fn shutdown(&self) -> Result<()> {
        self.inner().shut_down();
        // No state is added once the host is shut.
        let ids: Vec<String> = self
            .inner()
            .actors
            .lock()
            .lives
            .keys()
            .cloned()
            .collect();
        let stops: Vec<_> = ids
            .into_iter()
            .map(|actor_id| {
                let host = self.clone();
                let stopping = actor_id.clone();
                let stop = tokio::spawn(async move {
                    host.stop(&stopping, ShutdownKind::Sleep).await
                });
                (actor_id, stop)
            })
            .collect();
        let mut failed = Vec::new();
        for (actor_id, stop) in stops {
            if let Err(error) = stop
                .await
                .map_err(anyhow::Error::from)
                .and_then(|stopped| stopped)
            {
                failed.push(format!("{actor_id}: {error:#}"));
            }
        }
        // The host opens no file again. A file only a stop that
        // panicked still holds keeps its own lock.
        drop(std::mem::take(&mut *self.inner().locks.lock()));
        if !failed.is_empty() {
            bail!("the host failed to stop {}", failed.join("; "));
        }
        Ok(())
    }
}

impl HostInner {
    /// Starts the actor unless it runs, and returns the generation
    /// that runs it. Waits for a start or a stop under way first, and
    /// fails with a start it waited for, or a stop that missed its
    /// deadline. Fails at once for an actor that is being destroyed,
    /// or whose stop missed its deadline, and in a host that has shut
    /// down, also when it shuts down during the wait: such a host
    /// starts no actor again, so a wait could only delay the failure.
    /// Makes the actor only as `open` lets it: only `get_or_start`
    /// and a create make one. A call, a timer and boot start an actor
    /// from the file it has, and fail with `actor.not_found` for a
    /// key that has none, making no file. For [`Open::New`], fails
    /// at once with `actor.duplicate_key` for an actor that is
    /// `Stopped`, `Running` or `Stopping`, and leaves it as it is.
    async fn ensure_started(
        self: &Arc<Self>,
        actor_id: &str,
        open: &Open,
    ) -> Result<Arc<Generation>> {
        enum Next {
            Wait(Waiters),
            Start(Owned, u64),
        }
        // Holds a time once the host has shut down.
        let mut shut = self.starts_end.subscribe();
        loop {
            let next = {
                let mut actors = self.actors.lock();
                if actors.shut {
                    return Err(host_shut_down());
                }
                let life = actors.lives.get(actor_id);
                if matches!(open, Open::New(_))
                    && matches!(
                        life,
                        Some(
                            Life::Stopped { .. }
                                | Life::Running(_)
                                | Life::Stopping { .. }
                        )
                    )
                {
                    return Err(Found::Exists.to_error(actor_id));
                }
                match life {
                    Some(Life::Running(generation)) => {
                        return Ok(generation.clone());
                    }
                    Some(Life::Destroying { .. }) => {
                        return Err(ActorLifecycle::Destroying
                            .build()
                            .context(format!(
                                "actor {actor_id} is being destroyed"
                            )));
                    }
                    Some(Life::Stuck) => {
                        return Err(stop_missed(
                            actor_id,
                            &self.options,
                        ));
                    }
                    Some(
                        Life::Reading { done: waiters }
                        | Life::Starting { ready: waiters, .. }
                        | Life::Stopping { done: waiters, .. },
                    ) => Next::Wait(waiters.clone()),
                    Some(Life::Stopped { .. }) | None => {
                        let (owned, generation, replaced) =
                            actors.begin_start(self, actor_id)?;
                        if let Some(Life::Stopped { timer, .. }) =
                            replaced
                        {
                            timer.abort();
                        }
                        Next::Start(owned, generation)
                    }
                }
            };
            match next {
                Next::Wait(mut waiters) => {
                    tokio::select! {
                        _ = waiters.changed() => {}
                        _ = shut.wait_for(Option::is_some) => {
                            return Err(host_shut_down());
                        }
                    }
                    if let Some(failed) = &*waiters.borrow() {
                        return Err(failed.to_error());
                    }
                }
                Next::Start(owned, generation) => {
                    return tokio::spawn(start_actor(
                        owned,
                        generation,
                        open.clone(),
                    ))
                    .await?;
                }
            }
        }
    }
}

impl Host {
    /// The id, the address and the path of each actor file of this
    /// host's kinds. Another kind's folder is never read, since
    /// another host may run it.
    fn actor_files(&self) -> Result<Vec<(String, Address, PathBuf)>> {
        let host = self.inner();
        let mut found = Vec::new();
        for kind in host.directory.local_kinds() {
            let folder = host.dir.join("actors").join(kind);
            let files = match std::fs::read_dir(&folder) {
                Ok(files) => files,
                Err(error)
                    if error.kind()
                        == std::io::ErrorKind::NotFound =>
                {
                    continue;
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("read {}", folder.display())
                    });
                }
            };
            for file in files {
                let path = file?.path();
                let key = path
                    .file_stem()
                    .and_then(|key| key.to_str())
                    .filter(|_| {
                        path.extension()
                            .is_some_and(|ext| ext == "sqlite")
                    });
                // Not an actor's file.
                let Some((key, Ok(actor_id))) =
                    key.map(|key| (key, actor_id(kind, key)))
                else {
                    continue;
                };
                found.push((actor_id, Address::new(kind, key), path));
            }
        }
        Ok(found)
    }
}

/// The part of one actor file's `_hamlet_actor` row that boot reads.
struct StoredActor {
    /// The deadline the actor's last clean stop left.
    wake_at: Option<i64>,
    /// The actor was running when the host died: no clean stop
    /// cleared its mark.
    cut_off: bool,
    /// A destroy of the actor was stored, and a kill cut it off.
    destroyed: bool,
}

/// The `_hamlet_actor` row of the actor file at `path`, if it holds a
/// deadline, a run a kill cut off, or a destroy. A file that is gone
/// holds none: a destroy removed it after boot listed the files. The
/// file is closed again; starting the actor opens it anew.
fn stored_actor(path: &Path) -> Result<Option<StoredActor>> {
    // Boot holds the actor as `Reading`, so nothing of the host makes
    // or removes its file from here on.
    if !path
        .try_exists()
        .with_context(|| format!("find {}", path.display()))?
    {
        return Ok(None);
    }
    let conn = plain_sqlite::open(path, false)?;
    if !has_table(&conn, "_hamlet_actor")? {
        return Ok(None);
    }
    let stored = conn
        .query_row(
            "SELECT wake_at, running, destroying FROM _hamlet_actor",
            [],
            |row| {
                Ok(StoredActor {
                    wake_at: row.get(0)?,
                    cut_off: row.get(1)?,
                    destroyed: row.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(stored.filter(|stored| {
        stored.wake_at.is_some() || stored.cut_off || stored.destroyed
    }))
}

/// Whether the file has the table `name`. A table the core makes
/// lazily is missing from a file whose actor never needed it.
fn has_table(
    conn: &Connection,
    name: &str,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = \
         'table' AND name = ?)",
        [name],
        |row| row.get(0),
    )
}

/// Marks a stopped actor's file stopped, with the deadline that must
/// wake it, and returns that deadline: the earliest of its schedule
/// rows, claimed or not, its run wake deadline, and `alarm`, the last
/// alarm its generation pushed. A missing table holds no deadline.
fn mark_stopped(
    conn: &Connection,
    alarm: Option<i64>,
) -> Result<Option<i64>> {
    let scheduled: Option<i64> =
        if has_table(conn, "_actor_schedule_events")? {
            conn.query_row(
                "SELECT MIN(trigger_at) FROM _actor_schedule_events",
                [],
                |row| row.get(0),
            )?
        } else {
            None
        };
    let run_wake_at = if has_table(conn, "_actor_meta")? {
        conn.query_row(
            "SELECT value FROM _actor_meta WHERE key = ?",
            [RUN_WAKE_AT_META_KEY],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .map(|payload| decode_run_wake_at(&payload))
        .transpose()?
        .flatten()
    } else {
        None
    };
    let wake_at =
        [scheduled, run_wake_at, alarm].into_iter().flatten().min();
    crate::fault::point("run-ended");
    conn.execute(
        "UPDATE _hamlet_actor SET running = 0, wake_at = ?",
        [wake_at],
    )?;
    Ok(wake_at)
}

/// Stores a destroy of the actor in its file.
fn mark_destroyed(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE _hamlet_actor SET destroying = 1", [])?;
    Ok(())
}

/// The timer of a stopped actor. At `wake_at` it starts the actor, if
/// the actor is still `Stopped` with that deadline. Returns the
/// handle that aborts it.
fn arm(
    host: &Arc<HostInner>,
    actor_id: &str,
    wake_at: i64,
) -> AbortHandle {
    let host = Arc::downgrade(host);
    let actor_id = actor_id.to_owned();
    tokio::spawn(async move {
        let delay = u64::try_from(wake_at.saturating_sub(now_ms()))
            .unwrap_or(0);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        let Some(host) = host.upgrade() else {
            return;
        };
        if let Err(error) = wake(&host, &actor_id, wake_at).await {
            tracing::error!(
                ?error,
                actor_id,
                "host timer failed to start actor"
            );
        }
    })
    .abort_handle()
}

/// Starts the actor for its deadline `wake_at`, from the file it has,
/// if it is still `Stopped` with that deadline. A timer never makes a
/// file, and never starts an actor in any other state.
async fn wake(
    host: &Arc<HostInner>,
    actor_id: &str,
    wake_at: i64,
) -> Result<()> {
    let (owned, generation) = {
        let mut actors = host.actors.lock();
        if !matches!(
            actors.lives.get(actor_id),
            Some(Life::Stopped { wake_at: stopped, .. })
                if *stopped == wake_at
        ) {
            return Ok(());
        }
        // Replaces the state that holds this timer's own handle;
        // dropping a handle does not abort its task.
        let (owned, generation, _) =
            actors.begin_start(host, actor_id)?;
        (owned, generation)
    };
    start_when_due(owned, generation).await.map(|_| ())
}

/// Starts, from the file it has, the actor that `owned` holds as
/// `Starting`, as the generation of id `id`, for the deadline its
/// last stop left, which has come.
async fn start_when_due(
    owned: Owned,
    id: u64,
) -> Result<Arc<Generation>> {
    crate::fault::point("timer-fired");
    start_actor(owned, id, Open::Existing).await
}

/// The id of the actor of `kind` and `key`, which names its file.
/// Both must be plain file names.
fn actor_id(kind: &str, key: &str) -> Result<String> {
    for part in [kind, key] {
        plain(part, "actor kind or key")?;
    }
    Ok(format!("{kind}/{key}"))
}

/// Fails unless `name`, a `what`, is a plain file name.
fn plain(name: &str, what: &str) -> Result<()> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name.len() <= 128
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.')
        });
    ensure!(
        plain,
        "`{name}` is not a valid {what}: use letters, digits, `-`, \
         `_` and `.`, and do not start with `.`"
    );
    Ok(())
}

fn actor_path(dir: &Path, actor_id: &str) -> PathBuf {
    dir.join("actors").join(format!("{actor_id}.sqlite"))
}

/// Removes the actor's files at `path`. It stores the destroy in the
/// file first, and folds the write-ahead file into it, so a kill
/// before the file itself is removed leaves it marked. Then it
/// removes the write-ahead file and the shared-memory file, then the
/// file itself, so that no write-ahead file outlives its file and
/// joins one made later at that path. A file already missing is
/// passed over, so a destroy repeated ends the same way.
fn destroy_file(path: &Path) -> Result<()> {
    if path.exists() {
        let conn = plain_sqlite::open(path, false)?;
        // A file whose actor never started has no row to mark.
        if has_table(&conn, "_hamlet_actor")? {
            mark_destroyed(&conn)?;
        }
        let busy: i64 = conn.query_row(
            "PRAGMA wal_checkpoint(TRUNCATE)",
            [],
            |row| row.get(0),
        )?;
        ensure!(busy == 0, "{} stayed busy", path.display());
        conn.close().map_err(|(_, error)| error)?;
    }
    for suffix in ["-wal", "-shm", ""] {
        crate::fault::point("destroy-remove");
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        match std::fs::remove_file(&file) {
            Err(error)
                if error.kind() != std::io::ErrorKind::NotFound =>
            {
                return Err(error).with_context(|| {
                    format!("remove {}", Path::new(&file).display())
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// What a start may do with the actor's file.
#[derive(Clone, Debug)]
enum Open {
    /// Starts the actor the file holds, and fails with
    /// [`Found::Nothing`] for none: a call's start, a timer's and
    /// boot's.
    Existing,
    /// Starts the actor the file holds, made first if there is none:
    /// `get_or_start`'s start.
    OrCreate,
    /// Makes the actor, with this input kept in its row, and fails
    /// with [`Found::Exists`] for one the file holds: a create's
    /// start.
    New(Vec<u8>),
}

impl Open {
    /// Whether the start may make the actor's file and its row.
    fn makes(&self) -> bool {
        !matches!(self, Self::Existing)
    }
}

/// What a start found in the actor's file that ends it. Its waiters
/// do not share it: each reads the map again and starts the actor as
/// it asked, since another start may make what this one did not.
#[derive(Debug)]
enum Found {
    /// No file, or a file with no actor's row: no actor to start.
    Nothing,
    /// The actor's row, for a start that was to make the actor.
    Exists,
}

impl std::fmt::Display for Found {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Nothing => f.write_str("the file holds no actor"),
            Self::Exists => f.write_str("the file holds an actor"),
        }
    }
}

impl std::error::Error for Found {}

impl Found {
    /// What the start that found it answers its caller.
    fn to_error(&self, actor_id: &str) -> anyhow::Error {
        match self {
            Self::Nothing => ActorRuntime::NotFound {
                resource: "actor".to_owned(),
                id: actor_id.to_owned(),
            }
            .build()
            .context(format!("the actor {actor_id} does not exist")),
            Self::Exists => DuplicateKey
                .build()
                .context(format!("the actor {actor_id} exists")),
        }
    }
}

/// Opens the actor's file, made first if `open` makes it, adds a
/// generation to its `_hamlet_actor` row, marks it running, and
/// serves it. Returns the file and what the start read from the
/// row. Fails with [`Found::Nothing`] for
/// [`Open::Existing`] and no file or a file with no actor's row,
/// and with [`Found::Exists`] for [`Open::New`] and an actor's row. A
/// file that holds a destroy a kill cut off is removed first, as the
/// destroy would have: the start then makes the actor anew if `open`
/// makes it, and finds nothing if not.
fn open_actor_file(
    dir: &Path,
    kind: &str,
    key: &str,
    open: &Open,
) -> Result<(PlainSqlite, Bumped)> {
    if open.makes() {
        std::fs::create_dir_all(dir.join("actors").join(kind))?;
    }
    let path = actor_path(dir, &format!("{kind}/{key}"));
    // Runs twice at most: the second open finds no file to destroy.
    loop {
        if !open.makes() && !path.try_exists()? {
            return Err(Found::Nothing.into());
        }
        let mut conn = plain_sqlite::open(&path, open.makes())?;
        let bumped = bump_generation(&mut conn, kind, key, open)
            .with_context(|| format!("start {}", path.display()))?;
        if let Some(bumped) = bumped {
            return Ok((PlainSqlite::serve(conn)?, bumped));
        }
        conn.close().map_err(|(_, error)| error)?;
        destroy_file(&path)?;
    }
}

/// What a start reads from the actor's `_hamlet_actor` row.
struct Bumped {
    /// The generation the start added.
    generation: u32,
    /// Whether the start made the row: the actor's first start.
    first: bool,
    /// The input the actor was made with. Every start gives it to
    /// the core, which reads it only while the actor has no state:
    /// so a first start that fails leaves it for the next.
    input: Option<Vec<u8>>,
}

/// Adds a generation to the file's `_hamlet_actor` row, made first if
/// missing and `open` makes it, and marks it running. Returns what
/// the start reads from the row, or `None` for a file that holds a
/// destroy. Fails with [`Found::Nothing`] for a missing row
/// and [`Open::Existing`], and with [`Found::Exists`] for a row and
/// [`Open::New`], and changes nothing then. The row is the actor: of
/// two creates of one key, the one whose transaction writes it first
/// makes the actor.
fn bump_generation(
    conn: &mut Connection,
    kind: &str,
    key: &str,
    open: &Open,
) -> Result<Option<Bumped>> {
    let tx = conn.transaction()?;
    tx.execute_batch(ACTOR_TABLE)?;
    let destroyed: Option<bool> = tx
        .query_row(
            "SELECT destroying FROM _hamlet_actor",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match (destroyed, open) {
        (Some(true), _) => return Ok(None),
        (None, Open::Existing) => return Err(Found::Nothing.into()),
        (Some(false), Open::New(_)) => {
            return Err(Found::Exists.into());
        }
        _ => {}
    }
    let input = match open {
        Open::New(input) => Some(input),
        Open::Existing | Open::OrCreate => None,
    };
    let first = tx.execute(
        "INSERT OR IGNORE INTO _hamlet_actor (id, kind, key, \
         generation, input) VALUES (0, ?, ?, 0, ?)",
        (kind, key, input),
    )? == 1;
    let (generation, input) = tx.query_row(
        "UPDATE _hamlet_actor SET generation = generation + 1, \
         running = 1 RETURNING generation, input",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    tx.commit()?;
    Ok(Some(Bumped {
        generation,
        first,
        input,
    }))
}

/// Starts the actor that `owned` holds as `Starting`, as the
/// generation of id `id`, within the host's start deadline: opens its
/// file, made first if `open` makes it, off the async runtime, starts
/// its core, and makes it `Running`, or begins the stop its
/// generation asked for meanwhile. Returns the generation it started.
/// A start that fails, or misses its deadline, closes the file and
/// tells its waiters why. One that finds no actor, or a create's that
/// finds one, tells them nothing, so each reads the state again.
async fn start_actor(
    owned: Owned,
    id: u64,
    open: Open,
) -> Result<Arc<Generation>> {
    let host = owned.host.clone();
    let actor_id = owned.actor_id.clone();
    let opened = Mutex::new(None);
    let started = host
        .within_start_deadline(
            &actor_id,
            start_generation(&host, &actor_id, id, open, &opened),
        )
        .await;
    let generation = match started {
        Ok(generation) => generation,
        // A failed start keeps its running mark, so the next boot
        // tries it again rather than leave an alarm it may not have
        // stored. The core ends a start whose future was dropped at
        // the deadline, and the close fences what it does meanwhile.
        Err(error) => {
            let file = opened.lock().take();
            if let Some(file) = file {
                file.close().await;
            }
            // Not told to the waiters: each reads the map again.
            if let Some(found) = error.downcast_ref::<Found>() {
                return Err(found.to_error(&actor_id));
            }
            owned.fail(&error);
            return Err(error);
        }
    };
    let stop = {
        let mut actors = host.actors.lock();
        let Some(Life::Starting { request, .. }) =
            actors.lives.get(&actor_id)
        else {
            unreachable!(
                "only its start moves an actor out of Starting"
            )
        };
        match *request {
            None => {
                actors.lives.insert(
                    actor_id,
                    Life::Running(generation.clone()),
                );
                None
            }
            Some(reason) => {
                let owned = Owned::new(&host, &actor_id);
                actors.lives.insert(
                    actor_id,
                    stopping(&owned, generation.clone(), reason),
                );
                Some(Stop {
                    owned,
                    generation: Some(generation.clone()),
                    reason,
                })
            }
        }
    };
    // Wakes the start's waiters, which find the actor in its new
    // state.
    drop(owned);
    if let Some(stop) = stop {
        stop.spawn_logged();
    }
    Ok(generation)
}

/// Opens the actor's file and starts its core, as the generation of
/// id `id`. Puts the file in `opened` once it is open.
async fn start_generation(
    host: &Arc<HostInner>,
    actor_id: &str,
    id: u64,
    open: Open,
    opened: &Mutex<Option<PlainSqlite>>,
) -> Result<Arc<Generation>> {
    let (kind, key) = actor_id
        .split_once('/')
        .ok_or_else(|| anyhow!("actor id {actor_id} has no kind"))?;
    let Place::Local(factory) = host.directory.place(kind)?;
    let factory = factory.clone();
    let (file, bumped) = tokio::task::spawn_blocking({
        let dir = host.dir.clone();
        let (kind, key) = (kind.to_owned(), key.to_owned());
        move || open_actor_file(&dir, &kind, &key, &open)
    })
    .await??;
    *opened.lock() = Some(file.clone());
    let alarm = Arc::new(Mutex::new(None));
    let actor = RunningActor::start(StartRequest {
        actor_id: actor_id.to_owned(),
        name: kind.to_owned(),
        key: vec![ActorKeySegment::String(key.to_owned())],
        generation: bumped.generation,
        factory,
        handle: Arc::new(LocalHandle {
            host: Arc::downgrade(host),
            actor_id: actor_id.to_owned(),
            generation: id,
            file: file.clone(),
            alarm: alarm.clone(),
        }),
        new_storage: bumped.first,
        input: bumped.input,
    })
    .await?;
    Ok(Arc::new(Generation {
        id,
        actor,
        file,
        alarm,
    }))
}

impl LocalHandle {
    /// This generation asks to stop. A request from a generation that
    /// has ended does nothing.
    fn ask_stop(&self, reason: ShutdownKind) {
        if let Some(stop) = self.host.upgrade().and_then(|host| {
            host.ask_stop(
                &self.actor_id,
                Some(self.generation),
                reason,
            )
        }) {
            stop.spawn_logged();
        }
    }
}

impl ActorHost for LocalHandle {
    fn endpoint(&self) -> &str {
        "local"
    }

    fn namespace(&self) -> &str {
        "local"
    }

    fn pool_name(&self) -> &str {
        "local"
    }

    fn connection_session(&self) -> Option<u64> {
        Some(LOCAL_SESSION)
    }

    /// The session never changes, so a transaction never sees its
    /// connection lost.
    fn subscribe_connection_session(&self) -> watch::Receiver<u64> {
        match self.host.upgrade() {
            Some(host) => host.session.subscribe(),
            None => watch::channel(0).1,
        }
    }

    /// A crash, reported with an error, stops the actor as a sleep.
    fn sleep_actor(
        &self,
        _actor_id: String,
        _generation: Option<u32>,
        _error: Option<String>,
    ) {
        self.ask_stop(ShutdownKind::Sleep);
    }

    fn stop_actor(
        &self,
        _actor_id: String,
        _generation: Option<u32>,
    ) {
        self.ask_stop(ShutdownKind::Destroy);
    }

    /// Keeps the alarm as this generation's last, and acknowledges it
    /// at once. The host writes no running actor's alarm to its file:
    /// the alarm lives in the core, the stop reads this record into
    /// the deadline it stores, and a kill leaves the actor marked
    /// running, so its next start pushes its alarms again. An ended
    /// generation's alarm reaches only its own record.
    fn set_alarm_with_ack(
        &self,
        _actor_id: String,
        alarm_ts: Option<i64>,
        _generation: Option<u32>,
        ack_tx: Option<oneshot::Sender<()>>,
    ) {
        *self.alarm.lock() = alarm_ts;
        if let Some(ack_tx) = ack_tx {
            let _ = ack_tx.send(());
        }
    }

    fn sqlite_exec(
        &self,
        _actor_id: String,
        sql: String,
    ) -> RuntimeBoxFuture<Result<QueryResult>> {
        let file = self.file.clone();
        Box::pin(async move { file.exec(sql).await })
    }

    fn sqlite_execute(
        &self,
        _actor_id: String,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> RuntimeBoxFuture<Result<ExecuteResult>> {
        let file = self.file.clone();
        Box::pin(async move { file.execute(sql, params).await })
    }

    fn sqlite_execute_batch(
        &self,
        _actor_id: String,
        statements: Vec<SqliteBatchStatement>,
    ) -> RuntimeBoxFuture<Result<Vec<ExecuteResult>>> {
        let file = self.file.clone();
        Box::pin(async move { file.execute_batch(statements).await })
    }

    fn timer_set_aside(&self, actor_id: &str, timer: &SetAsideTimer) {
        tracing::warn!(
            actor_id,
            event_id = timer.event_id,
            action = timer.action,
            tries = timer.tries,
            error = timer.last_error,
            "the host set a timer aside"
        );
        let Some(host) = self.host.upgrade() else {
            return;
        };
        let Some((kind, key)) = actor_id.split_once('/') else {
            return;
        };
        let _ = host.events.send(HostEvent::TimerSetAside {
            actor: Address::new(kind, key),
            event_id: timer.event_id.clone(),
            action: timer.action.clone(),
            due_at: timer.due_at,
            tries: timer.tries,
            error: timer.last_error.clone(),
        });
    }
}

/// Milliseconds since the Unix epoch, the clock alarms use.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../examples/demo/counter.rs"]
mod counter;

#[cfg(test)]
mod tests {
    use hamlet_actor_core::{ActorConfig, ActorStart};
    use hamlet_testing::Scratch;

    use super::*;
    use crate::cause::Cause;

    /// The kind `waker`, whose run handler sets the run wake deadline
    /// `wake_at` and then answers only lifecycle events.
    fn waker(wake_at: i64) -> HashMap<String, ActorFactory> {
        let config = ActorConfig {
            name: Some("waker".to_owned()),
            has_database: true,
            ..ActorConfig::default()
        };
        HashMap::from([(
            "waker".to_owned(),
            ActorFactory::new(config, move |start: ActorStart| {
                Box::pin(async move {
                    let ActorStart {
                        ctx, mut events, ..
                    } = start;
                    ctx.set_run_wake_at(Some(wake_at)).await?;
                    while let Some(event) = events.recv().await {
                        counter::answer(event);
                    }
                    Ok(())
                })
            }),
        )])
    }

    /// The deadline a stop stores comes from the file as well as from
    /// the generation's last alarm: none from a file whose core made
    /// no tables, and the run wake deadline with no alarm pushed.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_stop_reads_its_deadline_from_the_file() {
        let dir = Scratch::new("host-stop-deadline");
        let mut bare =
            plain_sqlite::open(&dir.join("bare.sqlite"), true)
                .unwrap();
        bump_generation(&mut bare, "bare", "a", &Open::OrCreate)
            .unwrap();
        assert_eq!(mark_stopped(&bare, None).unwrap(), None);
        assert_eq!(mark_stopped(&bare, Some(5)).unwrap(), Some(5));

        let wake_at = now_ms() + 600_000;
        let host = Host::open(&dir, waker(wake_at)).unwrap();
        let id = host.get_or_start("waker", "a").await.unwrap();
        host.stop(&id, ShutdownKind::Sleep).await.unwrap();
        assert!(matches!(
            host.inner().actors.lock().lives.get(&id),
            Some(Life::Stopped { wake_at: stopped, .. })
                if *stopped == wake_at
        ));
        drop(host);
        let file = plain_sqlite::open(&actor_path(&dir, &id), false)
            .unwrap();
        assert_eq!(mark_stopped(&file, None).unwrap(), Some(wake_at));
    }

    /// The core of an ended generation keeps its handle. Through it,
    /// that generation can neither stop nor destroy the actor's
    /// running generation, give it an alarm, nor reach its file. If
    /// `made_again`, the actor is destroyed and its key made again
    /// first: the new file counts its generations from the start
    /// again.
    async fn an_ended_generations_handle_reaches_nothing(
        made_again: bool,
    ) {
        let dir = Scratch::new("host-ended-handle");
        let host = Host::open(
            &dir,
            counter::factories(counter::Sleep::WhenIdle),
        )
        .unwrap();
        let id = host.get_or_start("counter", "a").await.unwrap();
        let running = || {
            host.inner().actors.lock().lives[&id]
                .loaded()
                .unwrap()
                .clone()
        };
        let ended = {
            let generation = running();
            LocalHandle {
                host: Arc::downgrade(host.inner()),
                actor_id: id.clone(),
                generation: generation.id,
                file: generation.file.clone(),
                alarm: generation.alarm.clone(),
            }
        };
        host.stop(&id, ShutdownKind::Sleep).await.unwrap();
        host.get_or_start("counter", "a").await.unwrap();
        if made_again {
            host.stop(&id, ShutdownKind::Destroy).await.unwrap();
            host.get_or_start("counter", "a").await.unwrap();
        }
        let next = running();
        let counter_a = Address::new("counter", "a");
        let call = |name: &'static str| {
            host.call(
                &counter_a,
                name,
                Vec::new(),
                Duration::from_secs(600),
                Cause::root("test"),
            )
        };
        call("increment").await.unwrap();

        ended.sleep_actor(id.clone(), Some(1), None);
        ended.sleep_actor(id.clone(), Some(1), Some("crash".into()));
        ended.stop_actor(id.clone(), Some(1));
        let (ack, acked) = oneshot::channel();
        ended.set_alarm_with_ack(
            id.clone(),
            Some(now_ms() + 60_000),
            Some(1),
            Some(ack),
        );
        acked
            .await
            .expect("the ended generation's alarm was not acked");
        assert!(
            ended
                .sqlite_execute(
                    id.clone(),
                    "UPDATE counter SET value = 100".to_owned(),
                    None,
                )
                .await
                .is_err()
        );

        assert!(matches!(
            host.inner().actors.lock().lives.get(&id),
            Some(Life::Running(generation))
                if Arc::ptr_eq(generation, &next)
                    && generation.alarm.lock().is_none()
        ));
        let reply = call("get").await.unwrap();
        assert_eq!(counter::count(reply).unwrap(), 1);
        host.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_ended_generations_handle_reaches_nothing_of_the_next()
    {
        an_ended_generations_handle_reaches_nothing(false).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_ended_generations_handle_reaches_nothing_of_its_key_made_again()
     {
        an_ended_generations_handle_reaches_nothing(true).await;
    }
}
