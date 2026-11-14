//! A host that runs counter actors in the realm folder `HAMLET_HOME`.
//!
//! ```text
//! demo increment <key> [times]  add one per time, each in a transaction
//! demo get <key>                print the count
//! demo schedule <key> <ms> [<action>]
//!                               arm a one-shot of <action>, or of
//!                               increment, <ms> ahead
//! demo sleep <key>              ask the actor to sleep
//! demo destroy <key>            ask the actor to destroy itself
//! demo block <key> <ms>         block the actor's event loop <ms> in
//!                               synchronous code
//! demo timers <kind>/<key>      print the actor's set-aside timers
//! demo rearm <kind>/<key> <event_id>
//!                               put a set-aside timer back, due now
//! demo boot <ms>                boot, wait <ms>, print what booting started
//! demo serve [<n>]              boot, start <n> counters that never
//!                               sleep, then run one command per line
//!                               of stdin until it ends or SIGTERM
//! demo realm ...                the stub realm's commands, in
//!                               demo/realm.rs
//! ```
//!
//! A command on a counter makes it first if it has none. Each command
//! prints `counter/<key> <count>`, the count the actor replied with.
//! Booting prints `boot counter/<key> <count>` for each
//! actor it started, once its start ends. `timers` prints `timer
//! <kind>/<key> <event_id> <action> due=<ms> tries=<n> error=<code>`
//! for each timer, and `rearm` prints `rearm <kind>/<key> <event_id>
//! <true|false>`. Warnings and errors go to standard error. The
//! demo exits once its host has shut down, also when an actor it
//! could not stop still blocks a thread.

#[path = "demo/act.rs"]
mod act;
#[path = "demo/command.rs"]
mod command;
#[path = "demo/counter.rs"]
mod counter;
#[path = "demo/realm.rs"]
mod realm;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use act::act;
use anyhow::{Context, Result, anyhow};
use command::{number, print_reply, run};
use hamlet_actors::workspace;
use hamlet_host::{Host, HostOptions};
use hamlet_sandbox::{Isolation, Sandbox};
use tokio::sync::mpsc;

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
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::io::stderr)
        .init();
    let home = std::env::var_os("HAMLET_HOME")
        .map(PathBuf::from)
        .context("set HAMLET_HOME to the realm folder")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
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
    let mut kinds = counter::factories(sleep);
    kinds.insert(workspace::KIND.to_owned(), workspace::factory());
    kinds.insert(workspace::REALM.to_owned(), realm::factory());
    // The workspace does not start where there is no sandbox.
    let mut options = HostOptions::default();
    if let Ok(sandbox) = Sandbox::open(Isolation::Sandboxed) {
        options = options.with_value(Arc::new(sandbox));
    }
    let host = Host::open_with(&home, kinds, options)?;
    let result = match command {
        Some("serve") => serve(&host, loaded.unwrap_or(0)).await,
        Some("boot") => {
            let ms = number(args.get(1))?;
            let began = host.boot().await?;
            tokio::time::sleep(Duration::from_millis(ms)).await;
            print_began(&host, began).await
        }
        Some(_) => {
            host.boot().await?;
            run(&host, &args).await
        }
        None => Err(anyhow!(
            "usage: demo \
             increment|get|schedule|sleep|destroy|block|timers|rearm|\
             boot|serve ..."
        )),
    };
    host.shutdown().await?;
    result
}

/// Boots, starts `loaded` counters, then runs each line of stdin as
/// a command until stdin ends or the process gets SIGTERM or Ctrl-C,
/// also while a command runs.
async fn serve(host: &Host, loaded: u64) -> Result<()> {
    print_began(host, host.boot().await?).await?;
    if loaded > 0 {
        for n in 1..=loaded {
            host.get_or_start("counter", &format!("load-{n}"))
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
            ran = run(host, &words) => ran,
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
