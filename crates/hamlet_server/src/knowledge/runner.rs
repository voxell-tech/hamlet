//! The thread that runs jobs, apart from the loop, on a database
//! connection of its own.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use bevy_ecs::message::Message;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use diesel::connection::SimpleConnection;
use hamlet_store::Store;

use crate::clock;
use crate::core::RemoteWorkspace;
use crate::diagnostic::{Secrets, panicked, tell};
use crate::follow_up::InputEffects;
use crate::jobs::Job;
use crate::layout::Layout;
use crate::realm::Realm;
use crate::server::{AddHandler, InputMessage, InputSender};

/// The longest the runner sleeps with nothing due.
const IDLE: Duration = Duration::from_secs(60);

/// What a pass of the runner did comes back as an input.
pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_handler::<JobsDone, _>(take_in_jobs_done);
}

/// A pass of the runner that did jobs.
#[derive(Message)]
pub(crate) struct JobsDone(pub Vec<Job>);

/// The lodges the runner's jobs changed are owed a push, and the
/// clients see what they did.
fn take_in_jobs_done(
    mut done: InputMessage<JobsDone>,
    mut realm: ResMut<Realm>,
    mut remote: RemoteWorkspace,
    mut changed: ResMut<InputEffects>,
) {
    if let Some(JobsDone(done)) = done.take() {
        remote.owe_pushes(realm.store().connection(), &done);
        changed.state_changed_outside_change = true;
    }
}

#[derive(Resource)]
pub(crate) struct Runner {
    wake: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

/// The longest the runner waits for the database while it holds
/// [`RepositoryWriteLock`]. The loop may hold the write lock while
/// it waits for `RepositoryWriteLock`, so the runner gives up soon
/// and its job runs again later.
const BUSY: Duration = Duration::from_millis(250);

/// Held while a job runs, so that the loop and the runner never write
/// the realm's repositories and lodges at once. The runner holds it
/// for one job at a time.
#[derive(Resource, Clone, Default)]
pub(crate) struct RepositoryWriteLock(Arc<Mutex<()>>);

impl RepositoryWriteLock {
    /// Holds it until the guard drops; a holder that panicked let it
    /// go.
    pub fn hold(&self) -> MutexGuard<'_, ()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs every job due now, each holding `writing`. Returns the jobs
/// done.
pub(crate) fn run_due(
    db: &mut hamlet_store::Db,
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
                            if inputs.send(JobsDone(done)).is_err() {
                                return;
                            }
                            None
                        }
                        Ok(Err(error)) => Some(format!("{error:#}")),
                        Err(panic) => Some(panicked(&*panic)),
                    };
                    if let Some(failure) = failed {
                        tell(
                            Secrets::Fixed,
                            format!("running jobs: {failure}"),
                        );
                        if hamlet_store::unwind(store.connection())
                            .is_err()
                        {
                            match open(&layout) {
                                Ok(opened) => store = opened,
                                Err(error) => tell(
                                    Secrets::Fixed,
                                    format!(
                                        "opening the store for \
                                         jobs: {error:#}"
                                    ),
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
