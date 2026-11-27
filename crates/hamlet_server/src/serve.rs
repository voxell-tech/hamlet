//! How the server is started, the address it publishes, and the
//! signals that stop it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_protocol::client::Confinement;
use hamlet_sandbox::Isolation;
use serde_json::json;

use crate::credentials::write_whole;

/// How long a closing realm waits for a credential rotation begun.
pub(crate) const ROTATING: Duration = Duration::from_secs(15);

const USAGE: &str = "usage: hamlet_server [--home <dir>] [--port \
                     <n>] [--faux <file>] [--insecure]";

/// How the server is started.
pub struct Options {
    pub home: PathBuf,
    pub port: u16,
    pub faux: Option<PathBuf>,
    pub isolation: Isolation,
}

impl Options {
    pub fn parse(
        mut args: impl Iterator<Item = OsString>,
    ) -> Result<Self> {
        let mut home = std::env::var_os("HAMLET_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        let mut port = 7313;
        let mut faux = None;
        let mut isolation = Isolation::Sandboxed;
        while let Some(flag) = args.next() {
            let mut value = || args.next().context(USAGE);
            match flag.to_str() {
                Some("--insecure") => isolation = insecure()?,
                Some("--home") => home = Some(value()?.into()),
                Some("--port") => {
                    port = value()?
                        .to_str()
                        .context("port must be a number")?
                        .parse()
                        .context("port must be 0 to 65535")?;
                }
                Some("--faux") => faux = Some(value()?.into()),
                _ => bail!(USAGE),
            }
        }
        let home = home
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|p| PathBuf::from(p).join(".hamlet"))
            })
            .context("set --home or HAMLET_HOME")?;
        Ok(Self {
            home,
            port,
            faux,
            isolation,
        })
    }
}

/// `--insecure`, which only a Windows build takes (C18).
#[cfg(windows)]
fn insecure() -> Result<Isolation> {
    Ok(Isolation::Insecure)
}

/// `--insecure`, which only a Windows build takes (C18).
#[cfg(not(windows))]
fn insecure() -> Result<Isolation> {
    bail!(
        "Insecure mode is only for Windows. This platform has a \
         sandbox."
    )
}

/// The published address, removed when the server stops.
pub(crate) struct Published(PathBuf);

impl Drop for Published {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Publishes the address `url` a client reaches the server at in the
/// file `path`, until the returned value drops.
pub(crate) fn publish(
    path: PathBuf,
    url: &str,
    confinement: &Confinement,
) -> Result<Published> {
    let started = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let address = json!({
        "url": url,
        "pid": std::process::id(),
        "started": started,
        "confinement": confinement,
    });
    write_whole(&path, &serde_json::to_vec(&address)?)?;
    Ok(Published(path))
}

/// Tells whoever started the server which realm it holds and where
/// it serves.
#[expect(
    clippy::print_stderr,
    reason = "the banner is the server's output for whoever started \
              it"
)]
pub(crate) fn announce(root: &Path, url: &str) {
    eprintln!(
        "hamlet_server: holding {}; serving at {url}; Ctrl-C stops",
        hamlet_os::fs::path::text(root)
    );
}

/// The chief's name (C7): `HAMLET_USER` when set, else the account
/// the server runs as, else "chief".
pub(crate) fn chief_name() -> String {
    ["HAMLET_USER", "USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|name| {
            std::env::var(name).ok().filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| "chief".into())
}

/// Serves the realm at `options.home` from the actors until SIGTERM
/// or Ctrl-C.
pub async fn serve(options: Options) -> Result<()> {
    crate::front::serve(options).await
}

pub(crate) struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
    /// What `hamlet dev` sends the server it keeps in a process
    /// group of its own.
    #[cfg(windows)]
    brk: tokio::signal::windows::CtrlBreak,
}

impl Signals {
    pub(crate) fn new() -> Result<Self> {
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
                brk: tokio::signal::windows::ctrl_break()?,
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
                _ = self.brk.recv() => {}
            }
        }
    }
}
