//! One command line of the demo, run on its host.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hamlet_host::{Address, Host};

use crate::act::act;
use crate::counter;

/// Runs one command line split into words.
pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    let (command, key) = match words {
        [command, key, ..] => (command.as_str(), key.as_str()),
        _ => bail!("a command takes an actor key"),
    };
    let mut out = std::io::stdout();
    match command {
        "timers" => {
            let at = address(key)?;
            for timer in host.set_aside_timers(&at).await? {
                writeln!(
                    out,
                    "timer {key} {} {} due={} tries={} error={}",
                    timer.event_id,
                    timer.action,
                    timer.due_at,
                    timer.tries,
                    timer.last_error.as_deref().unwrap_or("-")
                )?;
            }
            return Ok(());
        }
        "rearm" => {
            let event_id =
                words.get(2).context("rearm takes an event id")?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
            let rearmed = host
                .rearm_timer(
                    &address(key)?,
                    event_id,
                    i64::try_from(now.as_millis())?,
                )
                .await?;
            writeln!(out, "rearm {key} {event_id} {rearmed}")?;
            return Ok(());
        }
        _ => {}
    }
    // Read before the counter is made, so that a line the demo
    // refuses makes none.
    let (args, times) = match command {
        "increment" => {
            (Vec::new(), number(words.get(2)).unwrap_or(1))
        }
        "get" | "sleep" | "destroy" => (Vec::new(), 1),
        "block" => (number(words.get(2))?.to_le_bytes().to_vec(), 1),
        "schedule" => {
            let mut args =
                number(words.get(2))?.to_le_bytes().to_vec();
            if let Some(action) = words.get(3) {
                args.extend_from_slice(action.as_bytes());
            }
            (args, 1)
        }
        other => bail!("unknown command `{other}`"),
    };
    let actor_id = host.get_or_start("counter", key).await?;
    let mut reply = Vec::new();
    for _ in 0..times {
        reply = act(host, &actor_id, command, args.clone()).await?;
    }
    print_reply("", &actor_id, reply)
}

/// Prints the count a counter replied with.
pub fn print_reply(
    prefix: &str,
    actor_id: &str,
    reply: Vec<u8>,
) -> Result<()> {
    let count = counter::count(reply)?;
    writeln!(std::io::stdout(), "{prefix}{actor_id} {count}")?;
    Ok(())
}

/// The address a `<kind>/<key>` word names.
fn address(word: &str) -> Result<Address> {
    let (kind, key) = word
        .split_once('/')
        .with_context(|| format!("`{word}` is not <kind>/<key>"))?;
    Ok(Address::new(kind, key))
}

pub fn number(word: Option<&String>) -> Result<u64> {
    let word = word.context("a number is missing")?;
    word.parse()
        .with_context(|| format!("`{word}` is not a number"))
}
