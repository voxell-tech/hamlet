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
//! --link                        run the agents of demo.ts in a Node
//!                               host, this host's child, and wait
//!                               for it before the first command.
//!                               Its commands, and the route table
//!                               the host then uses whatever --kind
//!                               says, are in demo/link.rs
//! --realm-actor                 run the realm actor, the workspace,
//!                               the profiles and the library, with
//!                               the real agent in a Node host, this
//!                               host's child, in place of the stub
//!                               realm and the stub agent, and wait
//!                               for the Node host before the first
//!                               command; with or without --link. Its
//!                               route table, used whatever --kind
//!                               says, and its `pi` commands are in
//!                               demo/agent_host.rs
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
//! demo --realm-actor realm|agent ...
//!                               the realm actor's commands, in
//!                               demo/realm_actor.rs
//! demo --realm-actor pi ...     the real agent's commands, in
//!                               demo/agent_host.rs
//! demo --realm-actor profile ...
//!                               a profile's commands, in
//!                               demo/profile.rs
//! demo --realm-actor library ...
//!                               the library's commands, in
//!                               demo/library.rs
//! ```
//!
//! A `<key>` names a counter of the first counter kind, and
//! `<kind>/<key>` an actor of any kind. A command on a counter makes
//! it first if there is none, except `timers` and `rearm`, which then
//! fail. `call` makes only its caller: a call to a `<to>` with no
//! actor fails with `actor.not_found` and makes none. Each command
//! prints `<kind>/<key> <count>`, the count the actor replied with.
//! Booting prints `boot <kind>/<key> <count>` for each counter it
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
#[path = "demo/agent_host.rs"]
mod agent_host;
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
#[path = "demo/realm_actor.rs"]
mod realm_actor;
#[path = "demo/tell.rs"]
mod tell;
#[path = "demo/watch.rs"]
mod watch;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use act::act;
use anyhow::{Context, Result, anyhow, bail, ensure};
use command::{counter_kind, number, print_reply, run, shown};
use hamlet_host::{Address, Host, HostOptions};
use hamlet_messages::address::{
    Agent, Library, Party, Profile, Realm, Workspace,
};
use hamlet_messages::workspace::AgentNumber;
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
    let mut options = start.options;
    if start.realm_actor {
        factories.extend(realm_actor::kinds());
        factories.remove(Agent::KIND);
        options =
            options.with_value(Arc::new(realm_actor::options()));
    }
    // The workspace does not start where there is no sandbox.
    let options = match Sandbox::open(Isolation::Sandboxed) {
        Ok(sandbox) => options.with_value(Arc::new(sandbox)),
        Err(_) => options,
    };
    let (host, node) = if start.realm_actor {
        let (host, node) =
            agent_host::open(&home, factories, options)?;
        (host, Some(node))
    } else if start.link {
        let (host, node) = link::open(&home, factories, options)?;
        (host, Some(node))
    } else {
        (Host::open_with(&home, factories, options)?, None)
    };
    let demo = Demo {
        host,
        node,
        home,
        counters: kinds::counters(&start.kinds),
        realm_actor: start.realm_actor,
        watches: Mutex::default(),
    };
    let result = match command {
        Some("serve") => serve(&demo, loaded.unwrap_or(0)).await,
        Some("boot") => {
            async {
                let ms = number(args.get(1))?;
                let began = demo.started().await?;
                tokio::time::sleep(Duration::from_millis(ms)).await;
                print_began(&demo, began).await
            }
            .await
        }
        Some(_) => {
            async {
                demo.started().await?;
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
    pub node: Option<link::NodeHost>,
    /// The realm folder.
    pub home: PathBuf,
    /// The counter kinds the host runs.
    counters: Vec<String>,
    /// Whether the host runs the realm actor and the real agent.
    pub realm_actor: bool,
    pub watches: Mutex<JoinSet<()>>,
}

impl Demo {
    /// Waits for the Node host, where the demo has one, until `stop`
    /// ends.
    async fn ready(
        &self,
        stop: impl Future<Output = ()>,
    ) -> Result<()> {
        let ready = async {
            match &self.node {
                Some(node) => node.ready().await,
                None => Ok(()),
            }
        };
        tokio::select! {
            ready = ready => ready,
            () = stop => bail!(
                "the Node host was not ready when the demo was stopped"
            ),
        }
    }

    /// Boots the host. The realm actor starts first, after the
    /// workspace and the library it and its agents call, and its Hand
    /// is live before anything else, so no agent wakes before its
    /// realm and nothing reaches the Hand before its actor is made.
    async fn start(&self) -> Result<Vec<Address>> {
        if self.realm_actor {
            self.host.get_or_start(&Workspace.address()?).await?;
            self.host.get_or_start(&Library.address()?).await?;
            self.host.get_or_start(&Realm.address()?).await?;
            ensure!(
                realm_actor::live(&self.host, AgentNumber(1)).await?,
                "the realm's Hand was not live in time"
            );
        }
        self.host.boot().await
    }

    /// Waits for the Node host, then boots as [`Self::start`] does.
    async fn started(&self) -> Result<Vec<Address>> {
        self.ready(std::future::pending()).await?;
        self.start().await
    }

    async fn shutdown(self) -> Result<()> {
        if let Some(node) = self.node {
            node.shutdown().await;
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
    /// Whether the host runs the agents of demo.ts in a Node child.
    link: bool,
    realm_actor: bool,
}

async fn run_line(demo: &Demo, words: &[String]) -> Result<()> {
    match words.split_first() {
        Some((first, _))
            if demo.realm_actor
                && (first == "realm" || first == "agent") =>
        {
            realm_actor::drive(&demo.host, &demo.home, words).await
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
        Some((first, rest)) if first == "pi" => {
            agent_host::run(demo, rest).await
        }
        Some((first, rest))
            if demo.realm_actor && first == "profile" =>
        {
            profile::run(&demo.host, rest).await
        }
        Some((first, rest))
            if demo.realm_actor && first == "library" =>
        {
            library::run(&demo.host, rest).await
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
    if kinds.is_empty() {
        let default = if link { kinds::LINKED } else { kinds::LOCAL };
        kinds =
            default.iter().map(|kind| (*kind).to_owned()).collect();
    }
    let mut needed = Vec::new();
    if realm_actor {
        needed.extend([
            Workspace::KIND,
            Profile::KIND,
            Library::KIND,
        ]);
    }
    if let Some(verb @ ("profile" | "library")) =
        rest.first().map(String::as_str)
        && !realm_actor
    {
        bail!(
            "`{verb}` needs the real realm and agent: start the \
             demo with --realm-actor"
        );
    }
    for kind in needed {
        if !kinds.iter().any(|named| named == kind) {
            kinds.push(kind.to_owned());
        }
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

/// Waits for the Node host where there is one, boots, starts
/// `loaded` counters of the first counter kind, then runs each line
/// of stdin as a command until stdin ends or the process gets
/// SIGTERM or Ctrl-C, also while a command runs.
async fn serve(demo: &Demo, loaded: u64) -> Result<()> {
    let host = &demo.host;
    // One for the whole of serve, so no signal falls between two
    // waits.
    let mut stop = std::pin::pin!(stop_signal());
    demo.ready(&mut stop).await?;
    print_began(demo, demo.start().await?).await?;
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

/// Prints the count of each counter whose start boot began, once the
/// start ends. A start that failed is logged and printed as nothing.
async fn print_began(demo: &Demo, began: Vec<Address>) -> Result<()> {
    let counters = began.into_iter().filter(|at| {
        demo.counters.iter().any(|kind| kind == at.kind())
    });
    for at in counters {
        match act(&demo.host, &at, "get", Vec::new()).await {
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
