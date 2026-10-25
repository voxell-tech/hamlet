//! The server from start to stop: the claim (S2), the harness and
//! the model providers, the published address, then the ordered
//! close (S3). What happens while it serves is the runtime's.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

use crate::connection::GRACE;
use crate::credentials::write_whole;
use crate::harness::{self, Harnesses, InputStatus, Notice};
use crate::protocol::Protocol;
use crate::runtime::{Serving, flush_models};
use crate::service::Service;
use crate::{Home, Overrides, chats};

/// How long a closing home waits for the reflections it owes.
const REFLECTING: Duration = Duration::from_secs(120);
/// How long a closing home waits for its runtimes to stop.
const STOPPING: Duration = Duration::from_secs(30);

/// The server start contract, including the script passed to its
/// harness.
pub struct Options {
    pub home: PathBuf,
    pub port: u16,
    pub faux: Option<PathBuf>,
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
        while let Some(flag) = args.next() {
            let value = args.next().context(
                "usage: hamlet_server [--home <dir>] [--port <n>] \
                 [--faux <file>]",
            )?;
            match flag.to_str() {
                Some("--home") => home = Some(value.into()),
                Some("--port") => {
                    port = value
                        .to_str()
                        .context("port must be a number")?
                        .parse()
                        .context("port must be 0 to 65535")?;
                }
                Some("--faux") => faux = Some(value.into()),
                _ => bail!(
                    "usage: hamlet_server [--home <dir>] [--port \
                     <n>] [--faux <file>]"
                ),
            }
        }
        let home = home
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|p| PathBuf::from(p).join(".hamlet"))
            })
            .context("set --home or HAMLET_HOME")?;
        Ok(Self { home, port, faux })
    }
}

struct Published(PathBuf);
impl Drop for Published {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// An error that ends the server: the home's claim, its store or the
/// client protocol's schema failed.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct Fatal(anyhow::Error);

/// Serves until SIGTERM or Ctrl-C, closing every client before
/// releasing the home.
#[expect(
    clippy::print_stderr,
    reason = "the banner is the server's output for whoever started \
              it, not a diagnostic"
)]
pub async fn serve(options: Options) -> Result<()> {
    let mut signals = Signals::new()?;
    hamlet_os::process::reap_with_server()?;
    let overrides = Overrides::from_env(options.faux.is_some())?;
    let home = Home::open(&options.home).map_err(Fatal)?;
    let mut service =
        Service::new(home, overrides.clone()).map_err(Fatal)?;
    service.houses.probe(service.chats.sandbox)?;
    tokio::select! {
        _ = signals.next() => return service.home.close(),
        initialized = service.initialize_houses() => initialized?,
    }
    service.prepare_gifts()?;
    chats::load(&mut service)?;
    let layout = service.home.layout().clone();
    let (mut harnesses, notices) = Harnesses::start(
        &layout,
        service.home.store(),
        harness::Config::from_env(
            options.faux.clone(),
            overrides,
            &service.houses.shells(),
        )?,
    )?;
    for notice in notices {
        tokio::select! {
            _ = signals.next() => {
                harnesses.close().await;
                return service.home.close();
            }
            result = chats::heard(
                &mut service,
                &mut harnesses,
                notice,
            ) => result?,
        }
    }
    service.start_models()?;
    flush_models(&mut service, &mut harnesses).await?;
    let started = async {
        while service.models.is_starting() {
            let event = harnesses.next().await;
            for notice in harnesses
                .handle(
                    service.home.store(),
                    event,
                    &service.credentials,
                )
                .await?
            {
                match notice {
                    Notice::Provider(result) => {
                        service.provider_result(result)?;
                    }
                    notice => {
                        chats::heard(
                            &mut service,
                            &mut harnesses,
                            notice,
                        )
                        .await?;
                    }
                }
            }
            flush_models(&mut service, &mut harnesses).await?;
        }
        Ok::<_, anyhow::Error>(())
    };
    let started =
        tokio::time::timeout(Duration::from_secs(15), started);
    tokio::select! {
        _ = signals.next() => {
            harnesses.close().await;
            return service.home.close();
        }
        result = started => {
            result.context("starting model providers")??;
        }
    }
    tokio::select! {
        _ = signals.next() => {
            harnesses.close().await;
            return service.home.close();
        }
        result = chats::start(&mut service, &mut harnesses) => {
            result?;
        }
    }
    let protocol = Protocol::new().map_err(Fatal)?;
    let listener = TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        options.port,
    ))
    .await?;
    let url = format!("ws://{}", listener.local_addr()?);
    let published = service.home.layout().root().join("server.json");
    let started = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let address = json!({
        "url": url,
        "pid": std::process::id(),
        "started": started,
    });
    write_whole(&published, &serde_json::to_vec(&address)?)?;
    let published = Published(published);
    eprintln!(
        "hamlet_server: holding {}; serving at {url}; Ctrl-C stops",
        service.home.layout().root().display()
    );
    let (events, mut incoming) = mpsc::channel(128);
    let (shutdown, closing) = watch::channel(false);
    let epoch = crate::clock::now();
    let state = service.state(epoch, 0)?;
    let mut serving = Serving {
        service,
        harnesses,
        protocol,
        clients: HashMap::new(),
        state,
        next: 0,
        events,
        closing,
        tasks: JoinSet::new(),
        reads: JoinSet::new(),
        reading: HashMap::new(),
        writes: JoinSet::new(),
        ready: VecDeque::new(),
    };
    let result =
        serving.run(&mut signals, &listener, &mut incoming).await;
    drop(listener);
    serving.reads.abort_all();
    serving.writes.abort_all();
    let result = match result {
        Ok(()) => serving.close(&mut signals).await,
        result => result,
    };
    serving.service.chats.stop_tools();
    shutdown.send_replace(true);
    let tasks = &mut serving.tasks;
    let drained =
        async { while tasks.join_next().await.is_some() {} };
    tokio::select! {
        _ = signals.next() => std::process::exit(130),
        _ = tokio::time::timeout(GRACE, drained) => {},
    }
    serving.tasks.abort_all();
    while serving.tasks.join_next().await.is_some() {}
    serving.service.houses.end_leftovers();
    serving.harnesses.close().await;
    drop(published);
    serving.service.home.close()?;
    result
}

impl Serving {
    /// Closes in order (S3): the reflections owed are asked for and
    /// waited on, then each runtime stops its work and exits. A
    /// second signal exits at once.
    async fn close(&mut self, signals: &mut Signals) -> Result<()> {
        let waits =
            chats::close(&mut self.service, &mut self.harnesses)
                .await?;
        for client in self.clients.keys() {
            self.service.models.disconnect(*client);
        }
        flush_models(&mut self.service, &mut self.harnesses).await?;
        self.deliver()?;
        let settled = |service: &mut Service| -> Result<bool> {
            for (binding, request) in &waits {
                let standing = service
                    .home
                    .store()
                    .standing(*binding, request)?;
                let open = standing.is_none_or(|standing| {
                    !standing
                        .status
                        .is_some_and(InputStatus::is_ended)
                });
                if open {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        let reflected = async {
            while !settled(&mut self.service)? {
                self.run_once().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::select! {
            _ = signals.next() => std::process::exit(130),
            done = tokio::time::timeout(REFLECTING, reflected) => {
                if let Ok(done) = done {
                    done?;
                }
            }
        }
        self.harnesses.begin_close().await;
        let stopped = async {
            while !self.harnesses.is_down() {
                self.run_once().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::select! {
            _ = signals.next() => std::process::exit(130),
            done = tokio::time::timeout(STOPPING, stopped) => {
                if let Ok(done) = done {
                    done?;
                }
            }
        }
        Ok(())
    }

    /// Handles one event of a runtime or one finished tool call.
    async fn run_once(&mut self) -> Result<()> {
        crate::society::merging::lane::advance(
            &mut self.service,
            &mut self.harnesses,
        )
        .await?;
        tokio::select! {
            event = self.harnesses.next() => {
                self.on_harness(event).await?;
            }
            finished = self.service.chats.next() => {
                self.on_finished(finished).await?;
            }
            Some(work) = self.service.house_work.next() => {
                self.on_work(work).await?;
            }
            Some(merged) = self.service.merges.tasks.join_next() => {
                self.on_merged(merged).await?;
                self.deliver()?;
            }
        }
        Ok(())
    }
}

pub(crate) struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
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
            self.interrupt.recv().await;
        }
    }
}
