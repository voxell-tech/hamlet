//! Follows an event of an agent as a client of the demo's host.
//!
//! ```text
//! demo watch <n> <event>
//!                     subscribe to <event> of agent <n>, and print
//!                     each item while the next commands run
//! ```
//!
//! `watch` prints `watch/<n> 1` once the subscribe is answered, or
//! `watch/<n> {"error":..}` as `tell` of demo/link.rs prints a failed
//! call. From then on it prints `watch/<n>/<event> <args>` for each
//! broadcast, its arguments as JSON on one line, `watch/<n>/gap 1`
//! where events were lost, and `watch/<n>/end <group.code>` when the
//! subscription ends.

use std::io::Write;

use anyhow::{Result, bail};
use hamlet_host::{Host, Item, Subscription};

use crate::command::{actor_error, shown};
use crate::link::{agent, failure};

pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    let [n, event] = words else {
        bail!("usage: demo watch <n> <event>");
    };
    let at = agent(n)?;
    let subscription =
        match host.subscribe_for_client(&at, event).await {
            Ok(subscription) => subscription,
            Err(error) => {
                let Some(shown) = failure(&error) else {
                    return Err(error);
                };
                writeln!(std::io::stdout(), "watch/{n} {shown}")?;
                return Ok(());
            }
        };
    writeln!(std::io::stdout(), "watch/{n} 1")?;
    tokio::spawn(print_items(
        subscription,
        format!("watch/{n}"),
        event.clone(),
    ));
    Ok(())
}

/// Prints each item of `subscription`, each line led by `name`, until
/// it ends.
async fn print_items(
    mut subscription: Subscription,
    name: String,
    event: String,
) {
    loop {
        let line = match subscription.next().await {
            Ok(Item::Event(args)) => {
                match ciborium::from_reader::<serde_json::Value, _>(
                    args.as_slice(),
                ) {
                    Ok(args) => format!("{name}/{event} {args}"),
                    Err(error) => format!("{name}/{event} {error}"),
                }
            }
            Ok(Item::Gap) => format!("{name}/gap 1"),
            Err(error) => {
                let why = actor_error(&error).map_or_else(
                    || shown(&error),
                    |found| {
                        format!("{}.{}", found.group(), found.code())
                    },
                );
                let _ =
                    writeln!(std::io::stdout(), "{name}/end {why}");
                return;
            }
        };
        if writeln!(std::io::stdout(), "{line}").is_err() {
            return;
        }
    }
}
