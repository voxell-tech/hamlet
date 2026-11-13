//! The thread that runs jobs, apart from the loop, on a database
//! connection of its own.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use bevy_ecs::resource::Resource;
use hamlet_store::Store;
use tokio::sync::mpsc::UnboundedSender;

use crate::clock;
use crate::diagnostic::{Secrets, panicked, tell};
use crate::jobs::Job;
use crate::layout::Layout;

/// The longest the runner sleeps with nothing due.
const IDLE: Duration = Duration::from_secs(60);

#[derive(Resource)]
pub(crate) struct Runner {
    wake: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

/// Held while jobs run, so that the loop and the runner never write
/// the realm's repositories and lodges at once.
#[derive(Resource, Clone, Default)]
pub(crate) struct Writing(Arc<Mutex<()>>);

impl Writing {
    /// Holds it until the guard drops; a holder that panicked let it
    /// go.
    pub fn hold(&self) -> MutexGuard<'_, ()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs every job due now, holding `writing`. Returns the jobs done.
pub(crate) fn run_due(
    db: &mut hamlet_store::Db,
    layout: &Layout,
    writing: &Writing,
) -> Result<Vec<Job>> {
    let _held = writing.hold();
    let mut done = Vec::new();
    // A job may keep jobs of its own, which run in the next pass.
    for _ in 0..4 {
        let now = clock::now();
        let ran = super::run::due(db, layout, now)?;
        if ran.is_empty() {
            break;
        }
        done.extend(ran);
    }
    Ok(done)
}

impl Runner {
    /// Starts the runner for the realm at `layout`, which runs again
    /// the jobs that failed when their time comes. It sends `worked`
    /// after each pass that ran a job.
    pub fn start(
        layout: &Layout,
        worked: UnboundedSender<Vec<Job>>,
        writing: Writing,
    ) -> Result<Self> {
        let mut store = Store::open(&layout.database())?;
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
                            if worked.send(done).is_err() {
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
                            match Store::open(&layout.database()) {
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

impl Drop for Runner {
    fn drop(&mut self) {
        self.wake = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
