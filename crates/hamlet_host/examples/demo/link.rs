//! The demo's Node host: the TypeScript kinds of
//! `hosts/node/src/demo.ts` in a Node child of the demo's host, and
//! the commands of a demo started with `--link`.
//!
//! ```text
//! demo --link link    wait until the Node host is ready
//! demo --link make <n>
//!                     make the agent <n>
//! demo --link tell <n> <action> [<arg>...] [--count <times>]
//!     [--time]
//!                     have the realm call <action> of agent <n> as
//!                     itself, <times> times in a row, once by
//!                     default; an <arg> is JSON where it reads as
//!                     JSON, and else a string
//! demo --link heard <n> <within-ms>
//!                     read how many jobs the realm heard agent <n>
//!                     resumed, once it heard of one or <within-ms>
//!                     passed
//! ```
//!
//! `link` prints `link/ready 1`. `make` prints `agent/<n> 1` when it
//! made the agent and `agent/<n> 0` when there was one. `tell`
//! prints `agent/<n>/<action> <answer>`, the last answer as JSON on
//! one line, or `agent/<n>/<action> {"error":{"group":..,"code":..,
//! "message":..,"metadata":..}}` when the call failed with an actor's
//! error of a group other than `link`. With `--time` it then prints
//! `time agent/<n>/<action> count=<times> p50=<us>us p99=<us>us`, the
//! times of the realm's calls of the agent. A `tell` never makes its
//! agent: for an agent that was not made it fails with
//! `actor.not_found`. While the Node host is not ready it fails at
//! once with `link.down`, and with `link.lost` when the Node host is
//! lost before it answers. `heard` prints `heard/<n> <jobs>`.
//!
//! The program is `HAMLET_NODE`, or `node`, started with nothing
//! between it and this host: a launcher there makes the Node host
//! exit at once, saying so.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use hamlet_actor::action::encode_positional;
use hamlet_actor_core::ActorFactory;
use hamlet_host::child::{self, ChildOptions, LinkState, Supervisor};
use hamlet_host::route::{CallerKind, Routes};
use hamlet_host::{Address, Host, HostOptions};
use hamlet_messages::address::{self, Party};
use hamlet_messages::bytes::Base64;
use hamlet_messages::workspace::AgentNumber;
use tokio::time::Instant;

use crate::Demo;
use crate::command::{actor_error, number, p50_p99};
use crate::realm::{self, Heard, TellAny};

/// The kinds the Node host runs, each a kind of `demo.ts`.
pub const NODE_KINDS: &[&str] = &[address::Agent::KIND];

/// The first words of this module's commands.
pub const COMMANDS: [&str; 4] = ["link", "make", "tell", "heard"];

/// The Node host's entry, in the checkout the demo was built from.
const MAIN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../hosts/node/src/main.ts"
);

const READY_WAIT: Duration = Duration::from_secs(20);

const HEARD_POLL: Duration = Duration::from_millis(25);

/// The Node host of a demo.
pub struct Link {
    supervisor: Supervisor,
}

/// Opens the folder `home` as [`Host::open_with`] does, with the
/// kinds of [`NODE_KINDS`] in a Node child, and the route table of
/// [`routes`] in place of the one `options` holds.
pub fn open(
    home: &Path,
    factories: HashMap<String, ActorFactory>,
    options: HostOptions,
) -> Result<(Host, Link)> {
    ensure!(
        Path::new(MAIN).is_file(),
        "no Node host entry at {MAIN}"
    );
    let node = std::env::var("HAMLET_NODE")
        .unwrap_or_else(|_| "node".to_owned());
    // Node before 24.3 warns at each start that it strips types.
    let mut argv = vec![
        node,
        "--disable-warning=ExperimentalWarning".to_owned(),
        MAIN.to_owned(),
    ];
    for kind in NODE_KINDS {
        argv.extend(["--kind".to_owned(), (*kind).to_owned()]);
    }
    let child = ChildOptions {
        argv,
        kinds: NODE_KINDS
            .iter()
            .map(|kind| (*kind).to_owned())
            .collect(),
    };
    let options = HostOptions {
        routes: routes(),
        ..options
    };
    let (host, supervisor) =
        child::open(home, factories, options, child)?;
    Ok((host, Link { supervisor }))
}

fn routes() -> Routes {
    Routes::only(&[
        (
            CallerKind::Client,
            &[address::Realm::KIND, address::Agent::KIND],
        ),
        (
            CallerKind::Actor(address::Realm::KIND.to_owned()),
            &[address::Agent::KIND],
        ),
        (
            CallerKind::Actor(address::Agent::KIND.to_owned()),
            &[address::Realm::KIND],
        ),
    ])
}

impl Link {
    /// Waits until the Node host is ready, at most [`READY_WAIT`].
    pub async fn ready(&self) -> Result<()> {
        let mut state = self.supervisor.state();
        tokio::time::timeout(
            READY_WAIT,
            state.wait_for(|now| *now == LinkState::Ready),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "the Node host was not ready within {} s: it and \
                 its supervisor say why above",
                READY_WAIT.as_secs()
            )
        })??;
        Ok(())
    }

    pub async fn shutdown(self) {
        self.supervisor.shutdown().await;
    }
}

pub async fn run(
    demo: &Demo,
    command: &str,
    words: &[String],
) -> Result<()> {
    let link = demo.link.as_ref().with_context(|| {
        format!(
            "`{command}` needs a Node host: start the demo with \
             --link"
        )
    })?;
    match (command, words) {
        ("link", []) => {
            link.ready().await?;
            writeln!(std::io::stdout(), "link/ready 1")?;
            Ok(())
        }
        ("make", [n]) => make(&demo.host, &agent(n)?).await,
        ("tell", [n, action, rest @ ..]) => {
            tell(&demo.host, &agent(n)?, action, rest).await
        }
        ("heard", [n, within]) => {
            heard(
                &demo.host,
                AgentNumber(n.parse()?),
                Duration::from_millis(number(Some(within))?),
            )
            .await
        }
        _ => bail!(
            "usage: demo --link link | make <n> | tell <n> <action> \
             [<arg>...] [--count <times>] [--time] | heard <n> \
             <within-ms>"
        ),
    }
}

pub fn agent(n: &str) -> Result<Address> {
    Ok(address::Agent(AgentNumber(n.parse()?)).address()?)
}

/// Makes the actor at `at` with no input, as the program.
async fn make(host: &Host, at: &Address) -> Result<()> {
    let mut null = Vec::new();
    ciborium::into_writer(&serde_json::Value::Null, &mut null)?;
    let made = match host.create(at, null).await {
        Ok(()) => 1,
        Err(error)
            if actor_error(&error).is_some_and(|found| {
                found.group() == "actor"
                    && found.code() == "duplicate_key"
            }) =>
        {
            0
        }
        Err(error) => return Err(error),
    };
    writeln!(std::io::stdout(), "{at} {made}")?;
    Ok(())
}

async fn tell(
    host: &Host,
    at: &Address,
    action: &str,
    words: &[String],
) -> Result<()> {
    let mut times = 1;
    let mut timed = false;
    let mut args = Vec::new();
    let mut words = words.iter();
    while let Some(word) = words.next() {
        if word == "--count" {
            times = number(words.next())?;
        } else if word == "--time" {
            timed = true;
        } else {
            args.push(serde_json::from_str(word).unwrap_or_else(
                |_| serde_json::Value::String(word.clone()),
            ));
        }
    }
    ensure!(times > 0, "tell takes a count above 0");
    let tell = TellAny {
        to: at.clone(),
        name: action.to_owned(),
        args: Base64(encode_positional(&args)?),
    };
    let mut answer = Vec::new();
    let mut micros = Vec::new();
    for _ in 0..times {
        let told = match realm::ask(host, &tell).await {
            Ok(told) => told,
            Err(error) => {
                let Some(shown) = non_link_failure(&error) else {
                    return Err(error);
                };
                writeln!(std::io::stdout(), "{at}/{action} {shown}")?;
                return Ok(());
            }
        };
        micros.push(u64::try_from(told.took.as_micros())?);
        answer = told.answer.0;
    }
    let value = if answer.is_empty() {
        serde_json::Value::Null
    } else {
        ciborium::from_reader(answer.as_slice())?
    };
    let mut out = std::io::stdout();
    writeln!(out, "{at}/{action} {value}")?;
    if timed {
        let (p50, p99) = p50_p99(&mut micros);
        writeln!(
            out,
            "time {at}/{action} count={times} p50={p50}us \
             p99={p99}us",
        )?;
    }
    Ok(())
}

pub fn non_link_failure(
    error: &anyhow::Error,
) -> Option<serde_json::Value> {
    let found =
        actor_error(error).filter(|found| found.group() != "link")?;
    Some(serde_json::json!({
        "error": {
            "group": found.group(),
            "code": found.code(),
            "message": found.message(),
            "metadata": found.metadata(),
        },
    }))
}

/// Prints how many jobs the realm heard `agent` resumed, once it
/// heard of one or `within` passed.
async fn heard(
    host: &Host,
    agent: AgentNumber,
    within: Duration,
) -> Result<()> {
    let until = Instant::now() + within;
    let mut jobs = realm::ask(host, &Heard(agent)).await?;
    while jobs == 0 && Instant::now() < until {
        tokio::time::sleep(HEARD_POLL).await;
        jobs = realm::ask(host, &Heard(agent)).await?;
    }
    writeln!(std::io::stdout(), "heard/{} {jobs}", agent.0)?;
    Ok(())
}
