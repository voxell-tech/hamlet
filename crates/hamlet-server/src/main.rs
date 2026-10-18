//! `hamlet-server`: holds a home and its realm database until SIGTERM
//! or Ctrl-C.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use hamlet_server::Home;

const USAGE: &str = "usage: hamlet-server [--home <dir>]; the home \
                     is --home, else HAMLET_HOME";

fn main() -> Result<()> {
    let dir = home_dir(std::env::args_os().skip(1))?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(serve(&dir))
}

async fn serve(dir: &Path) -> Result<()> {
    // Listened for before the home opens, so a stop always closes it
    // in order.
    let stopped = stopped()?;
    let home = Home::open(dir)?;
    eprintln!(
        "hamlet-server: holding {}; Ctrl-C stops",
        home.layout().root().display()
    );
    stopped.await;
    home.close()
}

/// The home named by `--home <dir>`, else by `HAMLET_HOME`.
fn home_dir(
    mut args: impl Iterator<Item = OsString>,
) -> Result<PathBuf> {
    let named = match (args.next(), args.next(), args.next()) {
        (None, ..) => std::env::var_os("HAMLET_HOME"),
        (Some(flag), Some(dir), None) if flag == "--home" => {
            Some(dir)
        }
        _ => bail!(USAGE),
    };
    match named.filter(|dir| !dir.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => bail!(USAGE),
    }
}

#[cfg(unix)]
fn stopped() -> Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => {}
            _ = terminate.recv() => {}
        }
    })
}

#[cfg(windows)]
fn stopped() -> Result<impl Future<Output = ()>> {
    let mut interrupt = tokio::signal::windows::ctrl_c()?;
    Ok(async move {
        interrupt.recv().await;
    })
}
