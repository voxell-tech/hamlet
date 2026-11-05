//! The thread that runs jobs, apart from the loop, on a database
//! connection of its own.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use hamlet_store::Store;
use tokio::sync::mpsc::UnboundedSender;

use crate::clock;
use crate::diagnostic::{Secrets, tell};
use crate::layout::Layout;

/// The longest the runner sleeps with nothing due.
const IDLE: Duration = Duration::from_secs(60);

pub(crate) struct Runner {
    wake: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Runner {
    /// Starts the runner for the realm at `layout`. It sends `worked`
    /// after each pass that ran a job.
    pub fn start(
        layout: &Layout,
        worked: UnboundedSender<()>,
    ) -> Result<Self> {
        let mut store = Store::open(&layout.database())?;
        let layout = layout.clone();
        let (wake, woken) = mpsc::channel::<()>();
        let thread = thread::Builder::new()
            .name("hamlet-jobs".into())
            .spawn(move || {
                loop {
                    let now = clock::now();
                    match super::run::due(
                        store.connection(),
                        &layout,
                        now,
                    ) {
                        Ok(0) => {}
                        Ok(_) => {
                            if worked.send(()).is_err() {
                                return;
                            }
                        }
                        Err(error) => tell(
                            Secrets::Fixed,
                            format!("running jobs: {error:#}"),
                        ),
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
