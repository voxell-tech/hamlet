//! Letters and questions along roads (H30, H12): each sent once from
//! its tool call into the receiver's conversation with the sender,
//! and its answer carried back.

use anyhow::{Context, Result, bail};
use diesel::prelude::*;
use diesel::sql_types::Bool;
use diesel::sqlite::Sqlite;
use hamlet_protocol::client::{
    self as protocol, ConversationView, ConversationViewKind,
};
use hamlet_protocol::runtime::{Item, ItemKind};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Caller, SocietyOutcome};
use crate::chats::records::{self, Place};
use crate::harness::{
    Answer, BindingId, Harnesses, Input, InputMark, InputStatus,
    Letter,
};
use crate::record::{
    self, Actor, AgentRef, Change, DecisionKind, RecordOperation,
};
use crate::schema::{
    harness_input, harness_item, society_binding, society_message,
    society_peer, society_task,
};
use crate::service::Service;
use crate::{Outcome, realm};

/// The kind of a peer conversation, as clients name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Letters,
    Questions,
}

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Letters => "letters",
            Self::Questions => "questions",
        }
    }
}

/// A letter or question as it was sent.
#[derive(Clone, Debug)]
struct Sent {
    request: String,
    question: bool,
    runtime: String,
    key: String,
    sender: BindingId,
    receiver: BindingId,
    from: AgentRef,
    to: AgentRef,
    address: String,
    task: String,
    text: String,
    at: i64,
    answer: Option<String>,
    failure: Option<String>,
    carried: bool,
    refused: bool,
}

/// A [`Sent`] as `society_message` keeps it.
#[derive(Queryable, Selectable)]
#[diesel(table_name = society_message, check_for_backend(Sqlite))]
struct SentRow {
    request: String,
    kind: String,
    runtime: String,
    key: String,
    sender: i64,
    receiver: i64,
    from_hamlet: String,
    from_number: i64,
    to_hamlet: String,
    to_number: i64,
    address: String,
    task: String,
    text: String,
    at: i64,
    answer: Option<String>,
    failure: Option<String>,
    carried: bool,
    refused: bool,
}

impl From<SentRow> for Sent {
    fn from(row: SentRow) -> Self {
        let number = |n| u64::try_from(n).unwrap_or_default();
        Self {
            request: row.request,
            question: row.kind == "question",
            runtime: row.runtime,
            key: row.key,
            sender: BindingId(row.sender),
            receiver: BindingId(row.receiver),
            from: AgentRef {
                hamlet: row.from_hamlet,
                number: number(row.from_number),
            },
            to: AgentRef {
                hamlet: row.to_hamlet,
                number: number(row.to_number),
            },
            address: row.address,
            task: row.task,
            text: row.text,
            at: row.at,
            answer: row.answer,
            failure: row.failure,
            carried: row.carried,
            refused: row.refused,
        }
    }
}

/// What the call `runtime`/`key` sent, if it sent anything.
fn sent_by(
    db: &mut SqliteConnection,
    runtime: &str,
    key: &str,
) -> Result<Option<Sent>> {
    Ok(society_message::table
        .filter(society_message::runtime.eq(runtime))
        .filter(society_message::key.eq(key))
        .select(SentRow::as_select())
        .first(db)
        .optional()?
        .map(Sent::from))
}

/// A condition on the messages [`every`] reads.
type Filter = Box<
    dyn BoxableExpression<
            society_message::table,
            Sqlite,
            SqlType = Bool,
        >,
>;

fn every(
    db: &mut SqliteConnection,
    filter: Filter,
) -> Result<Vec<Sent>> {
    Ok(society_message::table
        .filter(filter)
        .order(society_message::at)
        .select(SentRow::as_select())
        .load(db)?
        .into_iter()
        .map(Sent::from)
        .collect())
}

/// The messages whose answer has not come.
fn unanswered() -> Filter {
    Box::new(
        society_message::answer
            .is_null()
            .and(society_message::failure.is_null()),
    )
}

/// Why a letter or question was not sent.
enum Unsent {
    Empty,
    Left,
    /// The sender has no road to the agent `name`.
    NoRoad {
        name: String,
    },
    /// The task has used all `most` of its messages.
    Spent {
        most: i64,
    },
    /// The road went as the message was counted.
    Unroaded,
}

impl Unsent {
    fn text(&self) -> String {
        match self {
            Self::Empty => "`message` is empty".into(),
            Self::Left => "The agent has left.".into(),
            Self::NoRoad { name } => format!(
                "you have no road to `{name}`. Ask the mayor to \
                 pass a single message on, or ask it for a road \
                 with request_road."
            ),
            Self::Spent { most } => format!(
                "this task has used all {most} of its messages \
                 between agents; finish with what you have and \
                 report. The chief can raise it under One Task on \
                 the hamlet's Bylaws page"
            ),
            Self::Unroaded => {
                "There is no road between those agents.".into()
            }
        }
    }
}

/// The words for an agent of a letter or question that was refused.
fn refused(text: &str) -> SocietyOutcome {
    SocietyOutcome::answered(text)
}

/// The arguments of `tell_agent` and `ask_agent`, and the question a
/// dealing asks the other mayor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Message {
    pub name: String,
    pub message: String,
}

/// The arguments of `request_road`.
#[derive(Deserialize)]
pub(crate) struct RoadAsk {
    to: String,
    reason: String,
}

/// `tell_agent`: a letter whose reply comes as a new message.
pub(super) async fn tell(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    given: Message,
) -> Result<SocietyOutcome> {
    let db = service.db();
    if let Some(sent) =
        sent_by(db, caller.id.runtime.as_str(), &caller.id.key)?
    {
        return Ok(refused(&sent_words(&sent.address)));
    }
    let place = &caller.place.agent;
    let target = match realm::resolve(db, &place.hamlet, &given.name)?
    {
        Ok(target) => target,
        Err(failure) => {
            return Ok(refused(&failure.text(&given.name)));
        }
    };
    if let Some(InputMark::Question(_)) = service
        .home
        .store()
        .input_mark(caller.binding.id, caller.input)?
    {
        return Ok(refused(
            "another agent is waiting on your reply, so you cannot \
             wait for a later one. Use ask_agent.",
        ));
    }
    if target.agent.hamlet != place.hamlet {
        return Ok(refused(&format!(
            "`{}` lives in another hamlet; use ask_agent to reach it",
            given.name
        )));
    }
    match prepare(
        service,
        harnesses,
        caller,
        &target.agent,
        &given.message,
        Kind::Letters,
    )
    .await?
    {
        Ok(sent) => Ok(refused(&sent_words(&sent.address))),
        Err(unsent) => Ok(refused(&unsent.text())),
    }
}

fn sent_words(address: &str) -> String {
    format!(
        "Sent. `{address}` is working on it; its reply will come to \
         you as a new message."
    )
}

/// `ask_agent`: a question whose answer the call waits for.
pub(crate) async fn ask(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    given: Message,
) -> Result<Option<SocietyOutcome>> {
    let db = service.db();
    let known =
        sent_by(db, caller.id.runtime.as_str(), &caller.id.key)?;
    let place = &caller.place.agent;
    let resolved = realm::resolve(db, &place.hamlet, &given.name)?;
    let (to, address) = match (known, resolved) {
        // The call waits on as the same call: its address is the
        // one it resolved, while it names the agent asked.
        (Some(sent), Ok(target)) if target.agent == sent.to => {
            (sent.to, target.address)
        }
        (Some(sent), _) => (sent.to, sent.address),
        (None, Ok(target)) => (target.agent, target.address),
        (None, Err(failure)) => {
            return Ok(Some(refused(&failure.text(&given.name))));
        }
    };
    Ok(
        match question(
            service,
            harnesses,
            caller,
            &to,
            &given.message,
        )
        .await?
        {
            Asked::Waiting => None,
            Asked::Answered(text) => {
                Some(refused(&format!("`{address}`: {text}")))
            }
            Asked::Refused(words) => Some(refused(&words)),
        },
    )
}

/// How a question stands for the call that asks it.
enum Asked {
    Waiting,
    Answered(String),
    Refused(String),
}

/// The question of the call of `caller` to `to`: sent once, then its
/// answer once it came, carried back along its road.
async fn question(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    to: &AgentRef,
    text: &str,
) -> Result<Asked> {
    let sent = match prepare(
        service,
        harnesses,
        caller,
        to,
        text,
        Kind::Questions,
    )
    .await?
    {
        Ok(sent) => sent,
        Err(unsent) => return Ok(Asked::Refused(unsent.text())),
    };
    if sent.answer.is_none() && sent.failure.is_none() {
        let asked = service
            .home
            .store()
            .standing(sent.receiver, &sent.request)?
            .is_some();
        let db = service.db();
        if !asked && let Some(address) = cycle(db, &sent)? {
            // It was counted on its road, and is not asked.
            diesel::delete(
                society_message::table.find(&sent.request),
            )
            .execute(db)?;
            return Ok(Asked::Refused(format!(
                "`{address}` is waiting on your answer through this \
                 chain of questions. Answer it in your reply \
                 instead of asking it again."
            )));
        }
        if !asked {
            submit(service, harnesses, &sent).await?;
        }
        return Ok(Asked::Waiting);
    }
    if !sent.carried {
        let mut tx = service.home.store().transaction()?;
        carry_back(&mut tx, &sent)?;
        diesel::update(society_message::table.find(&sent.request))
            .set(society_message::carried.eq(true))
            .execute(&mut *tx)?;
        tx.commit()?;
    }
    Ok(match (sent.answer, sent.failure) {
        (Some(answer), _) => Asked::Answered(answer),
        (None, failure) => Asked::Refused(format!(
            "`{}` could not finish: {}",
            sent.address,
            failure.unwrap_or_default()
        )),
    })
}

/// The address of the agent a question would wait on in a chain that
/// comes back to its asker, if it would.
fn cycle(
    db: &mut SqliteConnection,
    sent: &Sent,
) -> Result<Option<String>> {
    // An edge stands while its question waits and its asker's turn
    // runs.
    let edges = society_message::table
        .inner_join(harness_input::table.on(
            harness_input::binding.eq(society_message::sender).and(
                harness_input::submission.eq(society_message::input),
            ),
        ))
        .filter(society_message::kind.eq("question"))
        .filter(society_message::answer.is_null())
        .filter(society_message::failure.is_null())
        .filter(society_message::request.ne(&sent.request))
        .filter(crate::harness::unended())
        .select((society_message::sender, society_message::receiver))
        .load::<(i64, i64)>(db)?
        .into_iter()
        .map(|(from, to)| (BindingId(from), BindingId(to)))
        .collect::<Vec<_>>();
    let mut pending = vec![sent.receiver];
    let mut seen = Vec::new();
    while let Some(at) = pending.pop() {
        if seen.contains(&at) {
            continue;
        }
        if at == sent.sender {
            return Ok(Some(sent.address.clone()));
        }
        seen.push(at);
        pending.extend(
            edges
                .iter()
                .filter(|(from, _)| *from == at)
                .map(|(_, to)| *to),
        );
    }
    Ok(None)
}

/// `request_road`: an agent asks the mayor for a road, and waits for
/// the mayor's answer.
pub(super) async fn request_road(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    given: RoadAsk,
) -> Result<Option<SocietyOutcome>> {
    let place = caller.place.agent.clone();
    let Some(record) = service.home.store().record(&place.hamlet)?
    else {
        return Ok(Some(refused(
            "The mayor builds roads with build_road.",
        )));
    };
    if place.number == record.mayor {
        return Ok(Some(refused(
            "The mayor builds roads with build_road.",
        )));
    }
    let db = service.db();
    let target = match realm::resolve(db, &place.hamlet, &given.to)? {
        Ok(target) => target,
        Err(failure) => {
            return Ok(Some(refused(&failure.text(&given.to))));
        }
    };
    let known =
        sent_by(db, caller.id.runtime.as_str(), &caller.id.key)?;
    if known.is_none() && connected(db, &place, &target.agent)? {
        return Ok(Some(refused(&format!(
            "You already have a road to `{}`.",
            given.to
        ))));
    }
    let me = record.address_of(place.number).to_owned();
    let to = given.to.as_str();
    let text = format!(
        "`{me}` asks for a road to `{to}`. Its reason: {}\nDecide. \
         To agree, call build_road with your reason, or agree_road \
         if `{to}` lives in another hamlet. To refuse, say why in a \
         sentence.",
        given.reason
    );
    let mayor = AgentRef {
        hamlet: place.hamlet.clone(),
        number: known.as_ref().map_or(record.mayor, |s| s.to.number),
    };
    let reply =
        match question(service, harnesses, caller, &mayor, &text)
            .await?
        {
            Asked::Waiting => return Ok(None),
            Asked::Refused(words) => {
                return Ok(Some(refused(&words)));
            }
            Asked::Answered(text) => text,
        };
    let db = service.db();
    if connected(db, &place, &target.agent)? {
        return Ok(Some(refused(&format!(
            "The mayor built the road. You can now ask_agent `{to}`."
        ))));
    }
    let mut tx = service.home.store().transaction()?;
    if let Some(sent) =
        sent_by(&mut tx, caller.id.runtime.as_str(), &caller.id.key)?
        && !sent.refused
    {
        crate::operation::change(
            &mut tx,
            &RecordOperation {
                hamlet: place.hamlet.clone(),
                actor: Actor::Mayor(sent.to.clone()),
                at: sent.at,
                reason: reply.chars().take(160).collect(),
                task: None,
                change: Change::RefuseRoad {
                    from: place.number,
                    to: to.into(),
                },
            },
        )?;
        diesel::update(society_message::table.find(&sent.request))
            .set(society_message::refused.eq(true))
            .execute(&mut *tx)?;
    }
    tx.commit()?;
    Ok(Some(refused(&format!(
        "The mayor did not build it: {reply}"
    ))))
}

/// Whether a road joins `from` and `to`.
pub(crate) fn connected(
    db: &mut SqliteConnection,
    from: &AgentRef,
    to: &AgentRef,
) -> Result<bool> {
    if from.hamlet == to.hamlet {
        return Ok(record::read(db, &from.hamlet)?
            .is_some_and(|r| r.has_road(from.number, to.number)));
    }
    Ok(realm::roads(db)?.iter().any(|road| {
        (road.a == *from && road.b == *to)
            || (road.a == *to && road.b == *from)
    }))
}

/// Counts one message of `task` from `from` to `to` on their road;
/// a reply comes back even when the road has gone since. Gives why
/// it may not go.
fn carry(
    db: &mut SqliteConnection,
    from: &AgentRef,
    to: &AgentRef,
    task: &str,
    reply: bool,
    at: i64,
) -> Result<std::result::Result<(), Unsent>> {
    let messages = society_task::table
        .find(task)
        .select(society_task::messages)
        .first::<i64>(db)
        .optional()?
        .unwrap_or(0);
    let most = crate::limits::of(db, &from.hamlet)?.task.messages;
    if !reply && messages >= most {
        return Ok(Err(Unsent::Spent { most }));
    }
    if from.hamlet == to.hamlet {
        let road = record::read(db, &from.hamlet)?
            .is_some_and(|r| r.has_road(from.number, to.number));
        if road {
            let operation = RecordOperation {
                hamlet: from.hamlet.clone(),
                actor: Actor::Member(from.clone()),
                at,
                reason: String::new(),
                task: Some(task.into()),
                change: Change::CarryRoad { to: to.number },
            };
            if let Outcome::Refused(_) =
                crate::operation::change(db, &operation)?
                && !reply
            {
                return Ok(Err(Unsent::Unroaded));
            }
        } else if !reply {
            return Ok(Err(Unsent::Unroaded));
        }
    } else if !realm::carry(db, from, to, at)? && !reply {
        return Ok(Err(Unsent::Unroaded));
    }
    diesel::insert_into(society_task::table)
        .values((
            society_task::task.eq(task),
            society_task::messages.eq(1),
        ))
        .on_conflict(society_task::task)
        .do_update()
        .set(society_task::messages.eq(society_task::messages + 1))
        .execute(db)?;
    Ok(Ok(()))
}

/// Counts the way back of a message's answer, which no road or cap
/// refuses.
fn carry_back(db: &mut SqliteConnection, sent: &Sent) -> Result<()> {
    let _counted = carry(
        db,
        &sent.to,
        &sent.from,
        &sent.task,
        true,
        crate::clock::now(),
    )?;
    Ok(())
}

/// The letter or question of the call of `caller`: the one it sent,
/// else one sent now along a road and counted on it.
async fn prepare(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    to: &AgentRef,
    text: &str,
    kind: Kind,
) -> Result<std::result::Result<Sent, Unsent>> {
    let runtime = caller.id.runtime.as_str();
    let key = caller.id.key.as_str();
    let db = service.db();
    if let Some(sent) = sent_by(db, runtime, key)? {
        return Ok(Ok(sent));
    }
    if text.trim().is_empty() {
        return Ok(Err(Unsent::Empty));
    }
    let from = caller.place.agent.clone();
    let name = record::read(db, &to.hamlet)?
        .map(|r| r.address_of(to.number).to_owned())
        .unwrap_or_default();
    if name.is_empty() {
        return Ok(Err(Unsent::Left));
    }
    if !connected(db, &from, to)? {
        return Ok(Err(Unsent::NoRoad { name }));
    }
    let task = caller
        .task
        .clone()
        .unwrap_or_else(|| format!("message:{runtime}:{key}"));
    let receiver = peer(service, harnesses, to, kind, &from).await?;
    let text = service.credentials.scrub_text(text);
    let address = if from.hamlet == to.hamlet {
        name
    } else {
        format!("{name}@{}", to.hamlet)
    };
    let request = format!(
        "{}:{runtime}:{key}",
        if kind == Kind::Letters {
            "letter"
        } else {
            "ask"
        }
    );
    let at = crate::clock::now();
    let from_number = i64::try_from(from.number)?;
    let to_number = i64::try_from(to.number)?;
    let mut tx = service.home.store().transaction()?;
    if let Err(unsent) = carry(&mut tx, &from, to, &task, false, at)?
    {
        return Ok(Err(unsent));
    }
    diesel::insert_into(society_message::table)
        .values((
            society_message::request.eq(&request),
            society_message::kind.eq(if kind == Kind::Letters {
                "letter"
            } else {
                "question"
            }),
            society_message::runtime.eq(runtime),
            society_message::key.eq(key),
            society_message::sender.eq(caller.binding.id.0),
            society_message::receiver.eq(receiver.0),
            society_message::from_hamlet.eq(&from.hamlet),
            society_message::from_number.eq(from_number),
            society_message::to_hamlet.eq(&to.hamlet),
            society_message::to_number.eq(to_number),
            society_message::address.eq(&address),
            society_message::task.eq(&task),
            society_message::text.eq(&text),
            society_message::at.eq(at),
            society_message::input.eq(caller.input),
        ))
        .execute(&mut *tx)?;
    let sent = sent_by(&mut tx, runtime, key)?
        .context("a message just sent")?;
    tx.commit()?;
    if kind == Kind::Letters {
        submit(service, harnesses, &sent).await?;
    }
    Ok(Ok(sent))
}

/// The receiver's conversation with the sender of `kind`, which the
/// first message begins; charged to the sender's hamlet (H14).
async fn peer(
    service: &mut Service,
    harnesses: &mut Harnesses,
    to: &AgentRef,
    kind: Kind,
    from: &AgentRef,
) -> Result<BindingId> {
    if let Some(binding) = peer_of(service.db(), to, kind, from)? {
        return Ok(binding);
    }
    let binding = crate::chats::begin(
        service,
        harnesses,
        &Place {
            agent: to.clone(),
            chief: None,
            session: 0,
        },
        None,
        0,
    )
    .await?;
    let db = service.db();
    diesel::insert_into(society_peer::table)
        .values((
            society_peer::hamlet.eq(&to.hamlet),
            society_peer::number.eq(i64::try_from(to.number)?),
            society_peer::kind.eq(kind.name()),
            society_peer::from_hamlet.eq(&from.hamlet),
            society_peer::from_number.eq(i64::try_from(from.number)?),
            society_peer::binding.eq(binding.0),
        ))
        .execute(db)?;
    diesel::insert_into(society_binding::table)
        .values((
            society_binding::binding.eq(binding.0),
            society_binding::payer.eq(&from.hamlet),
        ))
        .on_conflict(society_binding::binding)
        .do_update()
        .set(
            society_binding::payer
                .eq(diesel::upsert::excluded(society_binding::payer)),
        )
        .execute(db)?;
    Ok(binding)
}

/// The sender of a message as its receiver reads it: its name or
/// office now, following where its conversation went.
fn sender(
    db: &mut SqliteConnection,
    sent: &Sent,
) -> Result<(AgentRef, String)> {
    let from = records::kept(db, sent.sender)?
        .map_or_else(|| sent.from.clone(), |kept| kept.place.agent);
    let name = record::read(db, &from.hamlet)?
        .map(|r| {
            let living = r.address_of(from.number);
            if living.is_empty() {
                r.archive
                    .iter()
                    .rev()
                    .find(|a| a.agent.number == from.number)
                    .map(|a| a.agent.name.clone())
                    .unwrap_or_default()
            } else {
                living.to_owned()
            }
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("agent #{}", from.number));
    let address = if from.hamlet == sent.to.hamlet {
        name
    } else {
        format!("{name}@{}", from.hamlet)
    };
    Ok((from, address))
}

/// Submits the message to its receiver's conversation; once per
/// request.
async fn submit(
    service: &mut Service,
    harnesses: &mut Harnesses,
    sent: &Sent,
) -> Result<()> {
    let db = service.db();
    let (from, address) = sender(db, sent)?;
    let message = Letter {
        from,
        task: sent.task.clone(),
        at: sent.at,
        reply_to: None,
        author: None,
    };
    let (content, mark) = if sent.question {
        (
            format!(
                "Question from `{address}` (waiting for your \
                 answer):\n{}",
                sent.text
            ),
            InputMark::Question(message),
        )
    } else {
        (
            format!("Letter from `{address}`:\n{}", sent.text),
            InputMark::Letter(Letter {
                reply_to: Some(sent.sender.0),
                ..message
            }),
        )
    };
    harnesses
        .submit(
            service.home.store(),
            sent.receiver,
            &Input {
                request: &sent.request,
                content: &content,
                steer: false,
                mark: Some(&mark),
                write: false,
                boundary: false,
            },
        )
        .await
}

/// The agent a conversation is with while it lives in a living
/// hamlet; a message to or from a conversation whose agent left is
/// not carried on.
fn delivery_agent(
    db: &mut SqliteConnection,
    binding: BindingId,
) -> Result<Option<AgentRef>> {
    let Some(kept) = records::kept(db, binding)? else {
        return Ok(None);
    };
    let agent = kept.place.agent;
    if !realm::live(db)?.contains(&agent.hamlet) {
        return Ok(None);
    }
    Ok(record::read(db, &agent.hamlet)?
        .is_some_and(|r| {
            r.agents.iter().any(|a| a.number == agent.number)
        })
        .then_some(agent))
}

/// Carries on with every message whose end came: keeps its answer,
/// wakes the call that waits on a question, and replies to a
/// letter's sender. Each step is kept, so it happens once.
pub(crate) async fn sweep(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let db = service.db();
    for sent in every(db, unanswered())? {
        let Some(standing) = service
            .home
            .store()
            .standing(sent.receiver, &sent.request)?
        else {
            continue;
        };
        let db = service.db();
        let (answer, failure) = match standing.status {
            Some(InputStatus::Done) => {
                match answer_of(db, sent.receiver, &sent.request)? {
                    Some(answer) => (Some(answer), None),
                    // Its answer is not kept yet; it is read once it
                    // is.
                    None => continue,
                }
            }
            Some(InputStatus::Unanswered) => (
                None,
                Some(standing.reason.clone().unwrap_or_else(|| {
                    "It could not finish.".into()
                })),
            ),
            _ => continue,
        };
        let mut words = json!({"answer": answer, "failure": failure});
        service.credentials.scrub(&mut words);
        diesel::update(society_message::table.find(&sent.request))
            .set((
                society_message::answer.eq(words["answer"].as_str()),
                society_message::failure
                    .eq(words["failure"].as_str()),
            ))
            .execute(service.db())?;
        if sent.question
            && let Some(asker) =
                service.home.store().binding(sent.sender)?
        {
            harnesses.wake(&asker.runtime, &sent.key).await;
        }
    }
    let db = service.db();
    let unreplied = Box::new(
        society_message::kind
            .eq("letter")
            .and(society_message::replied.eq(false))
            .and(
                society_message::answer
                    .is_not_null()
                    .or(society_message::failure.is_not_null()),
            ),
    );
    for sent in every(db, unreplied)? {
        reply(service, harnesses, sent).await?;
    }
    Ok(())
}

/// Submits a letter's answer to its sender's conversation, counted on
/// the road back.
async fn reply(
    service: &mut Service,
    harnesses: &mut Harnesses,
    mut sent: Sent,
) -> Result<()> {
    let db = service.db();
    let forget =
        |service: &mut Service, request: &str| -> Result<()> {
            diesel::update(society_message::table.find(request))
                .set(society_message::replied.eq(true))
                .execute(service.db())?;
            Ok(())
        };
    let Some(to) = delivery_agent(db, sent.receiver)? else {
        return forget(service, &sent.request);
    };
    sent.to = to;
    let Some(from) = delivery_agent(db, sent.sender)? else {
        return forget(service, &sent.request);
    };
    sent.from = from;
    if !sent.carried {
        let mut tx = service.home.store().transaction()?;
        carry_back(&mut tx, &sent)?;
        diesel::update(society_message::table.find(&sent.request))
            .set(society_message::carried.eq(true))
            .execute(&mut *tx)?;
        tx.commit()?;
    }
    let content = match (&sent.answer, &sent.failure) {
        (Some(answer), _) => {
            format!("Reply from `{}`: {answer}", sent.address)
        }
        (None, failure) => format!(
            "Reply from `{}`: it could not finish: {}",
            sent.address,
            failure.as_deref().unwrap_or_default()
        ),
    };
    let request = format!("reply:{}:{}", sent.runtime, sent.key);
    let mark = InputMark::Reply(Letter {
        from: sent.to.clone(),
        task: sent.task.clone(),
        at: crate::clock::now(),
        reply_to: None,
        author: None,
    });
    harnesses
        .submit(
            service.home.store(),
            sent.sender,
            &Input {
                request: &request,
                content: &content,
                steer: false,
                mark: Some(&mark),
                write: false,
                boundary: false,
            },
        )
        .await?;
    forget(service, &sent.request)
}

/// The answer the input `request` of `binding` ended with: the text
/// of the last answer of its turn, once it is kept.
pub(super) fn answer_of(
    db: &mut SqliteConnection,
    binding: BindingId,
    request: &str,
) -> Result<Option<String>> {
    let items = harness_item::table
        .filter(harness_item::binding.eq(binding.0))
        .order(harness_item::ordinal)
        .select(harness_item::item)
        .load::<String>(db)?;
    let items = items
        .iter()
        .map(|item| serde_json::from_str::<Item>(item))
        .collect::<serde_json::Result<Vec<_>>>()?;
    let Some(start) = items
        .iter()
        .position(|item| item.request.as_deref() == Some(request))
    else {
        return Ok(None);
    };
    let mut answer = None;
    for item in &items[start + 1..] {
        if item.kind == ItemKind::User
            && item.request.as_deref().is_some_and(|r| r != request)
        {
            break;
        }
        if item.kind == ItemKind::Assistant {
            answer = Some(
                item.blocks
                    .iter()
                    .flatten()
                    .filter_map(|block| match block {
                        hamlet_protocol::runtime::Block::Text {
                            text,
                        } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }
    Ok(answer)
}

/// Submits again what was sent and not yet answered, and carries on
/// with what ended while the server was away.
pub(crate) async fn resume(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let db = service.db();
    for sent in every(db, unanswered())? {
        let db = service.db();
        if delivery_agent(db, sent.receiver)?.is_some() {
            submit(service, harnesses, &sent).await?;
        }
    }
    sweep(service, harnesses).await
}

/// The peer conversations of `agent`: with whom, which kind, how many
/// messages and when the last was (C22), latest first.
pub(crate) fn views(
    db: &mut SqliteConnection,
    agent: &AgentRef,
) -> Result<Vec<ConversationView>> {
    let peers = society_peer::table
        .filter(society_peer::hamlet.eq(&agent.hamlet))
        .filter(society_peer::number.eq(i64::try_from(agent.number)?))
        .select((
            society_peer::kind,
            society_peer::from_hamlet,
            society_peer::from_number,
            society_peer::binding,
        ))
        .load::<(String, String, i64, i64)>(db)?;
    let mut views = Vec::new();
    for (kind, hamlet, number, binding) in peers {
        let mut count = 0;
        let mut last = None::<i64>;
        for item in crate::harness::items(db, BindingId(binding))? {
            if let Some(message) =
                InputMark::of_item(item.mark.as_ref())?
                    .as_ref()
                    .and_then(InputMark::message)
            {
                count += 1;
                last = last.max(Some(message.at));
            }
            last = last.max(i64::try_from(item.at).ok());
        }
        let kind = match kind.as_str() {
            "letters" => ConversationViewKind::Letters,
            "questions" => ConversationViewKind::Questions,
            other => {
                bail!("a peer conversation of no kind `{other}`")
            }
        };
        views.push(ConversationView {
            count,
            kind,
            last,
            with: protocol::AgentRef {
                number: u64::try_from(number).with_context(|| {
                    format!(
                        "a conversation's agent number in {hamlet}"
                    )
                })?,
                hamlet,
            },
        });
    }
    views.sort_by_key(|view| {
        std::cmp::Reverse(view.last.unwrap_or(0))
    });
    Ok(views)
}

/// The peer conversation of `agent` with `from`, of `kind`, if any.
pub(crate) fn peer_of(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    kind: Kind,
    from: &AgentRef,
) -> Result<Option<BindingId>> {
    Ok(society_peer::table
        .find((
            &agent.hamlet,
            i64::try_from(agent.number)?,
            kind.name(),
            &from.hamlet,
            i64::try_from(from.number)?,
        ))
        .select(society_peer::binding)
        .first::<i64>(db)
        .optional()?
        .map(BindingId))
}

/// Who wrote or asked in the peer conversation of `binding`, if it is
/// one.
pub(crate) fn sender_of(
    db: &mut SqliteConnection,
    binding: BindingId,
) -> Result<Option<AgentRef>> {
    Ok(society_peer::table
        .filter(society_peer::binding.eq(binding.0))
        .select((
            society_peer::from_hamlet,
            society_peer::from_number,
        ))
        .first::<(String, i64)>(db)
        .optional()?
        .map(|(hamlet, number)| AgentRef {
            hamlet,
            number: u64::try_from(number).unwrap_or_default(),
        }))
}

/// An answer for a call that waits on a message.
pub(super) fn waiting() -> Answer {
    Answer::Waiting(
        "This call waits for the other agent's answer.".into(),
    )
}

/// The author a letter, question or reply shows: its sender's name
/// with its hamlet, following the sender where it moved.
fn author(
    db: &mut SqliteConnection,
    original: &AgentRef,
) -> Result<String> {
    let mut at = original.clone();
    let mut seen = Vec::new();
    while !seen.contains(&at) {
        seen.push(at.clone());
        let Some(record) = record::read(db, &at.hamlet)? else {
            break;
        };
        if let Some(agent) = record
            .agents
            .iter()
            .find(|a| a.number == at.number)
            .or_else(|| {
                record
                    .archive
                    .iter()
                    .find(|a| a.agent.number == at.number)
                    .map(|a| &a.agent)
            })
        {
            return Ok(format!("{}@{}", agent.name, at.hamlet));
        }
        let Some((moved, name, to)) =
            record.decisions.iter().rev().find_map(|d| {
                match &d.kind {
                    DecisionKind::MovedOut {
                        agent,
                        name,
                        hamlet,
                    } if *agent == at.number => {
                        Some((d.at, name, hamlet))
                    }
                    _ => None,
                }
            })
        else {
            break;
        };
        let Some(arrival) =
            record::read(db, to)?.and_then(|target| {
                target.decisions.iter().find_map(|d| match &d.kind {
                    DecisionKind::MovedIn {
                        agent,
                        name: arrived,
                        hamlet,
                    } if *hamlet == at.hamlet
                        && arrived == name
                        && d.at == moved =>
                    {
                        Some(*agent)
                    }
                    _ => None,
                })
            })
        else {
            break;
        };
        at = AgentRef {
            hamlet: to.clone(),
            number: arrival,
        };
    }
    Ok(format!("{}#{}", original.hamlet, original.number))
}

/// Gives each letter, question and reply among `items` the author it
/// shows, as `author` in its mark.
pub(crate) fn authored(
    db: &mut SqliteConnection,
    items: &mut [Item],
) -> Result<()> {
    for item in items {
        let Some(mut mark) = InputMark::of_item(item.mark.as_ref())?
        else {
            continue;
        };
        let (InputMark::Letter(message)
        | InputMark::Question(message)
        | InputMark::Reply(message)) = &mut mark
        else {
            continue;
        };
        message.author = Some(author(db, &message.from)?);
        item.mark = Some((&mark).into());
    }
    Ok(())
}
