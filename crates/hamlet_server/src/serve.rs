//! The app's runner: the server from start to stop. Its first update
//! opens and starts what it serves (`startup.rs`); then it runs the
//! inputs one at a time until a signal, and closes in order (S3).

use std::time::Duration;

use anyhow::Result;
use bevy_app::{App, AppExit};

use crate::clients::connection::GRACE;
use crate::server::{Fatal, Server};

/// The time a closing realm waits for its runtimes to stop.
const STOPPING: Duration = Duration::from_secs(30);
/// The time a closing realm waits for a credential rotation begun.
const ROTATING: Duration = Duration::from_secs(15);

/// The app's runner: serves it on a Tokio runtime of one thread until
/// SIGTERM or Ctrl-C, sleeping until an input comes. A server that
/// fails says why.
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

/// Serves `app` until SIGTERM or Ctrl-C, closing every client before
/// letting the realm go.
async fn serve(app: App) -> Result<()> {
    let mut signals = Signals::new()?;
    hamlet_os::process::reap_with_server()?;
    let mut server = Server::start(app)?;
    let served = async {
        loop {
            server.step(Duration::from_secs(60)).await?;
        }
        #[expect(
            unreachable_code,
            reason = "the loop ends with a fatal error or a signal"
        )]
        Ok::<_, Fatal>(())
    };
    let result = tokio::select! {
        result = served => result.map_err(|fatal| fatal.0),
        () = signals.next() => Ok(()),
    };
    let mut tasks = {
        let mut core = server.core()?;
        core.clients.close_connections();
        std::mem::take(&mut core.clients.connection_tasks)
    };
    let drained =
        async { while tasks.join_next().await.is_some() {} };
    tokio::select! {
        () = signals.next() => std::process::exit(130),
        _ = tokio::time::timeout(GRACE, drained) => {}
    }
    tasks.abort_all();
    let closed = server.close(ROTATING);
    tokio::select! {
        () = signals.next() => std::process::exit(130),
        closed = closed => closed?,
    }
    let stopped = server.stop(STOPPING);
    tokio::select! {
        () = signals.next() => std::process::exit(130),
        stopped = stopped => stopped?,
    }
    result
}

pub(crate) struct Signals {
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

    pub(crate) async fn next(&mut self) {
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
