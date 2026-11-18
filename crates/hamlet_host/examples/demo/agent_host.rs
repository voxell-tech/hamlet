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
//! demo --link pi copies <n> <json>
//!                             keep the copies <json> for agent <n>
//!                             and push them to it, as the realm
//! demo --link pi file <n> <path> <from>
//!                             copy the file <from> to <path> in
//!                             agent <n>'s filesystem
//! demo --link pi new <n>      begin a new session, as a client
//! demo --link pi resume <n> <session>
//!                             go back to session <session>
//! demo --link pi retry <n>    carry on the last turn
//! demo --link pi stop <n>     stop agent <n>'s turns
//! demo --link pi rewind <n> <item>
//!                             take the current session back to
//!                             before the chief's line <item>
//! demo --link pi chat <n> [past=<session>] [from=<a>]
//!     [kind=letters|questions] [before=<item>]
//!                             read a page of a chat of agent <n>
//! demo --link pi history <n>  list agent <n>'s sessions
//! demo --link pi prompt <n>   read agent <n>'s prompt
//! demo --link pi reflect <n>  ask agent <n> to reflect now
//! demo --link pi login start <provider> key|subscription
//! demo --link pi login reply <login> <value>
//! demo --link pi login cancel <login>
//!                             a step of a sign-in, on agent 1
//! demo --link pi follow <n>   follow agent <n>'s chat as a client
//! demo --link pi answered <verb> <n> [<words>...]
//!                             call agent <n> as `pi <verb>` does,
//!                             for a reader of counts
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
//! <pid>`. `copies` prints `copies/<n> 1` once the agent took them,
//! and `file` `file/<n> <bytes>`.
//!
//! `new` to `login` call the agent as a client does, each under a new
//! request id, and print `<verb>/<n> <answer>`, the answer as JSON on
//! one line, or `<verb>/<n> {"error":{"group":..,"code":..,
//! "message":..,"metadata":..}}` when the agent failed the call.
//! `answered` prints `<verb>/<n> 1` in place of an answer and
//! `<verb>/<n> 0` in place of a refusal or a failure, as the kill kit
//! reads a line. `follow` is `demo watch <n> chat`: it prints
//! `watch/<n> 1`, then `watch/<n>/chat <event>` for each change, as
//! demo/watch.rs says.
//!
//! With `HAMLET_DEMO_FAUX` naming a faux script, the Node host's
//! model answers from it, as hosts/node/src/agent/demo-host.ts says.
//! The Node host is started with only `HAMLET_HOME` and `PATH`, so
//! the demo hands it the script's path. With `HAMLET_DEMO_SIGN_IN`
//! set to `1`, it also lists two providers to sign in to, on
//! loopback, as demo-host.ts says.
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
use hamlet_messages::agent::chat::{
    Login, NewSession, ReadChat, ReadHistory, ReadPrompt, Reflect,
    Resume, Retry, Rewind, Stop,
};
use hamlet_messages::agent::copies::Copies;
use hamlet_messages::agent::{
    AgentInput, AgentKind, Fill, Input, InputId, Place, PlaceVersion,
    Submit,
};
use hamlet_messages::realm::{ProfileId, ToolName};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::reach::{
    Footing, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, ReachVersion, RealmTask, SetReach,
};
use hamlet_protocol::client::{
    self as protocol, AgentId, ChatRef, ChatRefKind, LoginMethod,
    LoginReply, LoginStart, RequestId,
};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::time::Instant;

use crate::Demo;
use crate::command::{actor_error, number};
use crate::link::{agent, non_link_failure};
use crate::realm::ask;
use crate::stand_in_realm::{
    self, Grant, PushCopies, Recorded, Records, Send, Set, Switch,
};

const CHOOSE: (&str, &str) = ("HAMLET_DEMO_AGENT", "pi");

const FAUX: &str = "HAMLET_DEMO_FAUX";

/// Lists the loopback providers the sign-in lanes sign in to.
const SIGN_IN: (&str, &str) = ("HAMLET_DEMO_SIGN_IN", "1");

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

/// Longer than `login.ts`'s 25 s `STEP_WAIT_MS`, which a sign-in's
/// step waits for its provider.
const ASK_WITHIN: Duration = Duration::from_secs(60);

const HAND: AgentNumber = AgentNumber(1);

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
    let mut argv = vec![
        node,
        "--disable-warning=ExperimentalWarning".to_owned(),
        ENTRY.to_owned(),
    ];
    if let Some(script) = std::env::var_os(FAUX) {
        let script =
            std::fs::canonicalize(&script).with_context(|| {
                format!(
                    "{FAUX} names {}, which is not there",
                    script.display()
                )
            })?;
        let script = script
            .to_str()
            .with_context(|| format!("{FAUX}'s path is not UTF-8"))?;
        argv.extend(["--faux".to_owned(), script.to_owned()]);
    }
    let (name, value) = SIGN_IN;
    if std::env::var(name).is_ok_and(|set| set == value) {
        argv.push("--sign-in".to_owned());
    }
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
        ["copies", n, json @ ..] => {
            let n = number_of(n)?;
            let copies: Copies = serde_json::from_str(
                &json.join(" "),
            )
            .context("the copies are not JSON of their message")?;
            ask(host, &PushCopies { to: n, copies }).await?;
            print(&format!("copies/{} 1", n.0))
        }
        ["file", n, path, from] => {
            let n = number_of(n)?;
            let to = own_folder(&pi.home, n)?.join(path);
            if let Some(folder) = to.parent() {
                std::fs::create_dir_all(folder)?;
            }
            let bytes =
                std::fs::copy(from, &to).with_context(|| {
                    format!("copy {from} to {}", to.display())
                })?;
            print(&format!("file/{} {bytes}", n.0))
        }
        ["follow", n] => {
            crate::watch::run(
                demo,
                &[(*n).to_owned(), "chat".to_owned()],
            )
            .await
        }
        ["login", step @ ..] => {
            let answer = login(host, step).await?;
            print(&format!("login/{} {answer}", HAND.0))
        }
        ["answered", verb, n, rest @ ..] => {
            let n = number_of(n)?;
            let answer = client(host, verb, n, rest).await?;
            let refused = answer.get("error").is_some()
                || answer.get("type")
                    == Some(&serde_json::json!("refused"));
            print(&format!("{verb}/{} {}", n.0, u8::from(!refused)))
        }
        [verb, n, rest @ ..] => {
            let n = number_of(n)?;
            let answer = client(host, verb, n, rest).await?;
            print(&format!("{verb}/{} {answer}", n.0))
        }
        _ => bail!("usage: {USAGE}"),
    }
}

const USAGE: &str =
    "demo --link pi make <n> | say <n> [--id <id>] <words>... | \
     place <n> <version> [lead=<a>] [hires=<a>,...] [roads=<a>,...] \
     [off=<tool>,...] | fill <n> <profile-id> <branch> | refuse \
     on|off | die-in-tell | recorded | charges <n> <at-least> \
     <within-ms> | conversation <n> | pid | copies <n> <json> | \
     file <n> <path> <from> | new <n> | resume <n> <session> | \
     retry <n> | stop <n> | rewind <n> <item> | chat <n> \
     [past=<session>] [from=<a>] [kind=letters|questions] \
     [before=<item>] | history <n> | prompt <n> | reflect <n> | \
     login start <provider> key|subscription | login reply <login> \
     <value> | login cancel <login> | follow <n> | answered <verb> \
     <n> [<words>...]";

async fn client(
    host: &Host,
    verb: &str,
    n: AgentNumber,
    words: &[&str],
) -> Result<serde_json::Value> {
    let id = RequestId::new;
    let to = AgentId(u64::from(n.0));
    match (verb, words) {
        ("new", []) => {
            asked(
                host,
                n,
                &NewSession(protocol::New { id: id(), to }),
            )
            .await
        }
        ("resume", [number]) => {
            let resume = protocol::Resume {
                id: id(),
                number: number.parse().with_context(|| {
                    format!("`{number}` is no session's number")
                })?,
                to,
            };
            asked(host, n, &Resume(resume)).await
        }
        ("retry", []) => {
            asked(host, n, &Retry(protocol::Retry { id: id(), to }))
                .await
        }
        ("stop", []) => {
            let stop = protocol::Stop {
                agent: Some(to),
                id: id(),
            };
            asked(host, n, &Stop(stop)).await
        }
        ("rewind", [item]) => {
            let rewind = protocol::Rewind {
                chat: chat_of(to, &[])?.0,
                id: id(),
                item: (*item).to_owned(),
            };
            asked(host, n, &Rewind(rewind)).await
        }
        ("chat", words) => {
            let (chat, before) = chat_of(to, words)?;
            let open = protocol::Open {
                before,
                chat,
                id: id(),
            };
            asked(host, n, &ReadChat(open)).await
        }
        ("history", []) => asked(host, n, &ReadHistory).await,
        ("prompt", []) => asked(host, n, &ReadPrompt).await,
        ("reflect", []) => asked(host, n, &Reflect).await,
        _ => bail!("usage: {USAGE}"),
    }
}

fn chat_of(
    with: AgentId,
    words: &[&str],
) -> Result<(ChatRef, Option<String>)> {
    let mut chat = ChatRef {
        from: None,
        kind: None,
        past: None,
        with,
    };
    let mut before = None;
    for word in words {
        match word.split_once('=') {
            Some(("past", session)) => {
                chat.past =
                    Some(session.parse().with_context(|| {
                        format!("`{session}` is no session's number")
                    })?);
            }
            Some(("from", peer)) => {
                chat.from =
                    Some(AgentId(u64::from(number_of(peer)?.0)));
            }
            Some(("kind", "letters")) => {
                chat.kind = Some(ChatRefKind::Letters);
            }
            Some(("kind", "questions")) => {
                chat.kind = Some(ChatRefKind::Questions);
            }
            Some(("before", item)) => before = Some(item.to_owned()),
            _ => bail!("`{word}` does not name a chat"),
        }
    }
    Ok((chat, before))
}

async fn login(
    host: &Host,
    words: &[&str],
) -> Result<serde_json::Value> {
    let id = RequestId::new();
    let login = match words {
        ["start", provider, method] => protocol::Login::Start {
            id,
            start: LoginStart {
                method: match *method {
                    "key" => LoginMethod::Key,
                    "subscription" => LoginMethod::Subscription,
                    other => bail!("`{other}` is no way to sign in"),
                },
                provider: (*provider).to_owned(),
            },
        },
        ["reply", login, value] => protocol::Login::Reply {
            id,
            reply: LoginReply {
                login: (*login).to_owned(),
                value: (*value).to_owned(),
            },
        },
        ["cancel", login] => protocol::Login::Cancel {
            cancel: (*login).to_owned(),
            id,
        },
        _ => bail!("usage: {USAGE}"),
    };
    asked(host, HAND, &Login(login)).await
}

/// Answers as `Ok` JSON both the agent's answer and the actor's error
/// it failed with.
async fn asked<M>(
    host: &Host,
    n: AgentNumber,
    message: &M,
) -> Result<serde_json::Value>
where
    M: Message<To = address::Agent, From = Client>,
    M::Output: Serialize + DeserializeOwned,
{
    let called = host
        .call_for_client(
            &address::Agent(n).address()?,
            M::NAME,
            encode_positional(message)?,
            ASK_WITHIN,
        )
        .await;
    match called {
        Ok(answer) => {
            match ciborium::from_reader::<M::Output, _>(
                answer.as_slice(),
            ) {
                Ok(read) => Ok(serde_json::to_value(read)?),
                Err(error) => {
                    let raw: serde_json::Value =
                        ciborium::from_reader(answer.as_slice())?;
                    bail!(
                        "agent {} answered `{}` with {raw}, which \
                         is not its answer: {error}",
                        n.0,
                        M::NAME
                    )
                }
            }
        }
        Err(error) => non_link_failure(&error).ok_or(error),
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
        copies: Default::default(),
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
