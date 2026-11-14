//! A host that runs counter actors in the realm folder `HAMLET_HOME`.
//!
//! ```text
//! demo [--name <name>] [--kind <kind>]... <command> ...
//!
//! --name <name>                 the host's name, `host` by default
//! --kind <kind>                 a counter kind the host runs, as many
//!                               as given, `counter` by default
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
//!                               <to> as itself
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
//! ```
//!
//! A `<key>` names a counter of the first kind, and `<kind>/<key>` an
//! actor of any kind. A command on a counter makes it first if it has
//! none. Each command prints `<kind>/<key> <count>`, the count the
//! actor replied with. Booting prints `boot <kind>/<key> <count>` for
//! each actor it started, once its start ends. `timers` prints `timer
//! <kind>/<key> <event_id> <action> due=<ms> tries=<n> error=<code>`
//! for each timer, and `rearm` prints `rearm <kind>/<key> <event_id>
//! <true|false>`. `call` prints `call <from> <to> <n> <us>us`, where
//! `<n>` counts the calls `<to>` had from `<from>`, and `calls`
//! prints `calls <from> <to> count=<n> p50=<us>us p99=<us>us
//! from=<n>`, the times of the calls measured in the calling counter.
//! A counter that is called logs its caller. Logs go to standard
//! error. The demo exits once its host has shut down, also when an
//! actor it could not stop still blocks a thread.

#[path = "demo/act.rs"]
mod act;
#[path = "demo/command.rs"]
mod command;
#[path = "demo/counter.rs"]
mod counter;
#[path = "demo/realm.rs"]
mod realm;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use act::act;
use anyhow::{Context, Result, anyhow, bail};
use command::{number, print_reply, run};
use hamlet_actors::workspace;
use hamlet_host::{Host, HostOptions};
use hamlet_sandbox::{Isolation, Sandbox};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let code = match demo().await {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "Error: {error:?}");
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
    let (options, kinds, args) = options(&args)?;
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
    let mut factories = counter::factories(
        &kinds.iter().map(String::as_str).collect::<Vec<_>>(),
        sleep,
    );
    factories
        .insert(workspace::KIND.to_owned(), workspace::factory());
    factories.insert(workspace::REALM.to_owned(), realm::factory());
    // The workspace does not start where there is no sandbox.
    let options = match Sandbox::open(Isolation::Sandboxed) {
        Ok(sandbox) => options.with_value(Arc::new(sandbox)),
        Err(_) => options,
    };
    let host = Host::open_with(&home, factories, options)?;
    let result = match command {
        Some("serve") => {
            serve(&host, &kinds, loaded.unwrap_or(0)).await
        }
        Some("boot") => {
            let ms = number(args.get(1))?;
            let began = host.boot().await?;
            tokio::time::sleep(Duration::from_millis(ms)).await;
            print_began(&host, began).await
        }
        Some(_) => {
            host.boot().await?;
            run_line(&host, &kinds, args).await
        }
        None => Err(anyhow!(
            "usage: demo [--name <name>] [--kind <kind>]... \
             increment|get|schedule|sleep|destroy|block|call|calls|\
             timers|rearm|boot|serve ..."
        )),
    };
    host.shutdown().await?;
    result
}

/// Runs one command line split into words: the stub realm's, or one
/// for the counters of `kinds`.
async fn run_line(
    host: &Host,
    kinds: &[String],
    words: &[String],
) -> Result<()> {
    match words.split_first() {
        Some((first, rest)) if first == "realm" => {
            realm::run(host, rest).await
        }
        _ => run(host, kinds, words).await,
    }
}

/// The host's options and its counter kinds, read from the options
/// before the command, and the words from the command on.
fn options(
    args: &[String],
) -> Result<(HostOptions, Vec<String>, &[String])> {
    let mut options = HostOptions::default();
    let mut kinds = Vec::new();
    let mut rest = args;
    loop {
        match rest {
            [option, value, more @ ..] if option == "--name" => {
                options.name.clone_from(value);
                rest = more;
            }
            [option, value, more @ ..] if option == "--kind" => {
                kinds.push(value.clone());
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
        kinds.push("counter".to_owned());
    }
    Ok((options, kinds, rest))
}

/// Boots, starts `loaded` counters of the first of `kinds`, then runs
/// each line of stdin as a command until stdin ends or the process
/// gets SIGTERM or Ctrl-C, also while a command runs.
async fn serve(
    host: &Host,
    kinds: &[String],
    loaded: u64,
) -> Result<()> {
    print_began(host, host.boot().await?).await?;
    if loaded > 0 {
        for n in 1..=loaded {
            host.get_or_start(&kinds[0], &format!("load-{n}"))
                .await?;
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
    // One for the whole loop, so no signal falls between two.
    let mut stop = std::pin::pin!(stop_signal());
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
            ran = run_line(host, kinds, &words) => ran,
            () = &mut stop => return Ok(()),
        };
        if let Err(error) = ran {
            writeln!(std::io::stdout(), "error: {error:#}")?;
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
async fn print_began(host: &Host, began: Vec<String>) -> Result<()> {
    for actor_id in began {
        match act(host, &actor_id, "get", Vec::new()).await {
            Ok(reply) => print_reply("boot ", &actor_id, reply)?,
            Err(error) => tracing::error!(
                ?error,
                actor_id,
                "the start boot began failed"
            ),
        }
    }
    Ok(())
}
