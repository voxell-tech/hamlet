//! The real agent, `agent/<n>` of hosts/node/src/agent/actor.ts, in a
//! Node child running hosts/node/src/agent/demo-host.ts, against the
//! stand-in realm of demo/stand_in_realm.rs and the real workspace.
//! A demo started with `--link` runs it in place of demo/link.rs's
//! Node host when `HAMLET_DEMO_AGENT` is `pi`.
//!
//! ```text
//! demo --link pi make <n>     make agent <n>, and set its reach so
//!                             its commands run in its filesystem
//! demo --link pi say <n> [--id <id>] <words>...
//!                             give agent <n> a chief's line, under
//!                             the input id <id>, a new one by
//!                             default; a word `own:<path>` is
//!                             <path> in agent <n>'s filesystem
//! demo --link pi place <n> <version> [lead=<a>] [hires=<a>,...]
//!     [roads=<a>,...] [off=<tool>,...]
//!                             send agent <n> its place
//! demo --link pi fill <n> <profile-id> <branch>
//!                             fill agent <n>'s filesystem
//! demo --link pi refuse on|off
//!                             refuse every model request, or stop
//! demo --link pi die-in-tell  have the next new `tell` kill the Node
//!                             host once the realm has kept it
//! demo --link pi recorded     list what the realm keeps
//! demo --link pi charges <n> <at-least> <within-ms>
//!                             wait until the realm has kept
//!                             <at-least> charges of agent <n>, and
//!                             no agent has called it for a second
//! demo --link pi conversation <n>
//!                             print agent <n>'s conversations
//! demo --link pi pid          print the Node host's process id
//! ```
//!
//! `make` prints `agent/<n> 1` when it made the agent and `agent/<n>
//! 0` when there was one. `say`, `place` and `fill` print
//! `said/<n> 1`, `placed/<n> <version>` and `filled/<n> 1` once the
//! agent took the message: a line's turn goes on after. `refuse`
//! prints `refuse/spending 1` or `0`, and `die-in-tell` prints
//! `die/tell 1`. `recorded` prints `calls/<action> <n>` for each
//! action of the realm the agents called, repeats included,
//! `acts/<action> <n>` for the society acts it keeps,
//! `charges/<agent> <n>` for the charges it keeps of each agent, and
//! `recorded/replies <n>` and `recorded/waits <n>`. `charges` prints
//! `charges/<n> <count>`, also when <within-ms> passed first: the
//! agent ends a turn soon after the charge of its last model request,
//! and calls no one about it. It never calls the agent.
//! `conversation` prints one line per entry, read from the agent's
//! file, which the agent holds while it runs: run it once the agent
//! is not running, as in a demo of its own. `pid` prints `node/pid
//! <pid>`.
//!
//! `pi` commands need this mode: in any other they fail at once.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail, ensure};
use hamlet_actor::action::encode_positional;
use hamlet_actors::workspace;
use hamlet_host::child::{self, ChildOptions, LinkState, Supervisor};
use hamlet_host::route::{CallerKind, Routes};
use hamlet_host::{Host, HostOptions};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::{
    AgentInput, AgentKind, Fill, Input, InputId, Place, PlaceVersion,
    Submit,
};
use hamlet_messages::realm::{ProfileId, ToolName};
use hamlet_messages::workspace::reach::{
    Footing, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, ReachVersion, RealmTask, SetReach,
};
use rusqlite::{Connection, OpenFlags};
use tokio::time::Instant;

use crate::Demo;
use crate::command::{actor_error, number};
use crate::link::agent;
use crate::realm::ask;
use crate::stand_in_realm::{
    self, Grant, Recorded, Records, Send, Set, Switch,
};

const CHOOSE: (&str, &str) = ("HAMLET_DEMO_AGENT", "pi");

const ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../hosts/node/src/agent/demo-host.ts"
);

/// The same file as `PID_FILE` of hosts/node/src/agent/demo-host.ts.
const PID_FILE: &str = "demo-node.pid";

const READY_WAIT: Duration = Duration::from_secs(20);

const POLL: Duration = Duration::from_millis(25);

/// How long the realm's records stay unchanged before `charges`
/// reads them as settled.
const QUIET: Duration = Duration::from_secs(1);

pub fn chosen() -> Result<bool> {
    let (name, value) = CHOOSE;
    match std::env::var(name) {
        Ok(set) if set == value => Ok(true),
        Ok(set) => bail!(
            "{name} is `{set}`; the one agent it names is `{value}`"
        ),
        Err(_) => Ok(false),
    }
}

pub struct PiHost {
    supervisor: Supervisor,
    home: PathBuf,
}

pub fn open(
    home: &Path,
    options: HostOptions,
) -> Result<(Host, PiHost)> {
    ensure!(
        Path::new(ENTRY).is_file(),
        "no Node host entry at {ENTRY}"
    );
    let node = std::env::var("HAMLET_NODE")
        .unwrap_or_else(|_| "node".to_owned());
    // Node before 24.3 warns at each start that it strips types.
    let argv = vec![
        node,
        "--disable-warning=ExperimentalWarning".to_owned(),
        ENTRY.to_owned(),
    ];
    let child = ChildOptions {
        argv,
        kinds: vec![address::Agent::KIND.to_owned()],
    };
    let factories = [
        (address::Realm::KIND.to_owned(), stand_in_realm::factory()),
        (address::Workspace::KIND.to_owned(), workspace::factory()),
    ]
    .into();
    let options = HostOptions {
        routes: routes(),
        ..options
    };
    let (host, supervisor) =
        child::open(home, factories, options, child)?;
    Ok((
        host,
        PiHost {
            supervisor,
            home: home.to_owned(),
        },
    ))
}

fn routes() -> Routes {
    let kind = |party: &str| CallerKind::Actor(party.to_owned());
    Routes::only(&[
        (
            CallerKind::Client,
            &[address::Realm::KIND, address::Agent::KIND],
        ),
        (
            kind(address::Realm::KIND),
            &[address::Agent::KIND, address::Workspace::KIND],
        ),
        (
            kind(address::Agent::KIND),
            &[address::Realm::KIND, address::Workspace::KIND],
        ),
        (
            kind(address::Workspace::KIND),
            &[address::Realm::KIND, address::Agent::KIND],
        ),
    ])
}

impl PiHost {
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

pub async fn run(demo: &Demo, words: &[String]) -> Result<()> {
    let (name, value) = CHOOSE;
    let pi = demo.pi.as_ref().with_context(|| {
        format!(
            "`pi` needs the real agent: start the demo with --link \
             and {name}={value}"
        )
    })?;
    let host = &demo.host;
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["make", n] => make(host, number_of(n)?).await,
        ["say", n, rest @ ..] => {
            say(pi, host, number_of(n)?, rest).await
        }
        ["place", n, version, rest @ ..] => {
            place(host, number_of(n)?, version, rest).await
        }
        ["fill", n, profile, branch] => {
            let n = number_of(n)?;
            let profile = profile.parse().with_context(|| {
                format!("the profile id {profile} is no number")
            })?;
            let fill = Fill {
                profile: ProfileId(profile),
                branch: (*branch).to_owned(),
            };
            send(host, n, &fill).await?;
            print(&format!("filled/{} 1", n.0))
        }
        ["refuse", on @ ("on" | "off")] => {
            let on = *on == "on";
            set(host, Switch::RefuseSpending, on).await?;
            print(&format!("refuse/spending {}", u8::from(on)))
        }
        ["die-in-tell"] => {
            set(host, Switch::DieInTell, true).await?;
            print("die/tell 1")
        }
        ["recorded"] => recorded(host).await,
        ["charges", n, at_least, within] => {
            charges(host, number_of(n)?, at_least, within).await
        }
        ["conversation", n] => conversation(pi, number_of(n)?),
        ["pid"] => {
            pi.ready().await?;
            let pid = live_pid(&pi.home, READY_WAIT).await?;
            print(&format!("node/pid {pid}"))
        }
        _ => bail!(
            "usage: demo --link pi make <n> | say <n> [--id <id>] \
             <words>... | place <n> <version> [lead=<a>] \
             [hires=<a>,...] [roads=<a>,...] [off=<tool>,...] | \
             fill <n> <profile-id> <branch> | refuse on|off | \
             die-in-tell | recorded | charges <n> <at-least> \
             <within-ms> | conversation <n> | pid"
        ),
    }
}

fn print(line: &str) -> Result<()> {
    use std::io::Write;
    writeln!(std::io::stdout(), "{line}")?;
    Ok(())
}

fn number_of(word: &str) -> Result<AgentNumber> {
    Ok(AgentNumber(word.parse().with_context(|| {
        format!("`{word}` is no agent's number")
    })?))
}

async fn make(host: &Host, n: AgentNumber) -> Result<()> {
    let input = AgentInput {
        kind: if n.0 == 1 {
            AgentKind::Hand
        } else {
            AgentKind::Hire
        },
        task: RealmTask(format!("standing-{}", n.0)),
        place: Place {
            version: PlaceVersion(1),
            lead: None,
            hires: Vec::new(),
            roads: Vec::new(),
            tools_off: Vec::new(),
            folder: None,
        },
        brief: None,
    };
    // The agent asks the workspace for its tools when it wakes.
    host.get_or_start(&address::Workspace.address()?).await?;
    let mut bytes = Vec::new();
    ciborium::into_writer(&input, &mut bytes)?;
    let made =
        match host.create(&agent(&n.0.to_string())?, bytes).await {
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
    ask(
        host,
        &Grant(SetReach {
            agent: n,
            version: ReachVersion(1),
            reach: Reach::Agent(Footing {
                project: None,
                network: false,
                trust: Vec::new(),
                secrets: SecretsVersion(1),
            }),
        }),
    )
    .await?;
    print(&format!("agent/{} {made}", n.0))
}

async fn say(
    pi: &PiHost,
    host: &Host,
    n: AgentNumber,
    words: &[&str],
) -> Result<()> {
    let (id, words) = match words {
        ["--id", id, rest @ ..] => ((*id).to_owned(), rest),
        rest => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
            (format!("line-{}", now.as_nanos()), rest)
        }
    };
    ensure!(!words.is_empty(), "say what?");
    let own = own_folder(&pi.home, n)?;
    let text = words
        .iter()
        .map(|word| match word.strip_prefix("own:") {
            Some(path) => format!("{}/{path}", own.display()),
            None => (*word).to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let submit = Submit {
        task: Some(RealmTask(id.clone())),
        id: InputId(id),
        input: Input::Line {
            text,
            images: Vec::new(),
            files: Vec::new(),
            note: None,
        },
    };
    send(host, n, &submit).await?;
    print(&format!("said/{} 1", n.0))
}

async fn place(
    host: &Host,
    n: AgentNumber,
    version: &str,
    words: &[&str],
) -> Result<()> {
    let numbers = |list: &str| -> Result<Vec<AgentNumber>> {
        list.split(',').map(number_of).collect()
    };
    let mut place = Place {
        version: PlaceVersion(version.parse()?),
        lead: None,
        hires: Vec::new(),
        roads: Vec::new(),
        tools_off: Vec::new(),
        folder: None,
    };
    for word in words {
        match word.split_once('=') {
            Some(("lead", lead)) => {
                place.lead = Some(number_of(lead)?);
            }
            Some(("hires", list)) => place.hires = numbers(list)?,
            Some(("roads", list)) => place.roads = numbers(list)?,
            Some(("off", list)) => {
                place.tools_off = list
                    .split(',')
                    .map(|name| ToolName {
                        name: name.to_owned(),
                    })
                    .collect();
            }
            _ => bail!("`{word}` is not a part of a place"),
        }
    }
    send(host, n, &place).await?;
    print(&format!("placed/{} {}", n.0, place.version.0))
}

async fn send<M>(
    host: &Host,
    n: AgentNumber,
    message: &M,
) -> Result<()>
where
    M: Message<To = address::Agent, From = address::Realm>,
{
    ask(
        host,
        &Send {
            to: n,
            name: M::NAME.to_owned(),
            args: encode_positional(message)?,
        },
    )
    .await
}

async fn set(host: &Host, switch: Switch, on: bool) -> Result<()> {
    ask(host, &Set { switch, on }).await
}

async fn recorded(host: &Host) -> Result<()> {
    let records = ask(host, &Recorded).await?;
    for (action, n) in &records.calls {
        print(&format!("calls/{action} {n}"))?;
    }
    for (action, n) in &records.acts {
        print(&format!("acts/{action} {n}"))?;
    }
    for (agent, n) in &records.charges {
        print(&format!("charges/{agent} {n}"))?;
    }
    print(&format!("recorded/replies {}", records.replies))?;
    print(&format!("recorded/waits {}", records.waits))
}

/// The folder of agent `n`'s filesystem, as the Node host names it.
fn own_folder(home: &Path, n: AgentNumber) -> Result<PathBuf> {
    Ok(std::fs::canonicalize(home)?
        .join("actors")
        .join("agent")
        .join(format!("{}.fs", n.0)))
}

async fn charges(
    host: &Host,
    n: AgentNumber,
    at_least: &str,
    within: &str,
) -> Result<()> {
    let at_least = number(Some(&at_least.to_owned()))?;
    let until = Instant::now()
        + Duration::from_millis(number(Some(&within.to_owned()))?);
    let of = |records: &Records| {
        records.charges.get(&n.0.to_string()).copied().unwrap_or(0)
    };
    let mut seen = ask(host, &Recorded).await?;
    let mut since = Instant::now();
    loop {
        let now = Instant::now();
        if now >= until
            || (of(&seen) >= at_least && now - since >= QUIET)
        {
            return print(&format!("charges/{} {}", n.0, of(&seen)));
        }
        tokio::time::sleep(POLL).await;
        let records = ask(host, &Recorded).await?;
        if records != seen {
            seen = records;
            since = Instant::now();
        }
    }
}

fn conversation(pi: &PiHost, n: AgentNumber) -> Result<()> {
    let path = pi
        .home
        .join("actors")
        .join("agent")
        .join(format!("{}.sqlite", n.0));
    let file = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .with_context(|| format!("open {}", path.display()))?;
    let mut entries = file
        .prepare(
            "SELECT conversation_id, id, record FROM pi_entries \
             ORDER BY id",
        )
        .with_context(|| {
            format!(
                "read the entries of {}; an agent that runs holds \
                 its file, so read it once agent {} does not, as in \
                 a demo of its own",
                path.display(),
                n.0
            )
        })?;
    let rows = entries.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (conversation, id, record) = row?;
        if let Some(shown) = entry(&serde_json::from_str(&record)?) {
            print(&format!("{conversation}/{id} {shown}"))?;
        }
    }
    Ok(())
}

fn entry(record: &serde_json::Value) -> Option<String> {
    let message = record.pointer("/model/0")?;
    let text = |content: &serde_json::Value| -> String {
        match content {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Array(blocks) => blocks
                .iter()
                .map(|block| match block["type"].as_str() {
                    Some("text") => block["text"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                    Some("toolCall") => format!(
                        "[{} {}]",
                        block["name"].as_str().unwrap_or(""),
                        block["arguments"]
                    ),
                    _ => String::new(),
                })
                .collect(),
            _ => String::new(),
        }
    };
    let words = text(&message["content"]).replace('\n', " / ");
    match message["role"].as_str()? {
        "system" => None,
        "assistant" => Some(match message["errorMessage"].as_str() {
            Some(error) => format!("assistant: failed: {error}"),
            None => format!("assistant: {words}"),
        }),
        "toolResult" => Some(format!(
            "{} result: {words}",
            message["toolName"].as_str().unwrap_or("tool")
        )),
        _ => {
            let kind = record["kind"].as_str().unwrap_or("user");
            Some(format!(
                "{}: {words}",
                kind.strip_prefix("hamlet.").unwrap_or(kind)
            ))
        }
    }
}

/// Just after a Node host dies, [`PID_FILE`] still names it until
/// the next one starts.
async fn live_pid(home: &Path, within: Duration) -> Result<u32> {
    let until = Instant::now() + within;
    loop {
        let pid = node_pid(home)?;
        if !has_ended(pid)? {
            return Ok(pid);
        }
        ensure!(
            Instant::now() < until,
            "the Node host {pid} has ended, and no other started \
             within {} s",
            within.as_secs()
        );
        tokio::time::sleep(POLL).await;
    }
}

fn node_pid(home: &Path) -> Result<u32> {
    let path = home.join(PID_FILE);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    text.trim()
        .parse()
        .with_context(|| format!("{} holds no pid", path.display()))
}

pub async fn kill_node_host(
    home: &Path,
    within: Duration,
) -> Result<()> {
    let pid = node_pid(home)?;
    tracing::info!(pid, "killing the Node host");
    signal(pid, "-KILL")?;
    let until = Instant::now() + within;
    while !has_ended(pid)? {
        ensure!(
            Instant::now() < until,
            "the Node host {pid} did not end within {} s of its kill",
            within.as_secs()
        );
        tokio::time::sleep(POLL).await;
    }
    Ok(())
}

/// Answers whether `kill` said it was sent.
#[cfg(unix)]
fn signal(pid: u32, flag: &str) -> Result<bool> {
    Ok(std::process::Command::new("kill")
        .args([flag, &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .context("run kill")?
        .success())
}

#[cfg(not(unix))]
fn signal(_pid: u32, _flag: &str) -> Result<bool> {
    bail!("the demo signals the Node host only on Unix")
}

/// On Linux, a process that is dead and waits to be collected has
/// ended too.
fn has_ended(pid: u32) -> Result<bool> {
    #[cfg(target_os = "linux")]
    if let Ok(stat) =
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
    {
        // The state follows the command's name, which is in
        // parentheses and may hold any character.
        return Ok(stat.rsplit_once(')').is_some_and(|(_, rest)| {
            rest.trim_start().starts_with('Z')
        }));
    }
    Ok(!signal(pid, "-0")?)
}
