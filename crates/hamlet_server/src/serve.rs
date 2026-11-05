//! The server from start to stop: the claim (S2), the harness, the
//! published address, then the ordered close (S3).

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_sandbox::{Isolation, Sandbox};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::clients::connection::GRACE;
use crate::core::Core;
use crate::credentials::write_whole;
use crate::realm::Realm;
use crate::settings::{self, Overrides};
use crate::talk::Env;
use crate::talk::process::Pi;

/// How long a closing realm waits for its runtimes to stop.
const STOPPING: Duration = Duration::from_secs(30);

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
struct Published(PathBuf);

impl Drop for Published {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The name of the account the server runs as, which is its chief's
/// (C7).
fn chief_name() -> String {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|name| {
            std::env::var(name).ok().filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| "chief".into())
}

/// Serves until SIGTERM or Ctrl-C, closing every client before
/// letting the realm go.
#[expect(
    clippy::print_stderr,
    reason = "the banner is the server's output for whoever started \
              it"
)]
pub async fn serve(options: Options) -> Result<()> {
    let mut signals = Signals::new()?;
    hamlet_os::process::reap_with_server()?;
    let sandbox = Sandbox::open(options.isolation)?;
    sandbox.probe()?;
    let realm = Realm::open(
        &options.home,
        &chief_name(),
        crate::clock::now(),
    )?;
    let overrides = Overrides::from_env(options.faux.is_some());
    let mut realm = realm;
    let saved = settings::stored(realm.store().connection())?;
    let env = Env {
        settings: overrides.apply(saved),
        overrides,
    };
    let pi = Pi::from_env(options.faux.clone())?;
    let (shutdown, closing) = watch::channel(false);
    let (mut core, mut inbox) =
        Core::start(realm, env, pi, sandbox, closing)?;
    let listener = TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        options.port,
    ))
    .await?;
    let url = format!("ws://{}", listener.local_addr()?);
    inbox.listen(listener);
    let published = core.realm.layout().server();
    let started = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let address = json!({
        "url": url,
        "pid": std::process::id(),
        "started": started,
        "confinement": core.confinement(),
    });
    write_whole(&published, &serde_json::to_vec(&address)?)?;
    let published = Published(published);
    eprintln!(
        "hamlet_server: holding {}; serving at {url}; Ctrl-C stops",
        hamlet_os::fs::path::text(core.realm.layout().root())
    );
    let served = async {
        loop {
            core.step(&mut inbox, Duration::from_secs(60)).await?;
        }
        #[expect(
            unreachable_code,
            reason = "the loop ends with an error or a signal"
        )]
        Ok::<_, anyhow::Error>(())
    };
    let result = tokio::select! {
        result = served => result,
        () = signals.next() => Ok(()),
    };
    shutdown.send_replace(true);
    let tasks = &mut core.clients.tasks;
    let drained =
        async { while tasks.join_next().await.is_some() {} };
    tokio::select! {
        () = signals.next() => std::process::exit(130),
        _ = tokio::time::timeout(GRACE, drained) => {}
    }
    core.clients.tasks.abort_all();
    let stopped = core.stop(STOPPING);
    tokio::select! {
        () = signals.next() => std::process::exit(130),
        stopped = stopped => stopped?,
    }
    drop(published);
    result
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
