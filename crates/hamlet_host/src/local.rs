//! Runs actors in this process. Each actor is one plain SQLite file
//! at `actors/<kind>/<key>.sqlite` under the realm folder. The host
//! finds an actor by its address, starts it, calls it, and stops it.
//!
//! The file's `_hamlet_actor` row marks the actor running from its
//! start until a clean stop, and holds the deadline that stop left:
//! the earliest of the actor's schedule rows, its run wake deadline
//! and the last alarm its core pushed, read once the core has
//! stopped. A running actor's alarms live in its core alone, and the
//! host keeps no timer for it. A stopped actor with a deadline has
//! one timer, which starts it from its own file. Boot reads the files
//! of the host's own kinds, starts again each actor a kill left
//! marked running, and arms a timer for each other stored deadline. A
//! host locks the folder by its name, so hosts of different names,
//! each with kinds of its own, share one folder.
//!
//! Each actor's place in its lifecycle is one [`Life`] in the host's
//! map. One actor's start or stop waits only on that actor's work. A
//! start has a deadline. A shutdown stops every actor at once, ends
//! the starts under way after a short grace, and the host starts
//! none after it, nor after its last [`Host`] is dropped.

use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use hamlet_actor_core::actor::schedule::{
    RUN_WAKE_AT_META_KEY, SetAsideTimer, decode_run_wake_at,
};
use hamlet_actor_core::host::{
    ActionTimedOut, ActorHost, RunningActor, StartRequest,
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
use tokio::task::{AbortHandle, JoinHandle, JoinSet};
use tracing::Instrument;

use crate::cause::Cause;
use crate::directory::{Address, Directory, Place};
use crate::event::{HostEvent, Letter};
use crate::plain_sqlite::{self, PlainSqlite};

/// The one session a local file has. The transaction code compares
/// sessions to detect a lost connection, which a local file cannot
/// have.
const LOCAL_SESSION: u64 = 1;

/// How long a shutdown lets the starts under way go on before it
/// ends them.
const START_GRACE_AT_SHUTDOWN: Duration = Duration::from_secs(1);

/// The host's row in each actor file. Its version ladder is Hamlet's,
/// apart from the core's `_actor_` tables.
const ACTOR_TABLE: &str = "CREATE TABLE IF NOT EXISTS _hamlet_actor \
                           (
	id INTEGER PRIMARY KEY CHECK (id = 0),
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	generation INTEGER NOT NULL,
	wake_at INTEGER,
	running INTEGER NOT NULL DEFAULT 0)";

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
/// | absent | `get_or_start` or a call, or boot for a run a kill cut off, the host not shut | `Starting` |
/// | absent | boot finds a deadline in its file, the host not shut | `Stopped` |
/// | `Stopped` | `get_or_start` or a call, the host not shut | `Starting`, its timer aborted |
/// | `Stopped` | its timer fires with the deadline the state holds | `Starting`, from the file it has |
/// | `Stopped` | `Host::shutdown`, or the last `Host` dropped | absent, its timer aborted |
/// | `Starting` | its generation asks to sleep or to be destroyed, or reports a crash | `Starting`, with the request kept; a destroy wins over a sleep |
/// | `Starting` | the start ended well | `Running`, or `Stopping` or `Destroying` for a kept request |
/// | `Starting` | the start failed or panicked, or missed its deadline | absent, the failure told to its waiters |
/// | `Running` | its generation asks to sleep or reports a crash, or `Host::stop(Sleep)` | `Stopping` |
/// | `Running` | its generation asks to be destroyed, or `Host::stop(Destroy)` | `Destroying` |
/// | `Stopping` | its generation asks to be destroyed, or `Host::stop(Destroy)` | `Destroying` |
/// | `Stopping` | its generation asks to sleep or reports a crash, or `Host::stop(Sleep)` | unchanged |
/// | `Destroying` | any request | unchanged |
/// | `Stopping` | the stop ended with a deadline, the host not shut | `Stopped`, its timer armed |
/// | `Stopping`, `Destroying` | the stop ended otherwise, however it ended | absent |
/// | any | a request from a generation the state does not hold | unchanged |
///
/// Callers wait without a transition of their own. A start, a call's
/// included, waits for a `Starting` or `Stopping` actor and then
/// starts it, and fails at once for a `Destroying` one. A start that
/// waited for a start that failed fails with it, rather than start
/// again. `Host::stop` waits for a `Starting` actor and then stops
/// it, waits for a stop under way, and leaves a `Stopped` actor as it
/// is. `Host::shutdown` stops each actor as `Host::stop` does.
enum Life {
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
    /// This generation stops, then its file is removed.
    Destroying {
        generation: Arc<Generation>,
        done: Waiters,
    },
}

/// What the waiters of a `Starting`, `Stopping` or `Destroying` state
/// await: the channel closes when the work that owns the state ends,
/// and holds how a start failed.
type Waiters = watch::Receiver<Option<StartFailed>>;

/// How a start failed, for each of its waiters to return.
#[derive(Clone)]
struct StartFailed {
    error: ActorError,
    /// The failure with its causes, as the start returned it.
    detail: String,
}

impl StartFailed {
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
            Self::Stopped { .. } | Self::Starting { .. } => None,
            Self::Running(generation)
            | Self::Stopping { generation, .. }
            | Self::Destroying { generation, .. } => Some(generation),
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
            Self::Stopped { .. } | Self::Running(_) => None,
            Self::Starting { ready: waiters, .. }
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

/// The work that owns an actor's `Starting`, `Stopping` or
/// `Destroying` state. When it is dropped, however its work ended, it
/// removes the state from the map if the state is still its own, and
/// then wakes the state's waiters.
struct Owned {
    host: Arc<HostInner>,
    actor_id: String,
    /// Wakes the waiters when it drops, after [`Drop::drop`].
    wake: watch::Sender<Option<StartFailed>>,
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

    /// Tells the waiters that the start this owns failed with
    /// `error`.
    fn fail(&self, error: &anyhow::Error) {
        self.wake.send_replace(Some(StartFailed::new(error)));
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
    generation: Arc<Generation>,
    reason: ShutdownKind,
}

impl Stop {
    /// Stops the generation, marks it stopped in its file with the
    /// deadline it leaves, and closes the file, then removes the file
    /// if the actor is destroyed by then. A sleep becomes `Stopped`,
    /// or leaves the map, once the file is closed; a destroy leaves
    /// the map when `owned` drops at the end. A sleep whose file
    /// could not be marked stopped fails, its file closed all the
    /// same: the file keeps its running mark, so the next boot starts
    /// it again.
    async fn run(self) -> Result<()> {
        let Self {
            owned,
            generation,
            reason,
        } = self;
        generation.actor.stop(reason).await;
        // The core has stopped and its alarm pushes are acknowledged,
        // so the generation's last alarm and its file are final.
        let alarm = *generation.alarm.lock();
        let marked = generation
            .file
            .close_with(move |conn| mark_stopped(conn, alarm))
            .await;
        let wake_at = marked.as_ref().ok().copied().flatten();
        let host = &owned.host;
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
                            )
                            .abort_handle(),
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
            return std::fs::remove_file(&path).with_context(|| {
                format!("remove {}", path.display())
            });
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
    /// Held while the host may open actor files: it keeps a second
    /// host out of the folder. [`Host::shutdown`] lets it go once
    /// every file is closed, since a task of the host may still hold
    /// the rest for a moment. Otherwise it goes with the host.
    lock: Mutex<Option<FolderLock>>,
    directory: Directory,
    actors: Mutex<Actors>,
    session: watch::Sender<u64>,
    commits: broadcast::Sender<String>,
    events: broadcast::Sender<HostEvent>,
    options: HostOptions,
    /// When the starts under way end, set once the host shuts down.
    starts_end: watch::Sender<Option<tokio::time::Instant>>,
}

/// The lock of `host.lock`. Its drop unlocks the file before it
/// closes it: a child process forked meanwhile shares the open file,
/// and would keep the lock until it runs its program.
struct FolderLock(File);

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
    /// The host's name, a plain file name: `host` by default. The
    /// host locks the folder with `<name>.lock`, so hosts of
    /// different names open one folder at once, and a second host of
    /// one name is refused. Hosts that share a folder must run kinds
    /// of their own.
    pub name: String,
}

impl Default for HostOptions {
    fn default() -> Self {
        Self {
            start_deadline: Duration::from_secs(60),
            name: "host".to_owned(),
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
    /// begins.
    fn ask_stop(
        self: &Arc<Self>,
        actor_id: &str,
        generation: Option<u64>,
        reason: ShutdownKind,
    ) -> Option<Stop> {
        let mut actors = self.actors.lock();
        let life = actors.lives.get_mut(actor_id)?;
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
                    generation,
                    reason,
                })
            }
            Life::Stopping { generation, done }
                if matches!(reason, ShutdownKind::Destroy) =>
            {
                *life = Life::Destroying {
                    generation: generation.clone(),
                    done: done.clone(),
                };
                None
            }
            Life::Stopped { .. }
            | Life::Stopping { .. }
            | Life::Destroying { .. } => None,
        }
    }
}

/// What a host that has shut down answers a start.
fn host_shut_down() -> anyhow::Error {
    ActorLifecycle::Stopping
        .build()
        .context("the host has shut down")
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
        ShutdownKind::Destroy => {
            Life::Destroying { generation, done }
        }
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
    /// holds the lock.
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
        std::fs::create_dir_all(dir.join("actors"))?;
        let lock_path = dir.join(format!("{}.lock", options.name));
        let lock = File::create(&lock_path).with_context(|| {
            format!("open {}", lock_path.display())
        })?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => bail!(
                "another host holds the lock {}",
                lock_path.display()
            ),
            Err(TryLockError::Error(error)) => {
                return Err(error).with_context(|| {
                    format!("lock {}", lock_path.display())
                });
            }
        }
        Ok(Self(Arc::new(Handle(Arc::new(HostInner {
            dir: dir.to_owned(),
            lock: Mutex::new(Some(FolderLock(lock))),
            directory: Directory::local(factories),
            actors: Mutex::new(Actors {
                lives: HashMap::new(),
                shut: false,
                generations: 0,
            }),
            session: watch::Sender::new(LOCAL_SESSION),
            commits: broadcast::channel(64).0,
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
    /// actors the host holds already. Starts again each actor whose
    /// run a kill cut off, all at once, and arms a timer for each
    /// other actor whose last stop left a deadline. A due timer
    /// starts its actor at once, and boot waits for it. Returns the
    /// started actors. An actor that fails to start is logged and
    /// left for the next boot. Fails at once in a host that has shut
    /// down, which may no longer hold the folder: it reads no file.
    pub async fn boot(&self) -> Result<Vec<String>> {
        if self.inner().actors.lock().shut {
            return Err(host_shut_down());
        }
        let mut started = Vec::new();
        let mut cut_off = JoinSet::new();
        let mut due = Vec::new();
        for stored in self.stored_actors()? {
            let actor_id = stored.actor_id;
            // Whatever deadline an earlier stop left, a run a kill
            // cut off starts again, and its core pushes its alarms
            // anew.
            if stored.cut_off {
                let host = self.clone();
                cut_off.spawn(async move {
                    let result =
                        host.ensure_started(&actor_id, false).await;
                    (actor_id, result)
                });
                continue;
            }
            let Some(wake_at) = stored.wake_at else {
                continue;
            };
            let mut actors = self.inner().actors.lock();
            if actors.shut || actors.lives.contains_key(&actor_id) {
                continue;
            }
            let timer = arm(self.inner(), &actor_id, wake_at);
            actors.lives.insert(
                actor_id.clone(),
                Life::Stopped {
                    wake_at,
                    timer: timer.abort_handle(),
                },
            );
            if wake_at <= now_ms() {
                due.push((actor_id, timer));
            }
        }
        while let Some(joined) = cut_off.join_next().await {
            match joined? {
                (actor_id, Ok(_)) => started.push(actor_id),
                (actor_id, Err(error)) => tracing::error!(
                    ?error,
                    actor_id,
                    "host failed to start an actor at boot"
                ),
            }
        }
        for (actor_id, timer) in due {
            if timer.await.unwrap_or(false) {
                started.push(actor_id);
            }
        }
        Ok(started)
    }

    /// The actor for `kind` and `key`, started if it is not running.
    /// Returns its id.
    pub async fn get_or_start(
        &self,
        kind: &str,
        key: &str,
    ) -> Result<String> {
        let actor_id = self.located(kind, key)?;
        self.ensure_started(&actor_id, true).await?;
        Ok(actor_id)
    }

    /// Calls the action `name` of the actor at `to`, started first if
    /// it is not running, as `get_or_start` starts it. The call ends
    /// within `deadline`, its start included, and the action within
    /// the shorter of what is left and the kind's action timeout:
    /// past either, it fails with `actor.action_timed_out`. Fails at
    /// once with `actor.not_registered` for a kind the directory does
    /// not hold, and with `actor.destroying` for an actor that is
    /// being destroyed. The host's log carries `cause`'s task.
    pub async fn call(
        &self,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
        cause: Cause,
    ) -> Result<Vec<u8>> {
        let actor_id = self.located(&to.kind, &to.key)?;
        let span = tracing::debug_span!(
            "call",
            actor_id,
            action = name,
            task = cause.task()
        );
        async {
            let begun = Instant::now();
            let generation = tokio::time::timeout(
                deadline,
                self.ensure_started(&actor_id, true),
            )
            .await
            .map_err(|_| ActionTimedOut.build())??;
            let left = deadline.saturating_sub(begun.elapsed());
            generation.actor.action(name, args, left).await
        }
        .instrument(span)
        .await
    }

    /// The id of the actor of `kind` and `key`. Fails for a kind the
    /// directory does not hold, before anything reaches the folder.
    fn located(&self, kind: &str, key: &str) -> Result<String> {
        let Place::Local(_) = self.inner().directory.place(kind)?;
        actor_id(kind, key)
    }

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

    /// Follows the actors whose transactions commit, by id.
    pub fn commits(&self) -> broadcast::Receiver<String> {
        self.inner().commits.subscribe()
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
        let actor_id = self.located(&at.kind, &at.key)?;
        let generation =
            self.ensure_started(&actor_id, false).await?;
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
        let actor_id = self.located(&at.kind, &at.key)?;
        let generation =
            self.ensure_started(&actor_id, false).await?;
        generation
            .actor
            .context()
            .rearm_set_aside(event_id, due_at)
            .await
    }

    /// Stops the actor and closes its file, or removes it for a
    /// destroy. A start under way is waited for, then stopped. A stop
    /// under way is waited for, and a destroy makes it a destroy. A
    /// stopped actor is left as it is, its file and its timer with
    /// it.
    pub async fn stop(
        &self,
        actor_id: &str,
        reason: ShutdownKind,
    ) -> Result<()> {
        let host = self.inner();
        loop {
            if let Some(stop) = host.ask_stop(actor_id, None, reason)
            {
                return tokio::spawn(stop.run()).await?;
            }
            let (mut waiters, starting) =
                match host.actors.lock().lives.get(actor_id) {
                    None | Some(Life::Stopped { .. }) => {
                        return Ok(());
                    }
                    // It started since: ask again.
                    Some(Life::Running(_)) => continue,
                    Some(Life::Starting { ready, .. }) => {
                        (ready.clone(), true)
                    }
                    Some(
                        Life::Stopping { done, .. }
                        | Life::Destroying { done, .. },
                    ) => (done.clone(), false),
                };
            let _ = waiters.changed().await;
            if !starting {
                return Ok(());
            }
        }
    }

    /// Stops every actor at once, and shuts the host: from then on it
    /// starts no actor, for a call, a boot or a timer. Cancels the
    /// timers of stopped actors; their deadlines stay in their files
    /// for the next boot. A start under way that has not ended after
    /// a grace of one second fails. Waits for every stop, and for a
    /// start or a stop under way, so it returns once every actor's
    /// file is closed, and then lets the folder go: a new host may
    /// open it at once. A stop that fails ends no other: the error
    /// names each actor whose stop failed.
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
        drop(self.inner().lock.lock().take());
        if !failed.is_empty() {
            bail!("the host failed to stop {}", failed.join("; "));
        }
        Ok(())
    }

    /// Starts the actor unless it runs, and returns the generation
    /// that runs it. Waits for a start or a stop under way first, and
    /// fails with a start it waited for. Fails at once for an actor
    /// that is being destroyed, and in a host that has shut down,
    /// also when it shuts down during the wait: such a host starts
    /// no actor again, so a wait could only delay the failure.
    /// Makes the actor's file only if `create`, which only
    /// `get_or_start` and a call ask for: boot starts an actor from
    /// the file it has, and never makes an empty one.
    async fn ensure_started(
        &self,
        actor_id: &str,
        create: bool,
    ) -> Result<Arc<Generation>> {
        enum Next {
            Wait(Waiters),
            Start(Owned, u64),
        }
        // Holds a time once the host has shut down.
        let mut shut = self.inner().starts_end.subscribe();
        loop {
            let next = {
                let mut actors = self.inner().actors.lock();
                if actors.shut {
                    return Err(host_shut_down());
                }
                match actors.lives.get(actor_id) {
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
                    Some(
                        Life::Starting { ready: waiters, .. }
                        | Life::Stopping { done: waiters, .. },
                    ) => Next::Wait(waiters.clone()),
                    Some(Life::Stopped { .. }) | None => {
                        let (owned, generation, replaced) = actors
                            .begin_start(self.inner(), actor_id)?;
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
                        owned, generation, create,
                    ))
                    .await?;
                }
            }
        }
    }

    /// The row of each file of this host's kinds that holds a
    /// deadline or a run a kill cut off, but for the actors the host
    /// holds: a running one holds its file locked, and a stopped one
    /// has its timer. Another kind's folder is never read, since
    /// another host may run it. The files are closed again; starting
    /// an actor opens its file anew.
    fn stored_actors(&self) -> Result<Vec<StoredActor>> {
        let host = self.inner();
        let mut stored = Vec::new();
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
                let Some(Ok(actor_id)) =
                    key.map(|key| actor_id(kind, key))
                else {
                    continue;
                };
                let held = || {
                    host.actors.lock().lives.contains_key(&actor_id)
                };
                if held() {
                    continue;
                }
                match stored_actor(&path, &actor_id) {
                    Ok(Some(actor)) => stored.push(actor),
                    Ok(None) => {}
                    // A start that began since holds it.
                    Err(_) if held() => {}
                    Err(error) => {
                        return Err(error.context(format!(
                            "read {}",
                            path.display()
                        )));
                    }
                }
            }
        }
        Ok(stored)
    }
}

/// The part of one actor file's `_hamlet_actor` row that boot reads.
struct StoredActor {
    actor_id: String,
    /// The deadline the actor's last clean stop left.
    wake_at: Option<i64>,
    /// The actor was running when the host died: no clean stop
    /// cleared its mark.
    cut_off: bool,
}

/// The `_hamlet_actor` row of the file of the actor `actor_id`, if it
/// holds a deadline or a run a kill cut off.
fn stored_actor(
    path: &Path,
    actor_id: &str,
) -> Result<Option<StoredActor>> {
    let conn = plain_sqlite::open(path, false)?;
    if !has_table(&conn, "_hamlet_actor")? {
        return Ok(None);
    }
    let stored: Option<(Option<i64>, bool)> = conn
        .query_row(
            "SELECT wake_at, running FROM _hamlet_actor",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(stored
        .filter(|(wake_at, cut_off)| wake_at.is_some() || *cut_off)
        .map(|(wake_at, cut_off)| StoredActor {
            actor_id: actor_id.to_owned(),
            wake_at,
            cut_off,
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

/// The timer of a stopped actor. At `wake_at` it starts the actor, if
/// the actor is still `Stopped` with that deadline. Its output says
/// whether it started the actor.
fn arm(
    host: &Arc<HostInner>,
    actor_id: &str,
    wake_at: i64,
) -> JoinHandle<bool> {
    let host = Arc::downgrade(host);
    let actor_id = actor_id.to_owned();
    tokio::spawn(async move {
        let delay = u64::try_from(wake_at.saturating_sub(now_ms()))
            .unwrap_or(0);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        let Some(host) = host.upgrade() else {
            return false;
        };
        wake(&host, &actor_id, wake_at)
            .await
            .inspect_err(|error| {
                tracing::error!(
                    ?error,
                    actor_id,
                    "host timer failed to start actor"
                );
            })
            .unwrap_or(false)
    })
}

/// Starts the actor for its deadline `wake_at`, from the file it has,
/// if it is still `Stopped` with that deadline. A timer never makes a
/// file, and never starts an actor in any other state. Returns
/// whether it started the actor.
async fn wake(
    host: &Arc<HostInner>,
    actor_id: &str,
    wake_at: i64,
) -> Result<bool> {
    let (owned, generation) = {
        let mut actors = host.actors.lock();
        if !matches!(
            actors.lives.get(actor_id),
            Some(Life::Stopped { wake_at: stopped, .. })
                if *stopped == wake_at
        ) {
            return Ok(false);
        }
        // Replaces the state that holds this timer's own handle;
        // dropping a handle does not abort its task.
        let (owned, generation, _) =
            actors.begin_start(host, actor_id)?;
        (owned, generation)
    };
    crate::fault::point("timer-fired");
    start_actor(owned, generation, false).await.map(|_| true)
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

/// Opens the actor's file, made first if `create`, adds a generation
/// to its `_hamlet_actor` row, marks it running, and serves it.
/// Returns the file, the new generation, and whether this is the
/// actor's first start.
fn open_actor_file(
    dir: &Path,
    kind: &str,
    key: &str,
    create: bool,
) -> Result<(PlainSqlite, u32, bool)> {
    if create {
        std::fs::create_dir_all(dir.join("actors").join(kind))?;
    }
    let path = actor_path(dir, &format!("{kind}/{key}"));
    let mut conn = plain_sqlite::open(&path, create)?;
    let (generation, first) =
        bump_generation(&mut conn, kind, key)
            .with_context(|| format!("start {}", path.display()))?;
    Ok((PlainSqlite::serve(conn)?, generation, first))
}

fn bump_generation(
    conn: &mut Connection,
    kind: &str,
    key: &str,
) -> Result<(u32, bool)> {
    let tx = conn.transaction()?;
    tx.execute_batch(ACTOR_TABLE)?;
    let first = tx.execute(
        "INSERT OR IGNORE INTO _hamlet_actor (id, kind, key, \
         generation) VALUES (0, ?, ?, 0)",
        (kind, key),
    )? == 1;
    let generation = tx.query_row(
        "UPDATE _hamlet_actor SET generation = generation + 1, \
         running = 1 RETURNING generation",
        [],
        |row| row.get(0),
    )?;
    tx.commit()?;
    Ok((generation, first))
}

/// Starts the actor that `owned` holds as `Starting`, as the
/// generation of id `id`, within the host's start deadline: opens its
/// file, made first if `create`, off the async runtime, starts its
/// core, and makes it `Running`, or begins the stop its generation
/// asked for meanwhile. Returns the generation it started. A start
/// that fails, or misses its deadline, closes the file and tells its
/// waiters why.
async fn start_actor(
    owned: Owned,
    id: u64,
    create: bool,
) -> Result<Arc<Generation>> {
    let host = owned.host.clone();
    let actor_id = owned.actor_id.clone();
    let opened = Mutex::new(None);
    let started = host
        .within_start_deadline(
            &actor_id,
            start_generation(&host, &actor_id, id, create, &opened),
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
                    generation: generation.clone(),
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
    create: bool,
    opened: &Mutex<Option<PlainSqlite>>,
) -> Result<Arc<Generation>> {
    let (kind, key) = actor_id
        .split_once('/')
        .ok_or_else(|| anyhow!("actor id {actor_id} has no kind"))?;
    let Place::Local(factory) = host.directory.place(kind)?;
    let factory = factory.clone();
    let (file, number, first) = tokio::task::spawn_blocking({
        let dir = host.dir.clone();
        let (kind, key) = (kind.to_owned(), key.to_owned());
        move || open_actor_file(&dir, &kind, &key, create)
    })
    .await??;
    *opened.lock() = Some(file.clone());
    let alarm = Arc::new(Mutex::new(None));
    let actor = RunningActor::start(StartRequest {
        actor_id: actor_id.to_owned(),
        name: kind.to_owned(),
        key: vec![ActorKeySegment::String(key.to_owned())],
        generation: number,
        factory,
        handle: Arc::new(LocalHandle {
            host: Arc::downgrade(host),
            actor_id: actor_id.to_owned(),
            generation: id,
            file: file.clone(),
            alarm: alarm.clone(),
        }),
        new_storage: first,
        input: None,
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

    fn transaction_committed(&self, actor_id: &str) {
        if let Some(host) = self.host.upgrade() {
            let _ = host.commits.send(actor_id.to_owned());
        }
    }

    fn timer_set_aside(&self, actor_id: &str, timer: &SetAsideTimer) {
        tracing::warn!(
            actor_id,
            event_id = timer.event_id,
            action = timer.action,
            tries = timer.tries,
            error = timer.last_error,
            "dead letter: the host set a timer aside"
        );
        let Some(host) = self.host.upgrade() else {
            return;
        };
        let Some((kind, key)) = actor_id.split_once('/') else {
            return;
        };
        let _ = host.events.send(HostEvent::DeadLetter {
            holder: Address::new(kind, key),
            letter: Letter::Timer {
                event_id: timer.event_id.clone(),
                action: timer.action.clone(),
                due_at: timer.due_at,
            },
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
        bump_generation(&mut bare, "bare", "a").unwrap();
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
