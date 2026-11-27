//! The realm actor, run by a demo started with `--realm-actor` beside
//! the workspace, the profiles, the library and the real agent in a
//! Node child; and its commands, which drive the realm as the chief,
//! and the agent as itself through the agent's demo-only `forward`:
//!
//! ```text
//! demo realm found <name>
//!                    found the hamlet <name> as the chief
//! demo realm hire <lead> <call> [profile#<n>[@<version>]] <purpose ...>
//!                    have agent <lead> hire, as its call <call>, from
//!                    the profile <n> at <version> when named
//! demo realm say <n> <id> <text ...>
//!                    say a line to agent <n> as the chief, under the
//!                    request id the word <id> stands for; a word
//!                    `own:<path>` is <path> in agent <n>'s
//!                    filesystem
//! demo realm answer <wait> allow|always|refuse|go|withdraw|reply
//!     |dismiss [<text ...>]
//!                    answer the wait <wait> as the chief
//! demo realm end <n>
//!                    end agent <n> as the chief
//! demo realm budget <hamlet> <micros>
//!                    bound the hamlet <hamlet>'s spending
//! demo realm review <hamlet> always|remote|never
//! demo realm mode <hamlet> manual|auto
//! demo realm folder <hamlet> <path>
//!                    set the hamlet's review limit, mode or folder
//! demo realm allowance <n> <micros>
//!                    set hire <n>'s allowance
//! demo realm nickname <n> [<name>]
//!                    show hire <n> by <name>, or by its own name
//! demo realm road <a> <b>
//!                    lay a road between the agents <a> and <b>, each
//!                    `hand`, `mayor` or `hire#<k>`
//! demo realm charter <text ...>
//! demo realm model <model>
//! demo realm price <model> <input> <output>
//!                    edit the charter, the realm's model, or the
//!                    model's price in dollars per million tokens
//! demo realm names <hamlet> <count>
//!                    rename the hamlet <count> times, timing each
//! demo realm live <n>
//!                    wait until agent <n> is live in its reach
//! demo realm world
//!                    read the world a client shows
//! demo realm reads <count> <ms> [<n>]
//!                    read the world <count> times, <ms> apart
//! demo agent <n> tell <call> <to> [task=<task>] [paths=<p>,...]
//!     <text ...>
//! demo agent <n> ask <call> <to> <text ...>
//!                    send a letter or a question as agent <n>'s call
//!                    <call>, to `hand`, `mayor` or `hire#<k>`; a
//!                    letter with the files at the absolute paths
//!                    <p>, in agent <n>'s hamlet's folder, which its
//!                    receiver finds in its inbox
//! demo agent <n> reply <question> <text ...>
//!                    answer the question <question>
//! demo agent <n> ask-chief <call> <task> <question ...>
//! demo agent <n> object <call> <task> <reason ...>
//!                    ask the chief, or object to its line, on <task>
//! demo agent <n> start-worker <call> <brief ...>
//! demo agent <n> worker-done|errand-done <text ...>
//!                    start a worker, or end as a worker or an errand
//!                    with an answer
//! demo agent <n> agree <call> <mine> <theirs>
//!                    agree a road between two hires as a mayor
//! demo agent <n> may-spend [<task>]
//! demo agent <n> charge <call> <micros> [<task>]
//!                    ask whether agent <n> may spend, or charge its
//!                    model call <call>, for <task> or its standing task
//! demo agent <n> call <call> <tokens in> <tokens out> [<task>]
//!                    a model call priced from the model copy: ask,
//!                    then charge
//! demo agent <n> read-budget <call> [<task>]
//! demo agent <n> spend <micros> <times>
//!                    up to <times> times, ask and charge <micros>
//! demo agent <n> letters <count> <to>
//!                    tell <to> <count> letters, one after another
//! demo agent <n> run <call> [net] <script ...>
//!                    run a script as agent <n>'s call <call>, with
//!                    the network when `net` is given
//! demo agent <n> copies
//!                    read the realm's copies for agent <n>
//! demo agent <n> keep-credential <provider> <key>
//! demo agent <n> inputs
//!                    list the inputs in agent <n>'s transcript
//! demo agent <n> await <kind> <count> [<text>]
//!                    wait until agent <n>'s transcript holds <count>
//!                    inputs of <kind>, only those whose text holds
//!                    <text> when given, and print how many it holds
//! demo agent <first>..<last> ...
//!                    run the command as each agent at once
//! demo ... &         run the command in the background
//! ```
//!
//! A `<task>` is a task's number, or `line:<k>`: the task of the
//! newest line agent `<k>` took. Each command prints `<name> <i64>`
//! lines: `founded/<name> <mayor>`, `hired/<call> <agent>`,
//! `accepted/<id> <task>`, `answered/<wait> 1`, `ended/<n> 1`,
//! `budget/<hamlet> <micros>`, `<verb>/<name> 1`
//! for `review`, `mode`, `folder`, `allowance`, `charter`, `model`,
//! `price` and `nickname`, `road/<a>/<b> 1`, `names/p50 <us>` and
//! `names/p95 <us>`, `live/<n> 1` or `0`, `sent/<call> 1`,
//! `replied/<question> 1`, `asked/<call> 1`, `objected/<call> 1`,
//! `started/<call> <worker>`, `<verb>/<n> 1` for `worker-done` and
//! `errand-done`, `waiting/<call> 1` or `laid/<call> 1`, `may/<n> 1`
//! or `0`, `charged/<call> <micros>`, `called/<call> <micros>`,
//! `own/<n> <micros>` and `allowance/<n> <micros>` (`-1` for none),
//! `spent/<n> <micros>`, `told/<n> <count>`, `copy/<n>/<part>
//! <version>`, `kept/<provider> 1`, `took/<n>/<kind> <count>`, and
//! for `run`, `done/<call>/<exit> <code>`, `running/<call> <command>`
//! or `waits/<call> <needs>`. A refusal prints `refused/<name> 1` and
//! logs why. `world` prints `world <revision>`, then `hamlet/<name>
//! <mayor>`, `agent/<n>/<status> <lead>`,
//! `reach/<n>/<none|applied|pending> <version>`, `wait/<n>/<call>
//! <wait>` for a tool's wait, and `spend/<payer> <micros>` with
//! `budget/<payer> <micros>` where a budget bounds it. `reads` prints
//! `read/<i> <us>`, the read's time, with `<n>`'s reach whenever it
//! changes, and then `reads/longest <us>`. `inputs` prints
//! `standing/<n> <task>`, then `input/<n>/<taken>/<kind> <task>` in
//! the order taken, and `question/<n>/<taken> <letter>` for a
//! question.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hamlet_actor::Action;
use hamlet_actor_core::ActorFactory;
use hamlet_actors::realm;
use hamlet_host::Host;
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::chat::Count;
use hamlet_messages::agent::copies::{Copies, Price, Versioned};
use hamlet_messages::profile::VersionName;
use hamlet_messages::realm::budget::{
    BudgetLimit, Charge, MaySpend, Payer,
};
use hamlet_messages::realm::choices::SetNickname;
use hamlet_messages::realm::client::{
    self, AnswerRefusal, ReadWorld, Say, SayAnswer,
};
use hamlet_messages::realm::documents::{
    CardSetting, EditCatalogue, EditCharter, EditSettings,
    ReadCopies, Setting,
};
use hamlet_messages::realm::hamlets::{
    Found, FoundAnswer, NameHamlet, SetFolder, SetMode,
};
use hamlet_messages::realm::letters::{LetterRefusal, Sent, Tell};
use hamlet_messages::realm::roads::LayRoad;
use hamlet_messages::realm::workers::SetAllowance;
use hamlet_messages::realm::world::{
    ReachSeen, Seen, Status, WaitKind,
};
use hamlet_messages::realm::{
    AgentRefusal, ChiefRequest, Done, End, HamletId, Hire,
    HireAnswer, Limit, Named, ProfileId, Review, SetLimits, Tier,
    Tool,
};
use hamlet_messages::workspace::reach::Mode;
use hamlet_messages::workspace::{
    AgentNumber, CallId, Exit, RealmTask,
};

use crate::agent_host::{forward, own_folder, taken};
use crate::command::{number, shown};
use crate::realm_agent::as_agent;

/// The realm actor, by kind.
pub fn kinds() -> [(String, ActorFactory); 1] {
    [(address::Realm::KIND.to_owned(), realm::factory())]
}

pub fn options() -> realm::Options {
    let defaults = realm::Options::default();
    realm::Options {
        chief: ["HAMLET_USER", "USER", "USERNAME", "LOGNAME"]
            .iter()
            .find_map(|name| {
                std::env::var(name).ok().filter(|v| !v.is_empty())
            })
            .unwrap_or(defaults.chief),
        ..defaults
    }
}

const DRIVE_WITHIN: Duration = Duration::from_secs(20);

pub const CONVERSATION: &str = "demo";

pub async fn drive(
    host: &Host,
    home: &Path,
    words: &[String],
) -> Result<()> {
    let [line @ .., last] = words else {
        bail!("a command takes words");
    };
    if last != "&" {
        return drive_now(host, home, words).await;
    }
    let host = host.clone();
    let home = home.to_owned();
    let line = line.to_vec();
    tokio::spawn(async move {
        if let Err(error) = drive_now(&host, &home, &line).await {
            let _ = writeln!(
                std::io::stdout(),
                "error: {}",
                shown(&error)
            );
        }
    });
    Ok(())
}

async fn drive_now(
    host: &Host,
    home: &Path,
    words: &[String],
) -> Result<()> {
    match words {
        [first, rest @ ..] if first == "realm" => {
            as_chief(host, home, rest).await
        }
        [first, agents, rest @ ..] if first == "agent" => {
            let Some((first, last)) = agents.split_once("..") else {
                return as_agent(host, agent_number(agents)?, rest)
                    .await;
            };
            let mut runs = tokio::task::JoinSet::new();
            for me in agent_number(first)?..=agent_number(last)? {
                let host = host.clone();
                let rest = rest.to_vec();
                runs.spawn(async move {
                    as_agent(&host, me, &rest).await
                });
            }
            while let Some(ran) = runs.join_next().await {
                ran??;
            }
            Ok(())
        }
        _ => bail!("usage: demo realm ... | agent <n> ..."),
    }
}

async fn as_chief(
    host: &Host,
    home: &Path,
    words: &[String],
) -> Result<()> {
    let mut out = std::io::stdout();
    match words {
        [verb, name] if verb == "found" => {
            let found = Found {
                name: name.clone(),
                mayor: None,
                version: None,
                tier: Tier::Fifty,
                purpose: None,
            };
            match request(host, found).await? {
                FoundAnswer::Founded { mayor, .. } => {
                    writeln!(out, "founded/{name} {}", mayor.0)?;
                }
                FoundAnswer::Refused { why } => refused(name, &why)?,
            }
        }
        [verb, lead, call, words @ ..]
            if verb == "hire" && !words.is_empty() =>
        {
            let (profile, version, purpose) = match words {
                [from, purpose @ ..] if from.starts_with("profile#") => {
                    let (profile, version) = profile_at(from)?;
                    (Some(profile), version, purpose)
                }
                purpose => (None, None, purpose),
            };
            let hire = Hire {
                hamlet: None,
                under: None,
                profile,
                version,
                purpose: (!purpose.is_empty()).then(|| purpose.join(" ")),
                name: None,
            };
            let tool = tool(call, hire);
            match forward(
                host,
                AgentNumber(agent_number(lead)?),
                &address::Realm,
                &tool,
            )
            .await?
            {
                HireAnswer::Hired { agent } => {
                    writeln!(out, "hired/{call} {}", agent.agent.0)?;
                }
                HireAnswer::Refused { why } => refused(call, &why)?,
            }
        }
        [verb, agent, id, text @ ..] if verb == "say" => {
            let to = agent_number(agent)?;
            let own = own_folder(home, AgentNumber(to))?;
            let text = text
                .iter()
                .map(|word| match word.strip_prefix("own:") {
                    Some(path) => format!("{}/{path}", own.display()),
                    None => word.clone(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            let line = Line {
                id: request_id(id),
                text,
                to: u64::from(to),
            };
            tracing::info!(id = line.id, "the request id of `{id}`");
            match as_client(host, Say::NAME, &line).await? {
                SayAnswer::Accepted { task } => {
                    writeln!(
                        out,
                        "accepted/{id} {}",
                        task_number(&task)
                    )?;
                }
                SayAnswer::Refused { why } => refused(id, &why)?,
            }
        }
        [verb, wait, choice, text @ ..] if verb == "answer" => {
            let choice = Choice {
                choice: match choice.as_str() {
                    "allow" => "allow",
                    "always" => "always",
                    "refuse" => "deny",
                    "go" => "go",
                    "withdraw" => "withdraw",
                    "reply" => "reply",
                    "dismiss" => "dismiss",
                    _ => bail!(
                        "an answer is allow, always, refuse, go, \
                         withdraw, reply or dismiss"
                    ),
                },
                id: fresh_id()?,
                text: (!text.is_empty()).then(|| text.join(" ")),
                wait: wait.clone(),
            };
            match as_client(host, client::Answer::NAME, &choice)
                .await?
            {
                Done::<AnswerRefusal>::Done => {
                    writeln!(out, "answered/{wait} 1")?;
                }
                Done::Refused { why } => refused(wait, &why)?,
            }
        }
        [verb, agent] if verb == "end" => {
            let end = End {
                hire: Named::Agent {
                    agent: AgentNumber(agent_number(agent)?),
                },
                why: None,
            };
            match request(host, end).await? {
                Done::Done => writeln!(out, "ended/{agent} 1")?,
                Done::Refused { why } => refused(agent, &why)?,
            }
        }
        [verb, name, micros] if verb == "budget" => {
            let limits = SetLimits {
                hamlet: Some(hamlet_named(host, name).await?),
                budget: vec![BudgetLimit::Cost {
                    micros: number(Some(micros))?,
                }],
                limits: Vec::new(),
            };
            match request(host, limits).await? {
                Done::Done => {
                    writeln!(out, "budget/{name} {micros}")?;
                }
                Done::Refused { why } => refused(name, &why)?,
            }
        }
        [verb, name, review] if verb == "review" => {
            let limits = SetLimits {
                hamlet: Some(hamlet_named(host, name).await?),
                budget: Vec::new(),
                limits: vec![Limit::Review {
                    review: match review.as_str() {
                        "always" => Review::Always,
                        "remote" => Review::Remote,
                        "never" => Review::Never,
                        _ => {
                            bail!("review is always, remote or never")
                        }
                    },
                }],
            };
            done("review", name, request(host, limits).await?)?;
        }
        [verb, name, mode] if verb == "mode" => {
            let mode = SetMode {
                hamlet: hamlet_named(host, name).await?,
                mode: match mode.as_str() {
                    "manual" => Mode::Manual,
                    "auto" => Mode::Auto,
                    _ => bail!("a mode is manual or auto"),
                },
            };
            done("mode", name, request(host, mode).await?)?;
        }
        [verb, name, folder] if verb == "folder" => {
            let folder = SetFolder {
                hamlet: hamlet_named(host, name).await?,
                folder: Some(folder.into()),
            };
            done("folder", name, request(host, folder).await?)?;
        }
        [verb, name, count] if verb == "names" => {
            let hamlet = hamlet_named(host, name).await?;
            let mut took = Vec::new();
            for time in 1..=number(Some(count))? {
                let begun = Instant::now();
                let named = NameHamlet {
                    hamlet: hamlet.clone(),
                    name: format!("{name}-{time}"),
                };
                if let Done::Refused { why } =
                    request(host, named).await?
                {
                    bail!("naming {name} was refused: {why}");
                }
                took.push(begun.elapsed().as_micros());
            }
            took.sort_unstable();
            let at =
                |share: usize| took[(took.len() - 1) * share / 100];
            writeln!(out, "names/p50 {}", at(50))?;
            writeln!(out, "names/p95 {}", at(95))?;
        }
        [verb, agent, micros] if verb == "allowance" => {
            let allowance = SetAllowance {
                hire: Named::Agent {
                    agent: AgentNumber(agent_number(agent)?),
                },
                micros: Some(Count(number(Some(micros))?)),
                why: None,
            };
            done(
                "allowance",
                agent,
                request(host, allowance).await?,
            )?;
        }
        [verb, text @ ..]
            if verb == "charter" && !text.is_empty() =>
        {
            let charter = EditCharter {
                text: Some(text.join(" ")),
            };
            done("charter", "realm", request(host, charter).await?)?;
        }
        [verb, model] if verb == "model" => {
            let settings = EditSettings {
                set: vec![Setting::Model {
                    model: model.clone(),
                }],
            };
            done("model", model, request(host, settings).await?)?;
        }
        [verb, model, input, output] if verb == "price" => {
            let catalogue = EditCatalogue {
                model: model.clone(),
                set: vec![CardSetting::Price {
                    price: Some(Price {
                        input: input.parse()?,
                        output: output.parse()?,
                        cache_read: 0.0,
                        cache_write: 0.0,
                    }),
                }],
            };
            done("price", model, request(host, catalogue).await?)?;
        }
        [verb, agent, name @ ..] if verb == "nickname" => {
            let nickname = SetNickname {
                hire: named(agent)?,
                name: (!name.is_empty()).then(|| name.join(" ")),
                why: None,
            };
            done("nickname", agent, request(host, nickname).await?)?;
        }
        [verb, a, b] if verb == "road" => {
            let road = LayRoad {
                a: named(a)?,
                b: named(b)?,
                why: None,
            };
            done("road", &format!("{a}/{b}"), request(host, road).await?)?;
        }
        [verb, agent] if verb == "live" => {
            let agent = agent_number(agent)?;
            let live = live(host, AgentNumber(agent)).await?;
            writeln!(out, "live/{agent} {}", i64::from(live))?;
        }
        [verb] if verb == "world" => {
            print_world(&read_world(host).await?)?;
        }
        [verb, count, every, watched @ ..] if verb == "reads" => {
            let every = Duration::from_millis(number(Some(every))?);
            let watched = watched
                .first()
                .map(|agent| agent_number(agent))
                .transpose()?;
            let mut longest = Duration::ZERO;
            let mut shown = None;
            for read in 1..=number(Some(count))? {
                let begun = Instant::now();
                let seen = read_world(host).await?;
                let took = begun.elapsed();
                longest = longest.max(took);
                writeln!(out, "read/{read} {}", took.as_micros())?;
                let reach = watched.and_then(|agent| {
                    seen.agents
                        .iter()
                        .find(|seen| seen.agent.0 == agent)
                        .map(|seen| (agent, reach_word(&seen.reach)))
                });
                if let Some((agent, (word, version))) = reach
                    && shown != Some((word, version))
                {
                    writeln!(out, "reach/{agent}/{word} {version}")?;
                    shown = Some((word, version));
                }
                tokio::time::sleep(every).await;
            }
            writeln!(out, "reads/longest {}", longest.as_micros())?;
        }
        _ => bail!(
            "usage: demo realm found|hire|say|answer|end|budget|\
             review|mode|folder|allowance|nickname|road|charter|model|\
             price|names|live|world|reads ..."
        ),
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct Request<T> {
    id: String,
    act: T,
}

#[derive(serde::Serialize)]
struct Line {
    id: String,
    text: String,
    to: u64,
}

#[derive(serde::Serialize)]
struct Choice {
    choice: &'static str,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    wait: String,
}

pub async fn request<T: ChiefRequest>(
    host: &Host,
    act: T,
) -> Result<T::Answer> {
    let request = Request {
        id: fresh_id()?,
        act,
    };
    as_client(host, T::NAME, &request).await
}

/// Waits until `agent` is live in its reach, at most
/// [`DRIVE_WITHIN`], and answers whether it is.
pub async fn live(host: &Host, agent: AgentNumber) -> Result<bool> {
    let live = until(move || async move {
        let seen = read_world(host).await?;
        Ok(seen
            .agents
            .iter()
            .any(|seen| {
                seen.agent == agent
                    && matches!(seen.status, Status::Live)
                    && matches!(seen.reach, ReachSeen::Applied { .. })
            })
            .then_some(()))
    })
    .await?;
    Ok(live.is_some())
}

async fn read_world(host: &Host) -> Result<Seen> {
    as_client(host, ReadWorld::NAME, &ReadWorld).await
}

async fn hamlet_named(host: &Host, name: &str) -> Result<HamletId> {
    read_world(host)
        .await?
        .hamlets
        .into_iter()
        .find(|hamlet| hamlet.name == name)
        .map(|hamlet| hamlet.hamlet)
        .with_context(|| format!("no hamlet is named `{name}`"))
}

/// Calls `look` every 50 ms until it finds something, for at most
/// [`DRIVE_WITHIN`].
pub async fn until<T, F: Future<Output = Result<Option<T>>>>(
    look: impl Fn() -> F,
) -> Result<Option<T>> {
    let end = Instant::now() + DRIVE_WITHIN;
    loop {
        if let Some(found) = look().await? {
            return Ok(Some(found));
        }
        if Instant::now() >= end {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The task a word names: `line:<n>` for the task of the newest line
/// agent `<n>` took, waiting for one, or else the task's own name.
pub async fn task_word(host: &Host, word: &str) -> Result<RealmTask> {
    let Some(agent) = word.strip_prefix("line:") else {
        return Ok(RealmTask(word.to_owned()));
    };
    let agent = agent_number(agent)?;
    until(move || async move {
        let holding = taken(host, AgentNumber(agent)).await?;
        Ok(holding
            .inputs
            .into_iter()
            .rev()
            .find(|input| input.kind == "line")
            .and_then(|input| input.task))
    })
    .await?
    .with_context(|| format!("agent {agent} took no line"))
}

/// Prints `<verb>/<name> 1`, or the refusal.
pub fn done<W: std::fmt::Display>(
    verb: &str,
    name: &str,
    done: Done<W>,
) -> Result<()> {
    match done {
        Done::Done => writeln!(std::io::stdout(), "{verb}/{name} 1")?,
        Done::Refused { why } => refused(name, &why)?,
    }
    Ok(())
}

async fn as_client<T: serde::de::DeserializeOwned>(
    host: &Host,
    name: &str,
    args: &impl serde::Serialize,
) -> Result<T> {
    let answer = host
        .call_for_client(
            &address::Realm.address()?,
            name,
            hamlet_actor::action::encode_positional(args)?,
            DRIVE_WITHIN,
        )
        .await?;
    Ok(ciborium::from_reader(answer.as_slice())?)
}

pub async fn read_copies(host: &Host, me: u32) -> Result<Copies> {
    let have = ReadCopies {
        charter: None,
        model: None,
        settings: None,
    };
    forward(host, AgentNumber(me), &address::Realm, &have).await
}

pub async fn tell(
    host: &Host,
    me: u32,
    call: &str,
    to: Named,
    text: String,
) -> Result<Sent<AgentRefusal<LetterRefusal>>> {
    let tell = Tell {
        to,
        text,
        paths: Vec::new(),
    };
    forward(host, AgentNumber(me), &address::Realm, &tool(call, tell))
        .await
}

pub async fn may_spend(
    host: &Host,
    me: u32,
    task: RealmTask,
) -> Result<bool> {
    match forward(
        host,
        AgentNumber(me),
        &address::Realm,
        &MaySpend { task },
    )
    .await?
    {
        Done::Done => Ok(true),
        Done::Refused { why } => {
            tracing::warn!(%why, "agent {me} may not spend");
            Ok(false)
        }
    }
}

pub async fn charge(
    host: &Host,
    me: u32,
    call: &str,
    task: RealmTask,
    micros: u32,
    tokens: Option<u32>,
) -> Result<()> {
    let charge = Charge {
        call: CallId(call.to_owned()),
        task,
        micros,
        took_ms: 1,
        tokens,
    };
    forward(host, AgentNumber(me), &address::Realm, &charge).await
}

pub async fn task_or_standing(
    host: &Host,
    me: u32,
    task: Option<&String>,
) -> Result<RealmTask> {
    match task {
        Some(task) => task_word(host, task).await,
        None => Ok(taken(host, AgentNumber(me)).await?.standing),
    }
}

pub fn tool<T>(call: &str, act: T) -> Tool<T> {
    Tool {
        call: CallId(call.to_owned()),
        task: None,
        conversation: None,
        act,
    }
}

pub fn named(word: &str) -> Result<Named> {
    Ok(match word {
        "hand" => Named::Hand,
        "mayor" => Named::Mayor,
        _ => Named::Agent {
            agent: AgentNumber(agent_number(
                word.strip_prefix("hire#").unwrap_or(word),
            )?),
        },
    })
}

/// The profile and version of `profile#<n>[@<version>]`.
fn profile_at(
    word: &str,
) -> Result<(ProfileId, Option<VersionName>)> {
    let named =
        word.strip_prefix("profile#").context("a profile#<n>")?;
    let (n, version) = match named.split_once('@') {
        Some((n, version)) => {
            (n, Some(VersionName(version.to_owned())))
        }
        None => (named, None),
    };
    Ok((ProfileId(agent_number(n)?), version))
}

pub fn agent_number(word: &str) -> Result<u32> {
    Ok(u32::try_from(number(Some(&word.to_owned()))?)?)
}

pub fn task_number(task: &RealmTask) -> i64 {
    task.0.parse().unwrap_or(-1)
}

/// A request id, the same for one word each time.
pub fn request_id(word: &str) -> String {
    let half = |seed: u64| {
        word.bytes().fold(seed, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    let hex = format!(
        "{:016x}{:016x}",
        half(0xcbf2_9ce4_8422_2325),
        half(0x8422_2325_cbf2_9ce4)
    );
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn fresh_id() -> Result<String> {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let made = MADE.fetch_add(1, Ordering::Relaxed);
    Ok(request_id(&format!(
        "{}-{}-{made}",
        now.as_nanos(),
        std::process::id()
    )))
}

pub fn refused(
    name: &str,
    why: &impl std::fmt::Display,
) -> Result<()> {
    tracing::warn!(%why, "{name} was refused");
    writeln!(std::io::stdout(), "refused/{name} 1")?;
    Ok(())
}

fn reach_word(reach: &ReachSeen) -> (&'static str, u64) {
    match reach {
        ReachSeen::None => ("none", 0),
        ReachSeen::Applied { version } => ("applied", version.0),
        ReachSeen::Pending { wanted, .. } => ("pending", wanted.0),
    }
}

fn print_world(seen: &Seen) -> Result<()> {
    let mut out = std::io::stdout();
    writeln!(out, "world {}", seen.revision.0)?;
    for hamlet in &seen.hamlets {
        writeln!(out, "hamlet/{} {}", hamlet.name, hamlet.mayor.0)?;
    }
    for agent in &seen.agents {
        let n = agent.agent.0;
        let status = match agent.status {
            Status::Hiring => "hiring",
            Status::Live => "live",
            Status::Ended { .. } => "ended",
        };
        let lead = agent.lead.map_or(0, |lead| lead.0);
        writeln!(out, "agent/{n}/{status} {lead}")?;
        let (word, version) = reach_word(&agent.reach);
        writeln!(out, "reach/{n}/{word} {version}")?;
    }
    for wait in &seen.waits {
        let WaitKind::Tool { call, .. } = &wait.kind else {
            continue;
        };
        writeln!(
            out,
            "wait/{}/{} {}",
            wait.agent.0, call.0, wait.wait.0
        )?;
    }
    for spend in &seen.spend {
        let payer = match &spend.payer {
            Payer::Realm => "realm",
            Payer::Hamlet { hamlet } => seen
                .hamlets
                .iter()
                .find(|seen| seen.hamlet == *hamlet)
                .map_or(hamlet.0.as_str(), |seen| seen.name.as_str()),
        };
        writeln!(out, "spend/{payer} {}", spend.spent_micros)?;
        if let Some(budget) = spend.budget_micros {
            writeln!(out, "budget/{payer} {budget}")?;
        }
    }
    Ok(())
}

/// Each part `copies` holds: its name, version and value as shown.
pub fn copy_parts(
    copies: &Copies,
) -> Vec<(&'static str, u32, String)> {
    fn part<T: std::fmt::Debug>(
        name: &'static str,
        copy: Option<&Versioned<T>>,
    ) -> Option<(&'static str, u32, String)> {
        copy.map(|copy| {
            (name, copy.version.0, format!("{:?}", copy.value))
        })
    }
    [
        part("charter", copies.charter.as_ref()),
        part("model", copies.model.as_ref()),
        part("settings", copies.settings.as_ref()),
    ]
    .into_iter()
    .flatten()
    .collect()
}

pub fn exit_word(exit: &Exit) -> (&'static str, i64) {
    match exit {
        Exit::Code { code } => ("exit", i64::from(*code)),
        Exit::TimedOut => ("timed_out", 0),
        Exit::Failed { .. } => ("failed", 0),
        Exit::CutOff => ("cut_off", 0),
        Exit::Cancelled => ("cancelled", 0),
    }
}
