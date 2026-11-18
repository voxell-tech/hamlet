//! The thread that runs jobs, apart from the loop, on a database
//! connection of its own.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use bevy_log::error;
use diesel::connection::SimpleConnection;
use hamlet_store::ids::AgentId;
use hamlet_store::{Db, Store};

use crate::app::clock;
use crate::app::failure::panicked;
use crate::app::follow_up::InputEffects;
use crate::app::input::{AddHandler, InputSender};
use crate::conversations::dreams::{DreamEnded, Learned, learning};
use crate::core::RemoteWorkspace;
use crate::realm::Realm;
use crate::realm::jobs::Job;
use crate::realm::layout::Layout;

/// The longest the runner sleeps with nothing due.
const IDLE: Duration = Duration::from_secs(60);

/// What a pass of the runner did comes back as an input.
pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_handler::<JobsDone, _>(take_in_jobs_done);
}

/// A pass of the runner that did jobs.
#[derive(Message)]
pub(crate) struct JobsDone {
    pub jobs: Vec<Job>,
    /// Whether each hire the jobs may have changed has learned
    /// anything.
    pub learning: BTreeMap<AgentId, bool>,
}

impl JobsDone {
    pub fn of(db: &mut Db, layout: &Layout, jobs: Vec<Job>) -> Self {
        let learning = learning(db, layout, &jobs);
        Self { jobs, learning }
    }
}

/// The lodges the runner's jobs changed are owed a push, and the
/// clients see what they did.
fn take_in_jobs_done(
    mut done: ResMut<Messages<JobsDone>>,
    mut realm: ResMut<Realm>,
    mut remote: RemoteWorkspace,
    mut changed: ResMut<InputEffects>,
    mut learned: ResMut<Learned>,
    inputs: Res<InputSender>,
) {
    for done in done.drain() {
        remote.owe_pushes(realm.store().connection(), &done.jobs);
        after_jobs(&mut learned, &inputs, &done);
        changed.state_changed_outside_change = true;
    }
}

/// What the hires learned and the dreams that ended, after `done`.
pub(crate) fn after_jobs(
    learned: &mut Learned,
    inputs: &InputSender,
    done: &JobsDone,
) {
    learned.take_in(&done.learning);
    let dreams = done
        .jobs
        .iter()
        .filter_map(|job| match job {
            Job::DreamEnd { dream, summary, .. } => {
                Some((*dream, summary.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if !dreams.is_empty() && inputs.send(DreamEnded(dreams)).is_err()
    {
        error!("the next dream could not begin: the server stops");
    }
}

#[derive(Resource)]
pub(crate) struct Runner {
    wake: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

/// The longest the runner waits for the database while it holds
/// [`RepositoryWriteLock`], which the loop may wait on.
const BUSY: Duration = Duration::from_millis(250);

/// Held by whoever writes the realm's repositories and lodges.
#[derive(Resource, Clone, Default)]
pub(crate) struct RepositoryWriteLock(Arc<Mutex<()>>);

impl RepositoryWriteLock {
    /// Holds it until the guard drops; a holder that panicked let it
    /// go.
    pub fn hold(&self) -> MutexGuard<'_, ()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs every job due now, returning those done.
pub(crate) fn run_due(
    db: &mut Db,
    layout: &Layout,
    writing: &RepositoryWriteLock,
) -> Result<Vec<Job>> {
    let mut done = Vec::new();
    // A job may keep jobs of its own, which run in the next pass.
    for _ in 0..4 {
        let now = clock::now();
        let ran = super::run::due(db, layout, writing, now)?;
        if ran.is_empty() {
            break;
        }
        done.extend(ran);
    }
    Ok(done)
}

impl Runner {
    /// Starts the runner for the realm at `layout`, which runs again
    /// the jobs that failed when their time comes. It sends `inputs`
    /// [`JobsDone`] after each pass that ran a job.
    pub fn start(
        layout: &Layout,
        inputs: InputSender,
        writing: RepositoryWriteLock,
    ) -> Result<Self> {
        let mut store = open(layout)?;
        let layout = layout.clone();
        let (wake, woken) = mpsc::channel::<()>();
        let thread = thread::Builder::new()
            .name("hamlet-jobs".into())
            .spawn(move || {
                loop {
                    // A pass that panics or leaves the store unsure
                    // ends; the runner goes on with the store opened
                    // again, so no job stops the others for good
                    // (S27).
                    let pass = catch_unwind(AssertUnwindSafe(|| {
                        run_due(store.connection(), &layout, &writing)
                    }));
                    let failed = match pass {
                        Ok(Ok(done)) if done.is_empty() => None,
                        Ok(Ok(done)) => {
                            let done = JobsDone::of(
                                store.connection(),
                                &layout,
                                done,
                            );
                            if inputs.send(done).is_err() {
                                return;
                            }
                            None
                        }
                        Ok(Err(error)) => Some(format!("{error:#}")),
                        Err(panic) => Some(panicked(&*panic)),
                    };
                    if let Some(failure) = failed {
                        error!("running jobs: {failure}");
                        if hamlet_store::unwind(store.connection())
                            .is_err()
                        {
                            match open(&layout) {
                                Ok(opened) => store = opened,
                                Err(error) => error!(
                                    "opening the store for jobs: \
                                     {error:#}"
                                ),
                            }
                        }
                    }
                    let wait =
                        hamlet_store::job::next(store.connection())
                            .ok()
                            .flatten()
                            .map_or(IDLE, |at| {
                                Duration::from_millis(
                                    u64::try_from(at - clock::now())
                                        .unwrap_or(0),
                                )
                                .min(IDLE)
                            });
                    match woken.recv_timeout(wait) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })?;
        Ok(Self {
            wake: Some(wake),
            thread: Some(thread),
        })
    }

    /// Runs what is due now.
    pub fn wake(&self) {
        if let Some(wake) = &self.wake {
            let _ = wake.send(());
        }
    }
}

/// The runner's own connection to the realm's database, which waits
/// at most [`BUSY`] for it.
fn open(layout: &Layout) -> Result<Store> {
    let mut store = Store::open(&layout.database())?;
    store.connection().batch_execute(&format!(
        "PRAGMA busy_timeout = {};",
        BUSY.as_millis()
    ))?;
    Ok(store)
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.wake = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
