//! The real agent, `agent/<n>` of hosts/node/src/agent/actor.ts, in a
//! Node child running hosts/node/src/agent/demo-host.ts, for a demo
//! started with `--realm-actor`, and the `pi` commands that reach it.
//!
//! ```text
//! demo --realm-actor pi die-after <action>
//!                             have the Node host kill itself once
//!                             the realm answers an agent's next call
//!                             of <action>, before the agent reads
//!                             the answer
//! demo --realm-actor pi conversation <n>
//!                             print agent <n>'s entries
//! demo --realm-actor pi pid   print the Node host's process id
//! demo --realm-actor pi file <n> <path> <from>
//!                             copy the file <from> to <path> in
//!                             agent <n>'s filesystem
//! demo --realm-actor pi new <n>
//!                             begin a new session, as a client
//! demo --realm-actor pi resume <n> <session>
//!                             go back to session <session>
//! demo --realm-actor pi retry <n>
//!                             carry on the last turn
//! demo --realm-actor pi stop <n>
//!                             stop agent <n>'s turns
//! demo --realm-actor pi rewind <n> <item>
//!                             take the current session back to
//!                             before the chief's line <item>
//! demo --realm-actor pi chat <n> [past=<session>] [from=<a>]
//!     [kind=letters|questions] [before=<item>]
//!                             read a page of a chat of agent <n>
//! demo --realm-actor pi history <n>
//!                             list agent <n>'s sessions
//! demo --realm-actor pi prompt <n>
//!                             read agent <n>'s prompt
//! demo --realm-actor pi reflect <n>
//!                             ask agent <n> to reflect now
//! demo --realm-actor pi login start <provider> key|subscription
//! demo --realm-actor pi login reply <login> <value>
//! demo --realm-actor pi login cancel <login>
//!                             a step of a sign-in, on agent 1
//! demo --realm-actor pi follow <n>
//!                             follow agent <n>'s chat as a client
//! demo --realm-actor pi answered <verb> <n> [<words>...]
//!                             call agent <n> as `pi <verb>` does,
//!                             for a reader of counts
//! ```
//!
//! `die-after` prints `die/<action> 1`. `conversation` prints one
//! line per entry, `<conversation>/<entry> <role>: <words>`, read
//! through the agent while it runs. `pid` prints `node/pid <pid>`,
//! and `file` `file/<n> <bytes>`.
//!
//! `new` to `login` call the agent as a client does and print
//! `<verb>/<n> <answer>`, the answer as JSON on one line, or
//! `<verb>/<n> {"error":{"group":..,"code":..,"message":..,
//! "metadata":..}}` when the agent failed the call. Each goes under a
//! new request id, or with `--id <word>` before its other words under
//! the request id the word stands for, as `demo realm say` makes it,
//! so that two calls with one word are one request and its repeat.
//! `answered` prints `<verb>/<n> 1` in place of an answer and
//! `<verb>/<n> 0` in place of a refusal or a failure, as the kill kit
//! reads a line. `follow` is `demo watch <n> chat`: it prints
//! `watch/<n> 1`, then `watch/<n>/chat <event>` for each change, as
//! demo/watch.rs says.
//!
//! With `HAMLET_DEMO_FAUX` naming a faux script, the Node host's
//! model answers from it, as hosts/node/src/agent/demo-host.ts says.
//! The demo hands it the script's path. With `HAMLET_DEMO_SIGN_IN`
//! set to `1`, it also lists two providers to sign in to, on
//! loopback, as demo-host.ts says.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_actor_core::ActorFactory;
use hamlet_host::route::{CallerKind, Routes};
use hamlet_host::{Host, HostOptions};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::chat::{
    Login, NewSession, ReadChat, ReadHistory, ReadPrompt, Reflect,
    Resume, Retry, Rewind, Stop,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use hamlet_protocol::client::{
    self as protocol, AgentId, ChatRef, ChatRefKind, LoginMethod,
    LoginReply, LoginStart, RequestId,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::Demo;
use crate::link::{self, NodeHost, non_link_failure};
use crate::realm_actor::request_id;

const FAUX: &str = "HAMLET_DEMO_FAUX";

/// Lists the loopback providers the sign-in lanes sign in to.
const SIGN_IN: (&str, &str) = ("HAMLET_DEMO_SIGN_IN", "1");

const ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../hosts/node/src/agent/demo-host.ts"
);

/// The same file as `PID_FILE` of hosts/node/src/agent/demo-host.ts.
const PID_FILE: &str = "demo-node.pid";

const POLL: Duration = Duration::from_millis(25);

/// Longer than `login.ts`'s 25 s `STEP_WAIT_MS`, which a sign-in's
/// step waits for its provider.
const ASK_WITHIN: Duration = Duration::from_secs(60);

/// Longer than `FORWARD_MS` of demo-host.ts, the deadline of the call
/// a `forward` makes.
const AT_AGENT_WITHIN: Duration = Duration::from_secs(30);

const HAND: AgentNumber = AgentNumber(1);

/// Opens `home` as [`Host::open_with`] does, with `factories`, the
/// agent in a Node child, and a route table of what each kind calls.
pub fn open(
    home: &Path,
    factories: HashMap<String, ActorFactory>,
    options: HostOptions,
) -> Result<(Host, NodeHost)> {
    let mut args = Vec::new();
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
        args.extend(["--faux".to_owned(), script.to_owned()]);
    }
    let (name, value) = SIGN_IN;
    if std::env::var(name).is_ok_and(|set| set == value) {
        args.push("--sign-in".to_owned());
    }
    let options = HostOptions {
        routes: routes(),
        ..options
    };
    link::start(
        home,
        factories,
        options,
        ENTRY,
        args,
        &[address::Agent::KIND],
    )
}

fn routes() -> Routes {
    let kind = |party: &str| CallerKind::Actor(party.to_owned());
    let peers = [address::Realm::KIND, address::Agent::KIND];
    Routes::only(&[
        (CallerKind::Client, &peers),
        (
            kind(address::Realm::KIND),
            &[
                address::Agent::KIND,
                address::Workspace::KIND,
                address::Profile::KIND,
                address::Library::KIND,
            ],
        ),
        (
            kind(address::Agent::KIND),
            &[
                address::Realm::KIND,
                address::Workspace::KIND,
                address::Profile::KIND,
                address::Library::KIND,
            ],
        ),
        (kind(address::Workspace::KIND), &peers),
        (kind(address::Profile::KIND), &peers),
        (kind(address::Library::KIND), &peers),
    ])
}

pub async fn run(demo: &Demo, words: &[String]) -> Result<()> {
    let node =
        demo.node.as_ref().filter(|_| demo.realm_actor).context(
            "`pi` needs the real agent: start the demo with \
             --realm-actor",
        )?;
    let host = &demo.host;
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["die-after", action] => {
            at_agent(host, HAND, &DieAfter((*action).to_owned()))
                .await?;
            print(&format!("die/{action} 1"))
        }
        ["conversation", n] => {
            conversation(host, number_of(n)?).await
        }
        ["pid"] => {
            node.ready().await?;
            let pid = live_pid(&demo.home, link::READY_WAIT).await?;
            print(&format!("node/pid {pid}"))
        }
        ["file", n, path, from] => {
            let n = number_of(n)?;
            let to = own_folder(&demo.home, n)?.join(path);
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
    "demo --realm-actor pi die-after <action> | conversation <n> | \
     pid | file <n> <path> <from> | new <n> | resume <n> <session> \
     | retry <n> | stop <n> | rewind <n> <item> | chat <n> \
     [past=<session>] [from=<a>] [kind=letters|questions] \
     [before=<item>] | history <n> | prompt <n> | reflect <n> | \
     login start <provider> key|subscription | login reply <login> \
     <value> | login cancel <login> | follow <n> | answered <verb> \
     <n> [<words>...]; a client verb takes --id <word> before its \
     other words";

/// The request id `--id <word>` names at the head of `words`, or a
/// new one, and the words after it.
fn id_of<'a, 'b>(
    words: &'a [&'b str],
) -> Result<(RequestId, &'a [&'b str])> {
    match words {
        ["--id", word, rest @ ..] => Ok((
            RequestId::try_from(request_id(word))
                .context("a request id made from a word is a UUID")?,
            rest,
        )),
        rest => Ok((RequestId::new(), rest)),
    }
}

async fn client(
    host: &Host,
    verb: &str,
    n: AgentNumber,
    words: &[&str],
) -> Result<serde_json::Value> {
    let to = AgentId(u64::from(n.0));
    let (id, words) = id_of(words)?;
    match (verb, words) {
        ("new", []) => {
            asked(host, n, &NewSession(protocol::New { id, to }))
                .await
        }
        ("resume", [number]) => {
            let resume = protocol::Resume {
                id,
                number: number.parse().with_context(|| {
                    format!("`{number}` is no session's number")
                })?,
                to,
            };
            asked(host, n, &Resume(resume)).await
        }
        ("retry", []) => {
            asked(host, n, &Retry(protocol::Retry { id, to })).await
        }
        ("stop", []) => {
            let stop = protocol::Stop {
                agent: Some(to),
                id,
            };
            asked(host, n, &Stop(stop)).await
        }
        ("rewind", [item]) => {
            let rewind = protocol::Rewind {
                chat: chat_of(to, &[])?.0,
                id,
                item: (*item).to_owned(),
            };
            asked(host, n, &Rewind(rewind)).await
        }
        ("chat", words) => {
            let (chat, before) = chat_of(to, words)?;
            let open = protocol::Open { before, chat, id };
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
    let (id, words) = id_of(words)?;
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

/// The folder of agent `n`'s filesystem, as the Node host names it.
pub fn own_folder(home: &Path, n: AgentNumber) -> Result<PathBuf> {
    Ok(std::fs::canonicalize(home)?
        .join("actors")
        .join("agent")
        .join(format!("{}.fs", n.0)))
}

/// Has agent `me` send `message` to `to` as itself, and answers the
/// answer.
pub async fn forward<M: Message>(
    host: &Host,
    me: AgentNumber,
    to: &M::To,
    message: &M,
) -> Result<M::Output> {
    let to = to.address()?;
    let args = ciborium::from_reader(
        encode_positional(message)?.as_slice(),
    )?;
    let forward = Forward(
        Target {
            kind: to.kind().to_owned(),
            key: to.key().to_owned(),
        },
        M::NAME.to_owned(),
        args,
    );
    let answer = at_agent(host, me, &forward).await?;
    Ok(serde_json::from_value(answer)?)
}

/// The inputs agent `me` took into its transcript, oldest first.
pub async fn taken(host: &Host, me: AgentNumber) -> Result<Holding> {
    at_agent(host, me, &Taken).await
}

/// Calls agent `me`'s demo-only `action` as the program.
async fn at_agent<A: Action>(
    host: &Host,
    me: AgentNumber,
    action: &A,
) -> Result<A::Output> {
    let reply = host
        .call(
            &address::Agent(me).address()?,
            A::NAME,
            encode_positional(action)?,
            AT_AGENT_WITHIN,
        )
        .await?;
    Ok(ciborium::from_reader(reply.as_slice())?)
}

/// The demo-only `forward` of demo-host.ts: its target, the action's
/// name and the action's positional arguments.
#[derive(Serialize, Deserialize)]
struct Forward(Target, String, serde_json::Value);

#[derive(Serialize, Deserialize)]
struct Target {
    kind: String,
    key: String,
}

impl Action for Forward {
    type Output = serde_json::Value;
    const NAME: &'static str = "forward";
}

#[derive(Serialize, Deserialize)]
struct Taken;

impl Action for Taken {
    type Output = Holding;
    const NAME: &'static str = "taken";
}

/// What `taken` of demo-host.ts answers.
#[derive(Serialize, Deserialize)]
pub struct Holding {
    pub standing: RealmTask,
    pub inputs: Vec<Input>,
}

/// An input in an agent's transcript.
#[derive(Serialize, Deserialize)]
pub struct Input {
    /// Its entry's id.
    pub taken: i64,
    /// As `line` or `letter`.
    pub kind: String,
    pub task: Option<RealmTask>,
    /// The letter a question asks in.
    pub question: Option<u32>,
    /// What the model reads of it, as its entry keeps it.
    pub text: String,
}

#[derive(Serialize, Deserialize)]
struct Entries;

impl Action for Entries {
    type Output = Vec<Entry>;
    const NAME: &'static str = "entries";
}

#[derive(Serialize, Deserialize)]
struct Entry {
    conversation: i64,
    id: i64,
    record: String,
}

#[derive(Serialize, Deserialize)]
struct DieAfter(String);

impl Action for DieAfter {
    type Output = ();
    const NAME: &'static str = "die_after";
}

async fn conversation(host: &Host, n: AgentNumber) -> Result<()> {
    for row in at_agent(host, n, &Entries).await? {
        if let Some(shown) =
            entry(&serde_json::from_str(&row.record)?)
        {
            print(&format!(
                "{}/{} {shown}",
                row.conversation, row.id
            ))?;
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
        if !hamlet_os::process::has_ended(pid)? {
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
