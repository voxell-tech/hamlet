//! A host that runs counter actors in the realm folder `HAMLET_HOME`.
//!
//! ```text
//! demo [--name <name>] [--kind <kind>]... [--link] [--realm-actor]
//!     <command> ...
//!
//! --name <name>                 the host's name, `host` by default
//! --kind <kind>                 a kind the host runs, as many as
//!                               given. `workspace`, `profile` and
//!                               `library` are those actors, and
//!                               `realm` and `agent` the stand-ins
//!                               of demo/realm.rs and demo/agent.rs;
//!                               any other name is a counter kind.
//!                               With none given the host runs
//!                               `counter`, `workspace`, `profile`,
//!                               `library`, `realm` and `agent`, or
//!                               with --link `realm` alone
//! --link                        run the agents in a Node host, this
//!                               host's child, and wait for it
//!                               before the first command. Its
//!                               commands, and the route table the
//!                               host then uses whatever --kind
//!                               says, are in demo/link.rs
//! --realm-actor                 run the realm actor with a stand-in
//!                               agent and profile, in place of the
//!                               stub realm and the stub agent; not
//!                               with --link
//!
//! demo increment <key> [times]  add one per time, each in a transaction
//! demo get <key>                print the count
//! demo schedule <key> <ms> [<action>]
//!                               arm a one-shot of <action>, or of
//!                               increment, <ms> ahead
//! demo sleep <key>              ask the actor to sleep
//! demo destroy <key>            ask the actor to destroy itself
//! demo block <key> <ms>         block the actor's event loop <ms> in
//!                               synchronous code
//! demo call <key> <to>          have the counter <key> call the actor
//!                               <to> as itself; make <to> first, as
//!                               with `get`
//! demo calls [<key> <to>] [--count <n>]
//!                               have the counter <key>, `a` by
//!                               default, call the counter <to>, `b` by
//!                               default, <n> times, 1000 by default
//! demo timers <key>             print the actor's set-aside timers
//! demo rearm <key> <event_id>   put a set-aside timer back, due now
//! demo boot <ms>                boot, wait <ms>, print what booting started
//! demo serve [<n>]              boot, start <n> counters that never
//!                               sleep, then run one command per line
//!                               of stdin until it ends or SIGTERM
//! demo realm ...                the stub realm's commands, in
//!                               demo/realm.rs
//! demo agent ...                the stub agent's commands, in
//!                               demo/agent.rs
//! demo watch <n> <event>        follow <event> of agent <n> as a
//!                               client, in demo/watch.rs
//! demo profile ...              a profile's commands, in
//!                               demo/profile.rs; not in serve
//! demo library ...              the library's commands, in
//!                               demo/library.rs; not in serve
//! demo --realm-actor realm|agent ...
//!                               the realm actor's commands, in
//!                               demo/realm.rs
//! ```
//!
//! A `<key>` names a counter of the first counter kind, and
//! `<kind>/<key>` an actor of any kind. A command on a counter makes
//! it first if there is none, except `timers` and `rearm`, which then
//! fail. `call` makes only its caller: a call to a `<to>` with no
//! actor fails with `actor.not_found` and makes none. Each command
//! prints `<kind>/<key> <count>`, the count the actor replied with.
//! Booting prints `boot <kind>/<key> <count>` for each actor it
//! started, once its start ends. `timers` prints `timer <kind>/<key>
//! <event_id> <action> due=<ms> tries=<n> error=<code>` for each
//! timer, and `rearm` prints `rearm <kind>/<key> <event_id>
//! <true|false>`. `call` prints `call <from> <to> <n> <us>us`, where
//! `<n>` counts the calls `<to>` had from `<from>`, and `calls`
//! prints `calls <from> <to> count=<n> p50=<us>us p99=<us>us
//! from=<n>`, the times of the calls measured in the calling counter.
//! A counter that is called logs its caller. An error is printed with
//! the `group.code` of the actor's error it holds first, as
//! `actor.not_found`. Logs go to standard error. The demo exits once
//! its host has shut down, also when an actor it could not stop still
//! blocks a thread.

#[path = "demo/act.rs"]
mod act;
#[path = "demo/agent.rs"]
mod agent;
#[path = "demo/command.rs"]
mod command;
#[path = "demo/counter.rs"]
mod counter;
#[path = "demo/kinds.rs"]
mod kinds;
#[path = "demo/library.rs"]
mod library;
#[path = "demo/link.rs"]
mod link;
#[path = "demo/profile.rs"]
mod profile;
#[path = "demo/realm.rs"]
mod realm;
#[path = "demo/tell.rs"]
mod tell;
#[path = "demo/watch.rs"]
mod watch;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use act::act;
use anyhow::{Context, Result, anyhow, bail};
use command::{counter_kind, number, print_reply, run, shown};
use hamlet_host::{Address, Host, HostOptions};
use hamlet_messages::address::{Party, Workspace};
use hamlet_sandbox::{Isolation, Sandbox};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let code = match demo().await {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(
                std::io::stderr(),
                "Error: {}",
                shown(&error)
            );
            1
        }
    };
    // A runtime's drop waits for its threads, and an actor whose stop
    // missed its deadline may block one for good.
    std::process::exit(code);
}

async fn demo() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("warn,demo=info"))
        .with_ansi(std::io::stderr().is_terminal())
        .with_writer(std::io::stderr)
        .init();
    let home = std::env::var_os("HAMLET_HOME")
        .map(PathBuf::from)
        .context("set HAMLET_HOME to the realm folder")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (start, args) = options(&args)?;
    let command = args.first().map(String::as_str);
    let loaded = match command {
        Some("serve") => {
            args.get(1).map(|n| number(Some(n))).transpose()?
        }
        _ => None,
    };
    let sleep = if loaded.is_some() {
        counter::Sleep::Never
    } else {
        counter::Sleep::WhenIdle
    };
    let mut factories = kinds::factories(&start.kinds, sleep);
    if start.realm_actor {
        factories.extend(realm::actor_kinds());
    }
    // A profile's and the library's commands call as stand-ins, in a
    // process of their own: `serve` keeps the stub realm and agents.
    if matches!(command, Some("profile" | "library")) {
        factories.extend(profile::stand_ins());
    }
    // The workspace does not start where there is no sandbox.
    let options = match Sandbox::open(Isolation::Sandboxed) {
        Ok(sandbox) => start.options.with_value(Arc::new(sandbox)),
        Err(_) => start.options,
    };
    let (host, link) = if start.link {
        let (host, link) = link::open(&home, factories, options)?;
        (host, Some(link))
    } else {
        (Host::open_with(&home, factories, options)?, None)
    };
    let demo = Demo {
        host,
        link,
        counters: kinds::counters(&start.kinds),
        realm_actor: start.realm_actor,
        watches: Mutex::default(),
    };
    let result = match command {
        Some("serve") => serve(&demo, loaded.unwrap_or(0)).await,
        Some("boot") => {
            async {
                let ms = number(args.get(1))?;
                let began = demo.host.boot().await?;
                tokio::time::sleep(Duration::from_millis(ms)).await;
                print_began(&demo.host, began).await
            }
            .await
        }
        Some("profile") => {
            async {
                demo.host.boot().await?;
                profile::run(&demo.host, &args[1..]).await
            }
            .await
        }
        Some("library") => {
            async {
                demo.host.boot().await?;
                library::run(&demo.host, &args[1..]).await
            }
            .await
        }
        Some(_) => {
            async {
                demo.host.boot().await?;
                demo.ready(std::future::pending()).await?;
                run_line(&demo, args).await
            }
            .await
        }
        None => Err(anyhow!(
            "usage: demo [--name <name>] [--kind <kind>]... [--link] \
             [--realm-actor] increment|get|schedule|sleep|destroy|\
             block|call|calls|timers|rearm|boot|serve|realm|agent|link|\
             make|tell|heard|watch ..."
        )),
    };
    demo.shutdown().await?;
    result
}

pub struct Demo {
    pub host: Host,
    pub link: Option<link::Link>,
    /// The counter kinds the host runs.
    counters: Vec<String>,
    /// Whether `realm` and `agent` lines drive the realm actor.
    realm_actor: bool,
    pub watches: Mutex<JoinSet<()>>,
}

impl Demo {
    /// Waits for the Node host, where the demo has one, until `stop`
    /// ends.
    async fn ready(
        &self,
        stop: impl Future<Output = ()>,
    ) -> Result<()> {
        let Some(link) = &self.link else {
            return Ok(());
        };
        tokio::select! {
            ready = link.ready() => ready,
            () = stop => bail!(
                "the Node host was not ready when the demo was stopped"
            ),
        }
    }

    async fn shutdown(self) -> Result<()> {
        if let Some(link) = self.link {
            link.shutdown().await;
        }
        let (shut, _) = tokio::join!(
            self.host.shutdown(),
            self.watches.into_inner().join_all()
        );
        shut
    }
}

/// The options before the command.
struct Start {
    options: HostOptions,
    /// The kinds the host runs, in the order given.
    kinds: Vec<String>,
    /// Whether the host runs the agents in a Node child.
    link: bool,
    realm_actor: bool,
}

async fn run_line(demo: &Demo, words: &[String]) -> Result<()> {
    match words.split_first() {
        Some((first, _))
            if demo.realm_actor
                && (first == "realm" || first == "agent") =>
        {
            realm::drive(&demo.host, words).await
        }
        Some((first, rest)) if first == "realm" => {
            realm::run(&demo.host, rest).await
        }
        Some((first, rest)) if first == "agent" => {
            agent::run(&demo.host, rest).await
        }
        Some((first, rest)) if first == "watch" => {
            watch::run(demo, rest).await
        }
        Some((first, rest))
            if link::COMMANDS.contains(&first.as_str()) =>
        {
            link::run(demo, first, rest).await
        }
        _ => run(&demo.host, &demo.counters, words).await,
    }
}

/// The options before the command, and the words from the command
/// on.
fn options(args: &[String]) -> Result<(Start, &[String])> {
    let mut options = HostOptions::default();
    let mut kinds = Vec::new();
    let mut link = false;
    let mut realm_actor = false;
    let mut rest = args;
    loop {
        match rest {
            [option, more @ ..] if option == "--realm-actor" => {
                realm_actor = true;
                rest = more;
            }
            [option, value, more @ ..] if option == "--name" => {
                options.name.clone_from(value);
                rest = more;
            }
            [option, value, more @ ..] if option == "--kind" => {
                kinds.push(value.clone());
                rest = more;
            }
            [option, more @ ..] if option == "--link" => {
                link = true;
                rest = more;
            }
            [option, ..] if option.starts_with("--") => {
                bail!(
                    "`{option}` is not an option, or lacks its value"
                )
            }
            _ => break,
        }
    }
    if link && realm_actor {
        bail!(
            "--link and --realm-actor do not go together: the Node \
             host and the realm actor's stand-in agent would both \
             run the kind `agent`"
        );
    }
    if kinds.is_empty() {
        let default = if link { kinds::LINKED } else { kinds::LOCAL };
        kinds =
            default.iter().map(|kind| (*kind).to_owned()).collect();
    }
    // The realm actor calls the workspace.
    if realm_actor
        && !kinds.iter().any(|kind| kind == Workspace::KIND)
    {
        kinds.push(Workspace::KIND.to_owned());
    }
    Ok((
        Start {
            options,
            kinds,
            link,
            realm_actor,
        },
        rest,
    ))
}

/// Boots, waits for the Node host where there is one, starts
/// `loaded` counters of the first counter kind, then runs each line
/// of stdin as a command until stdin ends or the process gets
/// SIGTERM or Ctrl-C, also while a command runs.
async fn serve(demo: &Demo, loaded: u64) -> Result<()> {
    let host = &demo.host;
    print_began(host, host.boot().await?).await?;
    // One for the whole of serve, so no signal falls between two
    // waits.
    let mut stop = std::pin::pin!(stop_signal());
    demo.ready(&mut stop).await?;
    if loaded > 0 {
        let kind = counter_kind(&demo.counters)?;
        for n in 1..=loaded {
            let at = Address::new(kind, format!("load-{n}"))?;
            host.get_or_start(&at).await?;
        }
        writeln!(std::io::stdout(), "loaded {loaded}")?;
    }
    // A thread of its own reads stdin, so a stop never waits for a
    // read that may never return.
    let (tx, mut lines) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { return };
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    loop {
        let line = tokio::select! {
            line = lines.recv() => line,
            () = &mut stop => None,
        };
        let Some(line) = line else { return Ok(()) };
        let words: Vec<String> =
            line.split_whitespace().map(str::to_owned).collect();
        if words.is_empty() {
            continue;
        }
        let ran = tokio::select! {
            ran = run_line(demo, &words) => ran,
            () = &mut stop => return Ok(()),
        };
        if let Err(error) = ran {
            writeln!(std::io::stdout(), "error: {}", shown(&error))?;
        }
    }
}

async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = term.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// Prints the count of each actor whose start boot began, once the
/// start ends. A start that failed is logged and printed as nothing.
async fn print_began(host: &Host, began: Vec<Address>) -> Result<()> {
    for at in began {
        match act(host, &at, "get", Vec::new()).await {
            Ok(reply) => print_reply("boot ", &at, reply)?,
            Err(error) => tracing::error!(
                ?error,
                actor_id = %at,
                "the start boot began failed"
            ),
        }
    }
    Ok(())
}
