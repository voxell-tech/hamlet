//! One command line of the demo, run on its host.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use hamlet_actor_error::ActorError;
use hamlet_host::{Address, Host};

use crate::act::act;
use crate::counter;

/// Runs one command line split into words, on a host of the counter
/// kinds `kinds`. A key with no kind names a counter of the first.
pub async fn run(
    host: &Host,
    kinds: &[String],
    words: &[String],
) -> Result<()> {
    let kind = kinds.first().context("the demo runs no kind")?;
    if words.first().is_some_and(|word| word == "calls") {
        return calls(host, kinds, kind, &words[1..]).await;
    }
    let (command, at) = match words {
        [command, key, ..] => (command.as_str(), address(kind, key)?),
        _ => bail!("a command takes an actor key"),
    };
    let mut out = std::io::stdout();
    match command {
        "timers" => {
            for timer in host.set_aside_timers(&at).await? {
                writeln!(
                    out,
                    "timer {at} {} {} due={} tries={} error={}",
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
                    &at,
                    event_id,
                    i64::try_from(now.as_millis())?,
                )
                .await?;
            writeln!(out, "rearm {at} {event_id} {rearmed}")?;
            return Ok(());
        }
        "call" => {
            let to = address(
                kind,
                words.get(2).context("call takes <to>")?,
            )?;
            make(host, kinds, &at).await?;
            let (n, micros) = call(host, &at, &to).await?;
            writeln!(out, "call {at} {to} {n} {micros}us")?;
            return Ok(());
        }
        _ => {}
    }
    // Read before the counter is made, so that a line the demo
    // refuses makes none.
    let (args, times) = match command {
        "increment" => {
            let times = words
                .get(2)
                .map_or(Ok(1), |word| number(Some(word)))?;
            ensure!(times > 0, "`increment` counts from 1");
            (Vec::new(), times)
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
    host.get_or_start(&at).await?;
    let mut reply = Vec::new();
    for _ in 0..times {
        reply = act(host, &at, command, args.clone()).await?;
    }
    print_reply("", &at, reply)
}

/// Has the counter `[<from> <to>]`, `a` and `b` by default, call the
/// other `--count <n>` times, 1000 by default, each a call of its
/// own, and prints the calls' times and how many calls `<to>` had
/// from `<from>`. Fails as soon as a call reads another caller's
/// count.
async fn calls(
    host: &Host,
    kinds: &[String],
    kind: &str,
    words: &[String],
) -> Result<()> {
    let mut pair = Vec::new();
    let mut count = 1000;
    let mut words = words.iter();
    while let Some(word) = words.next() {
        if word == "--count" {
            count = number(words.next())?;
        } else {
            pair.push(address(kind, word)?);
        }
    }
    let (from, to) = match pair.as_slice() {
        [] => (address(kind, "a")?, address(kind, "b")?),
        [from, to] => (from.clone(), to.clone()),
        _ => bail!("calls takes no actor, or <from> <to>"),
    };
    ensure!(count > 0, "calls takes a count above 0");
    make(host, kinds, &from).await?;
    make(host, kinds, &to).await?;
    let mut micros = Vec::new();
    let mut first = None;
    let mut last = 0;
    for done in 0..count {
        let (n, took) = call(host, &from, &to).await?;
        let want =
            first.get_or_insert(n).saturating_add_unsigned(done);
        ensure!(
            n == want,
            "call {done} read {n} calls from {from}, not {want}"
        );
        micros.push(took);
        last = n;
    }
    micros.sort_unstable();
    let at = |percent: usize| {
        let rank = (micros.len() * percent).div_ceil(100);
        micros[rank.saturating_sub(1)]
    };
    writeln!(
        std::io::stdout(),
        "calls {from} {to} count={count} p50={}us p99={}us \
         from={last}",
        at(50),
        at(99)
    )?;
    Ok(())
}

/// Starts the actor at `at`, made if it has none, when its kind is
/// one of `kinds`.
async fn make(
    host: &Host,
    kinds: &[String],
    at: &Address,
) -> Result<()> {
    if kinds.iter().any(|kind| kind == at.kind()) {
        host.get_or_start(at).await?;
    }
    Ok(())
}

/// Has the counter `caller` call the actor at `to`, and answers how
/// many calls `to` had from it and the call's time in microseconds.
async fn call(
    host: &Host,
    caller: &Address,
    to: &Address,
) -> Result<(i64, u64)> {
    let mut target = Vec::new();
    ciborium::into_writer(to, &mut target)?;
    let reply = act(host, caller, "call", target).await?;
    let (n, micros) = reply
        .split_first_chunk::<8>()
        .and_then(|(n, micros)| Some((n, micros.first_chunk::<8>()?)))
        .context(
            "the counter's call replied with no count and time",
        )?;
    Ok((i64::from_le_bytes(*n), u64::from_le_bytes(*micros)))
}

/// Prints the count a counter replied with.
pub fn print_reply(
    prefix: &str,
    at: &Address,
    reply: Vec<u8>,
) -> Result<()> {
    let count = counter::count(reply)?;
    writeln!(std::io::stdout(), "{prefix}{at} {count}")?;
    Ok(())
}

/// The address a word names: `<kind>/<key>`, or `<key>` of `kind`.
fn address(kind: &str, word: &str) -> Result<Address> {
    Ok(match word.split_once('/') {
        Some((kind, key)) => Address::new(kind, key)?,
        None => Address::new(kind, word)?,
    })
}

/// `error` as the demo prints it, led by the `group.code` of the
/// first `ActorError` in its chain, as `actor.not_found`.
pub fn shown(error: &anyhow::Error) -> String {
    match error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ActorError>())
    {
        Some(found) => {
            format!("{}.{}: {error:#}", found.group(), found.code())
        }
        None => format!("{error:#}"),
    }
}

pub fn number(word: Option<&String>) -> Result<u64> {
    let word = word.context("a number is missing")?;
    word.parse()
        .with_context(|| format!("`{word}` is not a number"))
}
