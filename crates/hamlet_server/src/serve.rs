//! The app's runner: the server from start to stop. Its first update
//! opens and starts what it serves (`startup.rs`); then it runs the
//! inputs one at a time until the app exits, which a stop signal
//! leads to (`closing.rs`).

use std::time::Duration;

use anyhow::Result;
use bevy_app::{App, AppExit};

use crate::closing::StopAsked;
use crate::server::{InputSender, Server};

/// The longest the runner sleeps with nothing coming.
const IDLE: Duration = Duration::from_secs(60);

/// The app's runner: serves it on a Tokio runtime of one thread,
/// sleeping until an input comes, until it exits. A server that fails
/// says why.
pub(crate) fn runner(app: App) -> AppExit {
    let served = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(serve(app)));
    match served {
        Ok(()) => AppExit::Success,
        Err(error) => {
            failed(&error);
            AppExit::error()
        }
    }
}

/// Says why the server could not serve, as `main` would have.
#[expect(
    clippy::print_stderr,
    reason = "the failure is the server's last output for whoever \
              started it"
)]
fn failed(error: &anyhow::Error) {
    eprintln!("Error: {error:?}");
}

/// Serves `app` until it exits. The first SIGTERM or Ctrl-C asks it
/// to stop, and a second exits at once. A realm that can no longer be
/// served is stopped too, and its error is the server's.
async fn serve(app: App) -> Result<()> {
    let signals = Signals::new()?;
    hamlet_os::process::reap_with_server()?;
    let mut server = Server::start(app)?;
    tokio::spawn(stop_on_signals(signals, server.inputs()));
    let mut fatal = None;
    while !server.has_exited() {
        match server.step(IDLE).await {
            Ok(_) => {}
            // A second while it stops ends the server where it is.
            Err(error) if fatal.is_some() => return Err(error.0),
            Err(error) => {
                fatal = Some(error.0);
                let _ = server.inputs().send(StopAsked);
            }
        }
    }
    fatal.map_or(Ok(()), Err)
}

/// Asks the server to stop at the first signal, and exits at the
/// second.
async fn stop_on_signals(mut signals: Signals, inputs: InputSender) {
    signals.next().await;
    let _ = inputs.send(StopAsked);
    signals.next().await;
    std::process::exit(130);
}

struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
    /// The signal `hamlet dev` sends the server it keeps in a
    /// process group of its own.
    #[cfg(windows)]
    ctrl_break: tokio::signal::windows::CtrlBreak,
}

impl Signals {
    fn new() -> Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                interrupt: tokio::signal::windows::ctrl_c()?,
                ctrl_break: tokio::signal::windows::ctrl_break()?,
            })
        }
    }

    async fn next(&mut self) {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.interrupt.recv() => {}
                _ = self.terminate.recv() => {}
            }
        }
        #[cfg(windows)]
        {
            tokio::select! {
                _ = self.interrupt.recv() => {}
                _ = self.ctrl_break.recv() => {}
            }
        }
    }
}
