//! A chief's chats with the agents (C22-C31): their conversations,
//! what each turn does as it happens, and the deeds a chief does on
//! them.

mod activity;
mod calls;
mod deeds;
mod fold;
mod follow;
pub(crate) mod records;
mod say;
mod watch;

use std::collections::HashMap;

use anyhow::{Context, Result};
use hamlet_protocol::v0::{
    ChatRef, ClientMessage, Op, Read, ServerMessage, Turn, UsageView,
};
use serde_json::{Value, json};
use tokio::task::JoinSet;

use self::activity::Activity;
pub(crate) use self::activity::words;
pub(crate) use self::calls::{Ran, prepared, ran, specs, stop_tools};
pub(crate) use self::deeds::stop_agent;
use self::fold::Fold;
use self::records::Place;
pub(crate) use self::say::{admit_line, deeds as deed_names};
use crate::harness::link::Mark;
use crate::harness::{BindingId, Harnesses, Input, Notice};
use crate::protocol::{Refusal, reply};
use crate::record::AgentRef;
use crate::sandbox::Sandbox;
use crate::service::Service;

/// What the server holds of the chats while it runs.
pub(crate) struct Chats {
    conversations: HashMap<BindingId, Conversation>,
    watches: Vec<Watch>,
    /// Lines whose reply waits for their input to end.
    deferred: Vec<Deferred>,
    /// Messages for clients, sent once a request or event is
    /// handled.
    pub outbox: Vec<(u64, ServerMessage)>,
    /// The tool calls running for runtimes.
    pub tools: JoinSet<Ran>,
    running: HashMap<(String, String), calls::Running>,
    pub preparing: JoinSet<calls::PreparedCall>,
    preparations: HashMap<uuid::Uuid, calls::Preparing>,
    /// The notice of a stop, written once its runtime stopped the
    /// turn.
    stopping: HashMap<BindingId, String>,
    /// The home is closing: a reflection's call that asks a chief
    /// does not run.
    pub closing: bool,
    pub sandbox: Sandbox,
}

/// One conversation as its runtime last told it.
struct Conversation {
    fold: Fold,
    activity: Activity,
    running: bool,
    pending: u64,
    /// The runtime's submission id of the running turn's input.
    turn: Option<String>,
    /// The usage last shown to its watchers.
    usage: Option<UsageView>,
}

/// A client's watch of one chat.
struct Watch {
    client: u64,
    chat: ChatRef,
    binding: BindingId,
}

/// A reply that waits for an input: for its end, or for a write to
/// be kept.
struct Deferred {
    client: u64,
    id: String,
    binding: BindingId,
    request: String,
    text: String,
    kept: bool,
}

/// What a client shows of an agent's work.
pub(crate) struct Work {
    pub busy: bool,
    pub faulted: bool,
    pub doing: &'static str,
    pub step: i64,
    pub failures: i64,
    pub saying: String,
    pub thought: bool,
}

impl Chats {
    pub fn new(sandbox: Sandbox) -> Self {
        Self {
            conversations: HashMap::new(),
            watches: Vec::new(),
            deferred: Vec::new(),
            outbox: Vec::new(),
            tools: JoinSet::new(),
            running: HashMap::new(),
            preparing: JoinSet::new(),
            preparations: HashMap::new(),
            stopping: HashMap::new(),
            closing: false,
            sandbox,
        }
    }

    /// Forgets the watches and replies of a client that is gone.
    pub fn gone(&mut self, client: u64) {
        self.watches.retain(|watch| watch.client != client);
        self.deferred.retain(|line| line.client != client);
    }

    /// Stops every tool call that is running.
    pub fn stop_tools(&mut self) {
        self.tools.abort_all();
        self.running.clear();
        self.preparing.abort_all();
        self.preparations.clear();
    }
}

/// Handles a client's request about conversations; `None` when it is
/// another owner's, or is answered later.
pub(crate) async fn converse(
    service: &mut Service,
    harnesses: &mut Harnesses,
    client: u64,
    message: &ClientMessage,
) -> Result<Option<Option<ServerMessage>>> {
    let answer = match message {
        ClientMessage::Say(said) => {
            say::say(service, harnesses, client, said).await?
        }
        ClientMessage::Open(open) => {
            watch::open(service, harnesses, client, open).await?
        }
        ClientMessage::Close(close) => {
            let key = watch::key(&close.chat);
            service.chats.watches.retain(|watch| {
                watch.client != client
                    || watch::key(&watch.chat) != key
            });
            None
        }
        ClientMessage::New(new) => {
            let done = deeds::protocol(
                service,
                harnesses,
                &new.to,
                deeds::Deed::New,
            )
            .await?;
            reply_done(service, client, &new.id, done)
        }
        ClientMessage::Resume(resume) => {
            let done = deeds::protocol(
                service,
                harnesses,
                &resume.to,
                deeds::Deed::Resume(resume.number),
            )
            .await?;
            reply_done(service, client, &resume.id, done)
        }
        ClientMessage::Retry(retry) => {
            let done = deeds::protocol(
                service,
                harnesses,
                &retry.to,
                deeds::Deed::Retry,
            )
            .await?;
            reply_done(service, client, &retry.id, done)
        }
        ClientMessage::Rewind(rewind) => {
            Some(deeds::rewind(service, harnesses, rewind).await?)
        }
        ClientMessage::Stop(stop) => Some(reply(
            &stop.id,
            deeds::stop(service, harnesses, stop.agent.as_ref())
                .await?,
        )),
        ClientMessage::Answer(answer) => {
            let choice = serde_json::to_value(&answer.choice)?;
            Some(reply(
                &answer.id,
                calls::answer_words(
                    service,
                    harnesses,
                    &answer.wait,
                    choice.as_str().unwrap_or_default(),
                    answer.text.as_deref().unwrap_or_default(),
                    answer.picks.as_deref(),
                )
                .await?
                .map_err(Refusal::Invalid),
            ))
        }
        ClientMessage::Read(Read::History { agent, id }) => {
            Some(watch::history(service, harnesses, agent, id).await?)
        }
        ClientMessage::Give(_) | ClientMessage::GivePart(_) => {
            crate::gifts::request(service, harnesses, client, message)
                .await?
        }
        _ => return Ok(None),
    };
    refresh_watches(service, harnesses).await?;
    Ok(Some(answer))
}

/// Takes in what a runtime told.
pub(crate) async fn heard(
    service: &mut Service,
    harnesses: &mut Harnesses,
    notice: Notice,
) -> Result<()> {
    match notice {
        Notice::Kept { binding, mut item } => {
            crate::society::letters::authored(
                service.home.store().connection(),
                std::slice::from_mut(&mut item),
            )?;
            let loaded =
                service.chats.conversations.contains_key(&binding);
            let conversation = conversation(service, binding)?;
            if loaded {
                conversation.fold.entry(&item);
                conversation.activity.kept(&item);
            }
            broadcast(service, binding)?;
            if let Some(request) = &item.request {
                answer_deferred(service, binding, request, true);
            }
        }
        Notice::Live {
            binding,
            running,
            pending,
            partial,
        } => {
            let secrets = service.credentials.secrets();
            let conversation = conversation(service, binding)?;
            conversation.running = running;
            conversation.pending = pending;
            if !running {
                conversation.turn = None;
            }
            conversation.fold.live(
                running,
                pending,
                partial.as_ref(),
                &secrets,
            );
            conversation.activity.live(
                running,
                pending,
                partial.as_ref(),
            );
            broadcast(service, binding)?;
        }
        Notice::Attempted { binding } => {
            conversation(service, binding)?.activity.attempted();
        }
        Notice::Admitted {
            binding,
            request,
            standing,
        }
        | Notice::Settled {
            binding,
            request,
            standing,
        } => {
            if matches!(
                standing.status.as_deref(),
                Some("done" | "unanswered")
            ) {
                follow::follow(service, harnesses, binding, &request)
                    .await?;
                answer_deferred(service, binding, &request, false);
                crate::society::letters::sweep(service, harnesses)
                    .await?;
                crate::society::errands::sweep(service, harnesses)
                    .await?;
            }
        }
        Notice::Call { binding, re, call } => {
            calls::call(service, harnesses, binding, re, call)
                .await?;
        }
        Notice::Cancelled { binding } => {
            if let Some(text) =
                service.chats.stopping.remove(&binding)
            {
                write_notice(service, harnesses, binding, &text)
                    .await?;
            }
        }
        notice => report(service, &notice),
    }
    refresh_watches(service, harnesses).await
}

/// Logs what a chief must hear of the runtimes until protocol v0
/// shows it.
fn report(service: &Service, notice: &Notice) {
    if let Notice::Refused { runtime, words }
    | Notice::Fault { runtime, words } = notice
    {
        let mut words = Value::String(words.clone());
        service.credentials.scrub(&mut words);
        eprintln!(
            "hamlet-server: {}: {}",
            runtime.as_str(),
            words.as_str().unwrap_or_default()
        );
    }
}

/// Carries on with inputs that ended while the server was away.
pub(crate) async fn start(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    crate::society::leaving::recover(service, harnesses).await?;
    crate::society::reviews::sweep(service, harnesses).await?;
    for (binding, request) in service.home.store().unfollowed()? {
        follow::follow(service, harnesses, binding, &request).await?;
    }
    crate::society::merging::finish(service).await?;
    crate::society::errands::sweep(service, harnesses).await?;
    crate::society::reconfigure(service, harnesses).await?;
    crate::society::letters::resume(service, harnesses).await
}

/// Requests the reflections owed as the home closes; gives the inputs
/// to wait for.
pub(crate) async fn close(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<Vec<(BindingId, String)>> {
    service.chats.closing = true;
    if follow::reflect_every(service)? == 0 {
        return Ok(Vec::new());
    }
    let mut waits = Vec::new();
    for binding in records::all(service.home.store().connection())? {
        let Some(kept) = records::kept(
            service.home.store().connection(),
            binding,
        )?
        else {
            continue;
        };
        if kept.place.chief.is_none() {
            continue;
        }
        let Some(request) = follow::request_reflection(
            service,
            harnesses,
            binding,
            follow::Mode::Owed,
        )
        .await?
        else {
            continue;
        };
        // A running chief turn is suspended by the close, not waited
        // on for a reflection.
        let running = service
            .chats
            .conversations
            .get(&binding)
            .is_some_and(|c| c.running);
        let reflecting = turn_request(service, binding)?
            .is_some_and(|request| request.starts_with("reflect:"));
        if !running || reflecting {
            waits.push((binding, request));
        }
    }
    Ok(waits)
}

/// What a client shows of `agent`'s work: the turn running, else the
/// chief's current chat, else its latest conversation.
pub(crate) fn work(
    service: &mut Service,
    agent: &AgentRef,
) -> Result<Work> {
    let db = service.home.store().connection();
    let bindings = records::conversations_of(db, agent)?;
    let current = records::current(db, agent, &service.chief)?
        .map(|session| session.binding);
    let chats = &service.chats;
    let busy = bindings.iter().any(|binding| {
        chats
            .conversations
            .get(binding)
            .is_some_and(|c| c.running || c.pending > 0)
    });
    let active = bindings
        .iter()
        .find(|b| {
            chats.conversations.get(b).is_some_and(|c| c.running)
        })
        .or(current.as_ref())
        .or(bindings.last())
        .and_then(|binding| chats.conversations.get(binding));
    let faulted = current
        .and_then(|binding| chats.conversations.get(&binding))
        .is_some_and(|c| c.activity.faulted);
    Ok(match active {
        Some(c) => Work {
            busy,
            faulted: !busy && faulted,
            doing: c.activity.doing,
            step: c.activity.step,
            failures: c.activity.failures,
            saying: c.activity.saying.clone(),
            thought: c.activity.thought,
        },
        None => Work {
            busy,
            faulted: !busy && faulted,
            doing: "rest",
            step: 0,
            failures: 0,
            saying: String::new(),
            thought: false,
        },
    })
}

/// Loads every conversation an agent has, so what it is doing shows.
pub(crate) fn load(service: &mut Service) -> Result<()> {
    for binding in records::all(service.home.store().connection())? {
        conversation(service, binding)?;
    }
    Ok(())
}

fn conversation(
    service: &mut Service,
    binding: BindingId,
) -> Result<&mut Conversation> {
    if !service.chats.conversations.contains_key(&binding) {
        let mut items = service.home.store().items(binding)?;
        crate::society::letters::authored(
            service.home.store().connection(),
            &mut items,
        )?;
        service.chats.conversations.insert(
            binding,
            Conversation {
                fold: Fold::of(&items),
                activity: Activity::of(&items),
                running: false,
                pending: 0,
                turn: None,
                usage: None,
            },
        );
    }
    service
        .chats
        .conversations
        .get_mut(&binding)
        .context("a conversation just loaded")
}

/// Sends the ops of `binding` to the clients watching it, with its
/// usage when that changed.
fn broadcast(
    service: &mut Service,
    binding: BindingId,
) -> Result<()> {
    let watched = service
        .chats
        .watches
        .iter()
        .any(|watch| watch.binding == binding);
    let usage = if watched {
        Some(usage(service, binding)?)
    } else {
        None
    };
    let number =
        records::kept(service.home.store().connection(), binding)?
            .map(|kept| kept.place.session);
    let conversation = conversation(service, binding)?;
    let mut ops = conversation.fold.take();
    let stale = std::mem::take(&mut conversation.fold.stale);
    if stale {
        ops.push(Op::Reset {});
        let mut items = service.home.store().items(binding)?;
        crate::society::letters::authored(
            service.home.store().connection(),
            &mut items,
        )?;
        let conversation = self::conversation(service, binding)?;
        let mut fold = Fold::of(&items);
        fold.working = conversation.running;
        fold.pending = conversation.pending;
        fold.take();
        conversation.fold = fold;
    }
    let conversation = self::conversation(service, binding)?;
    if let Some(usage) = usage
        && conversation.usage.as_ref() != Some(&usage)
    {
        conversation.usage = Some(usage.clone());
        ops.push(Op::Usage { usage });
    }
    if ops.is_empty() {
        return Ok(());
    }
    let watchers = service
        .chats
        .watches
        .iter()
        .filter(|watch| watch.binding == binding)
        .map(|watch| (watch.client, watch.chat.clone()))
        .collect::<Vec<_>>();
    for (client, chat) in watchers {
        let number = chat.from.is_none().then_some(number).flatten();
        service.chats.outbox.push((
            client,
            ServerMessage::Turn(Turn {
                chat: chat.clone(),
                number,
                ops: ops.clone(),
            }),
        ));
        if stale {
            let message = watch::transcript(
                service, binding, &chat, None, None,
            )?;
            service.chats.outbox.push((client, message));
        }
    }
    Ok(())
}

/// Rebinds each watch of a chat's current conversation that changed,
/// resetting what its client shows.
async fn refresh_watches(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let watches = service
        .chats
        .watches
        .iter()
        .enumerate()
        .filter(|(_, watch)| {
            watch.chat.past.is_none() && watch.chat.from.is_none()
        })
        .map(|(at, watch)| (at, watch.chat.clone(), watch.binding))
        .collect::<Vec<_>>();
    for (at, chat, binding) in watches {
        let now =
            match watch::resolve(service, harnesses, &chat).await? {
                Ok(now) => now,
                Err(_) => continue,
            };
        if now == binding {
            continue;
        }
        let client = service.chats.watches[at].client;
        service.chats.watches[at].binding = now;
        service.chats.outbox.push((
            client,
            ServerMessage::Turn(Turn {
                chat: chat.clone(),
                number: None,
                ops: vec![Op::Reset {}],
            }),
        ));
        let message =
            watch::transcript(service, now, &chat, None, None)?;
        service.chats.outbox.push((client, message));
    }
    Ok(())
}

/// Sends the replies that waited for the input `request` to end, or
/// to be kept when `kept`.
fn answer_deferred(
    service: &mut Service,
    binding: BindingId,
    request: &str,
    kept: bool,
) {
    let chats = &mut service.chats;
    let (done, rest) = std::mem::take(&mut chats.deferred)
        .into_iter()
        .partition::<Vec<_>, _>(|line| {
            line.binding == binding
                && line.request == request
                && line.kept == kept
        });
    chats.deferred = rest;
    for line in done {
        chats
            .outbox
            .push((line.client, reply(&line.id, Ok(line.text))));
    }
}

/// Replies `done` to the request `id`, once the notice it wrote is
/// kept, so what the client opens next shows it.
fn reply_done(
    service: &mut Service,
    client: u64,
    id: &str,
    done: std::result::Result<deeds::Done, Refusal>,
) -> Option<ServerMessage> {
    match done {
        Ok(deeds::Done {
            text,
            written: Some((binding, request)),
            ..
        }) => {
            service.chats.deferred.push(Deferred {
                client,
                id: id.into(),
                binding,
                request,
                text,
                kept: true,
            });
            None
        }
        done => Some(reply(id, done.map(|done| done.text))),
    }
}

/// The usage of the conversation of `binding` and the branches it
/// came from.
fn usage(
    service: &mut Service,
    binding: BindingId,
) -> Result<UsageView> {
    let db = service.home.store().connection();
    crate::spend::usage_of(db, &records::lineage(db, binding)?)
}

/// The request of the input whose turn `binding` runs now.
fn turn_request(
    service: &mut Service,
    binding: BindingId,
) -> Result<Option<String>> {
    let Some(turn) = service
        .chats
        .conversations
        .get(&binding)
        .and_then(|c| c.turn.clone())
    else {
        return Ok(None);
    };
    service.home.store().request_of(binding, &turn)
}

/// The current conversation of `chief`'s chat with `agent`, which its
/// first use starts.
pub(crate) async fn current(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
) -> Result<BindingId> {
    let chief = service.chief.clone();
    if let Some(session) = records::current(
        service.home.store().connection(),
        agent,
        &chief,
    )? {
        return Ok(session.binding);
    }
    let binding = begin(
        service,
        harnesses,
        &Place {
            agent: agent.clone(),
            chief: Some(chief.clone()),
            session: 1,
        },
        None,
        0,
    )
    .await?;
    records::point(
        service.home.store().connection(),
        agent,
        &chief,
        1,
        binding,
        now(),
    )?;
    Ok(binding)
}

/// Binds a new conversation with `place` and keeps whom it is with.
pub(crate) async fn begin(
    service: &mut Service,
    harnesses: &mut Harnesses,
    place: &Place,
    previous: Option<BindingId>,
    unreflected: i64,
) -> Result<BindingId> {
    let (binding, notices) = harnesses
        .bind(service.home.store(), &place.agent, false)
        .await?;
    records::keep(
        service.home.store().connection(),
        binding,
        place,
        previous,
        unreflected,
    )?;
    for notice in notices {
        report(service, &notice);
    }
    crate::society::configure(service, harnesses, binding).await?;
    Ok(binding)
}

/// Writes the notice `text` into every chief's chat with the mayor of
/// `hamlet`, beginning the chat when it is new.
pub(crate) async fn tell_chiefs(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    text: &str,
) -> Result<()> {
    let Some(record) = service.home.store().record(hamlet)? else {
        return Ok(());
    };
    let mayor = AgentRef {
        hamlet: hamlet.into(),
        number: record.mayor,
    };
    let binding = current(service, harnesses, &mayor).await?;
    write_notice(service, harnesses, binding, text).await?;
    Ok(())
}

/// Writes a notice into the conversation of `binding`, for people
/// alone.
/// Writes the notice `text` into the conversation of `binding`;
/// gives the write's request.
async fn write_notice(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
    text: &str,
) -> Result<String> {
    let request = format!("notice:{}", uuid::Uuid::new_v4());
    write(service, harnesses, binding, &request, text, false).await?;
    Ok(request)
}

pub(crate) async fn write(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
    request: &str,
    text: &str,
    boundary: bool,
) -> Result<()> {
    let mark = notice(text);
    harnesses
        .submit(
            service.home.store(),
            binding,
            &Input {
                request,
                content: "",
                steer: false,
                mark: Some(&mark),
                write: true,
                boundary,
            },
        )
        .await
}

/// The mark of a notice: words for people, which the model never
/// sees (C30).
pub(crate) fn notice(text: &str) -> Mark {
    mark("hamlet.notice", json!({"text": text, "at": now()}))
}

fn mark(kind: &str, data: Value) -> Mark {
    Mark {
        kind: kind.into(),
        data: serde_json::from_value(data).unwrap_or_default(),
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// A new H12 task, begun now.
fn new_task() -> String {
    format!("{}-{}", now(), uuid::Uuid::new_v4())
}
