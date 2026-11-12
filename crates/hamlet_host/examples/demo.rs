//! A host that runs counter actors in the realm folder `HAMLET_HOME`.
//!
//! ```text
//! demo increment <key> [times]  add one per time, each in a transaction
//! demo get <key>                print the count
//! demo schedule <key> <ms>      arm a one-shot increment <ms> ahead
//! demo boot <ms>                boot, wait <ms>, print what booting started
//! demo serve [<n>]              boot, start <n> counters that never
//!                               sleep, then run one command per line
//!                               of stdin until it ends or SIGTERM
//! ```
//!
//! Each command prints `counter/<key> <count>`, the count the actor
//! replied with. Booting prints `boot counter/<key> <count>` for each
//! actor it started.

#[path = "demo/counter.rs"]
mod counter;

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use hamlet_host::Host;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
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
    let host = Host::open(&home, counter::factories(sleep))?;
    let result = match command {
        Some("serve") => serve(&host, loaded.unwrap_or(0)).await,
        Some("boot") => {
            let ms = number(args.get(1))?;
            let started = host.boot().await?;
            tokio::time::sleep(Duration::from_millis(ms)).await;
            for actor_id in started {
                print_count(&host, "boot ", &actor_id).await?;
            }
            Ok(())
        }
        Some(_) => {
            host.boot().await?;
            run(&host, &args).await
        }
        None => Err(anyhow!(
            "usage: demo increment|get|schedule|boot|serve ..."
        )),
    };
    host.stop_all().await?;
    result
}

/// Runs one command line split into words.
async fn run(host: &Host, words: &[String]) -> Result<()> {
    let (command, key) = match words {
        [command, key, ..] => (command.as_str(), key.as_str()),
        _ => bail!("a command takes an actor key"),
    };
    let actor_id = host.get_or_start("counter", key).await?;
    let reply = match command {
        "increment" => {
            let mut reply = Vec::new();
            for _ in 0..number(words.get(2)).unwrap_or(1) {
                reply = host
                    .action(&actor_id, "increment", Vec::new())
                    .await?;
            }
            reply
        }
        "get" => host.action(&actor_id, "get", Vec::new()).await?,
        "schedule" => {
            let ms = number(words.get(2))?;
            host.action(
                &actor_id,
                "schedule",
                ms.to_le_bytes().to_vec(),
            )
            .await?
        }
        other => bail!("unknown command `{other}`"),
    };
    print_reply("", &actor_id, reply)
}

/// Boots, starts `loaded` counters, then runs each line of stdin as
/// a command until stdin ends or the process gets SIGTERM or Ctrl-C.
async fn serve(host: &Host, loaded: u64) -> Result<()> {
    for actor_id in host.boot().await? {
        print_count(host, "boot ", &actor_id).await?;
    }
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
    loop {
        let line = tokio::select! {
            line = lines.recv() => line,
            () = stop_signal() => None,
        };
        let Some(line) = line else { return Ok(()) };
        let words: Vec<String> =
            line.split_whitespace().map(str::to_owned).collect();
        if words.is_empty() {
            continue;
        }
        if let Err(error) = run(host, &words).await {
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

async fn print_count(
    host: &Host,
    prefix: &str,
    actor_id: &str,
) -> Result<()> {
    let reply = host.action(actor_id, "get", Vec::new()).await?;
    print_reply(prefix, actor_id, reply)
}

/// Prints the count a counter replied with.
fn print_reply(
    prefix: &str,
    actor_id: &str,
    reply: Vec<u8>,
) -> Result<()> {
    let count =
        i64::from_le_bytes(reply.try_into().map_err(|_| {
            anyhow!("the counter replied with no count")
        })?);
    writeln!(std::io::stdout(), "{prefix}{actor_id} {count}")?;
    Ok(())
}

fn number(word: Option<&String>) -> Result<u64> {
    let word = word.context("a number is missing")?;
    word.parse()
        .with_context(|| format!("`{word}` is not a number"))
}
