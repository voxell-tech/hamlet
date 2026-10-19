//! Letters and questions along roads (H30, H12): each sent once from
//! its tool call into the receiver's conversation with the sender,
//! and its answer carried back.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Caller, outcome};
use crate::chats::records::{self, Place};
use crate::harness::link::{Item, ItemKind, Mark};
use crate::harness::{Answer, BindingId, Harnesses, Input};
use crate::record::{
    self, Actor, AgentRef, Change, Decision, DecisionKind,
    RecordOperation,
};
use crate::service::Service;
use crate::{Outcome, realm, society};

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

const SENT: &str = "SELECT request, kind, runtime, key, sender, \
                    receiver, from_hamlet, from_number, to_hamlet, \
                    to_number, address, task, text, at, answer, \
                    failure, carried, refused FROM society_message";

fn sent_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Sent> {
    let number = |at| {
        row.get::<_, i64>(at)
            .map(|n| u64::try_from(n).unwrap_or_default())
    };
    Ok(Sent {
        request: row.get(0)?,
        question: row.get::<_, String>(1)? == "question",
        runtime: row.get(2)?,
        key: row.get(3)?,
        sender: BindingId(row.get(4)?),
        receiver: BindingId(row.get(5)?),
        from: AgentRef {
            hamlet: row.get(6)?,
            number: number(7)?,
        },
        to: AgentRef {
            hamlet: row.get(8)?,
            number: number(9)?,
        },
        address: row.get(10)?,
        task: row.get(11)?,
        text: row.get(12)?,
        at: row.get(13)?,
        answer: row.get(14)?,
        failure: row.get(15)?,
        carried: row.get(16)?,
        refused: row.get(17)?,
    })
}

/// What the call `runtime`/`key` sent, if it sent anything.
fn sent_by(
    db: &Connection,
    runtime: &str,
    key: &str,
) -> Result<Option<Sent>> {
    Ok(db
        .query_row(
            &format!("{SENT} WHERE runtime = ?1 AND key = ?2"),
            params![runtime, key],
            sent_row,
        )
        .optional()?)
}

fn every(db: &Connection, filter: &str) -> Result<Vec<Sent>> {
    let mut query =
        db.prepare(&format!("{SENT} WHERE {filter} ORDER BY at"))?;
    Ok(query
        .query_map([], sent_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The words for an agent of a letter or question that was refused.
fn refused(text: &str) -> Value {
    outcome(text, false)
}

#[derive(Deserialize)]
struct Message {
    name: String,
    message: String,
}

#[derive(Deserialize)]
struct RoadAsk {
    to: String,
    reason: String,
}

/// `tell_agent`: a letter whose reply comes as a new message.
pub(super) async fn tell(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    args: &Value,
) -> Result<Value> {
    let Ok(given) = serde_json::from_value::<Message>(args.clone())
    else {
        return Ok(refused("`message` is empty"));
    };
    let db = service.home.store().connection();
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
    if turn_kind(service, caller)?.as_deref()
        == Some("hamlet.question")
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
        Err(words) => Ok(refused(&words)),
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
    args: &Value,
) -> Result<Option<Value>> {
    let Ok(given) = serde_json::from_value::<Message>(args.clone())
    else {
        return Ok(Some(refused("`message` is empty")));
    };
    let db = service.home.store().connection();
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
        Err(words) => return Ok(Asked::Refused(words)),
    };
    if sent.answer.is_none() && sent.failure.is_none() {
        let asked = service
            .home
            .store()
            .standing(sent.receiver, &sent.request)?
            .is_some();
        let db = service.home.store().connection();
        if !asked && let Some(address) = cycle(db, &sent)? {
            // It was counted on its road, and is not asked.
            db.execute(
                "DELETE FROM society_message WHERE request = ?1",
                [&sent.request],
            )?;
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
        let tx = service.home.store().db.transaction()?;
        carry_back(&tx, &sent)?;
        tx.execute(
            "UPDATE society_message SET carried = 1 WHERE request = \
             ?1",
            [&sent.request],
        )?;
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
fn cycle(db: &Connection, sent: &Sent) -> Result<Option<String>> {
    // An edge stands while its question waits and its asker's turn
    // runs.
    let mut query = db.prepare(
        "SELECT m.sender, m.receiver FROM society_message m JOIN \
         harness_input i ON i.binding = m.sender AND i.submission = \
         m.input WHERE m.kind = 'question' AND m.answer IS NULL AND \
         m.failure IS NULL AND m.request != ?1 AND \
         coalesce(i.status, '') NOT IN ('done', 'unanswered')",
    )?;
    let edges = query
        .query_map([&sent.request], |row| {
            Ok((BindingId(row.get(0)?), BindingId(row.get(1)?)))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
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
    args: &Value,
) -> Result<Option<Value>> {
    let Ok(given) = serde_json::from_value::<RoadAsk>(args.clone())
    else {
        return Ok(Some(refused("`message` is empty")));
    };
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
    let db = service.home.store().connection();
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
    let db = service.home.store().connection();
    if connected(db, &place, &target.agent)? {
        return Ok(Some(refused(&format!(
            "The mayor built the road. You can now ask_agent `{to}`."
        ))));
    }
    let tx = service.home.store().db.transaction()?;
    if let Some(sent) =
        sent_by(&tx, caller.id.runtime.as_str(), &caller.id.key)?
        && !sent.refused
        && let Some(mut r) = record::read(&tx, &place.hamlet)?
    {
        r.decisions.push(Decision {
            at: sent.at,
            by: "mayor".into(),
            reason: reply.chars().take(160).collect(),
            kind: DecisionKind::RoadRefused {
                from: place.number,
                to: to.into(),
            },
        });
        record::write(&tx, &r)?;
        tx.execute(
            "UPDATE society_message SET refused = 1 WHERE request = \
             ?1",
            [&sent.request],
        )?;
    }
    tx.commit()?;
    Ok(Some(refused(&format!(
        "The mayor did not build it: {reply}"
    ))))
}

/// The kind of the mark of the input whose turn made the call.
fn turn_kind(
    service: &mut Service,
    caller: &Caller<'_>,
) -> Result<Option<String>> {
    let Some(input) = caller.input else {
        return Ok(None);
    };
    let Some(request) =
        service.home.store().request_of(caller.binding.id, input)?
    else {
        return Ok(None);
    };
    let db = service.home.store().connection();
    Ok(records::mark_of(db, caller.binding.id, &request)?
        .map(|mark| mark.kind))
}

/// Whether a road joins `from` and `to`.
pub(crate) fn connected(
    db: &Connection,
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
/// a reply comes back even when the road has gone since. Gives the
/// words of why it may not go.
fn carry(
    db: &Connection,
    from: &AgentRef,
    to: &AgentRef,
    task: &str,
    reply: bool,
    at: i64,
) -> Result<std::result::Result<(), String>> {
    let messages = db
        .query_row(
            "SELECT messages FROM society_task WHERE task = ?1",
            [task],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    let limits = crate::spend::limits(db, &from.hamlet)?;
    let most = limits["task"]["messages"].as_i64().unwrap_or(12);
    if !reply && messages >= most {
        return Ok(Err(format!(
            "this task has used all {most} of its messages between \
             agents; finish with what you have and report. The \
             chief can raise it under One Task on the hamlet's \
             Bylaws page"
        )));
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
                record::change(db, &operation)?
                && !reply
            {
                return Ok(Err("There is no road between those \
                               agents."
                    .into()));
            }
        } else if !reply {
            return Ok(Err(
                "There is no road between those agents.".into()
            ));
        }
    } else if !realm::carry(db, from, to, at)? && !reply {
        return Ok(Err(
            "There is no road between those agents.".into()
        ));
    }
    db.execute(
        "INSERT INTO society_task (task, messages) VALUES (?1, 1) \
         ON CONFLICT (task) DO UPDATE SET messages = messages + 1",
        [task],
    )?;
    Ok(Ok(()))
}

/// Counts the way back of a message's answer, which no road or cap
/// refuses.
fn carry_back(db: &Connection, sent: &Sent) -> Result<()> {
    let _counted = carry(
        db,
        &sent.to,
        &sent.from,
        &sent.task,
        true,
        society::now(),
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
) -> Result<std::result::Result<Sent, String>> {
    let runtime = caller.id.runtime.as_str();
    let key = caller.id.key.as_str();
    let db = service.home.store().connection();
    if let Some(sent) = sent_by(db, runtime, key)? {
        return Ok(Ok(sent));
    }
    if record::trim(text).is_empty() {
        return Ok(Err("`message` is empty".into()));
    }
    let from = caller.place.agent.clone();
    let name = record::read(db, &to.hamlet)?
        .map(|r| r.address_of(to.number).to_owned())
        .unwrap_or_default();
    if name.is_empty() {
        return Ok(Err("The agent has left.".into()));
    }
    if !connected(db, &from, to)? {
        return Ok(Err(format!(
            "you have no road to `{name}`. Ask the mayor to pass a \
             single message on, or ask it for a road with \
             request_road."
        )));
    }
    let task = caller
        .task
        .clone()
        .unwrap_or_else(|| format!("message:{runtime}:{key}"));
    let receiver = peer(service, harnesses, to, kind, &from).await?;
    let mut words = serde_json::Value::String(text.into());
    service.credentials.scrub(&mut words);
    let text = words.as_str().unwrap_or_default().to_owned();
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
    let at = society::now();
    let tx = service.home.store().db.transaction()?;
    if let Err(words) = carry(&tx, &from, to, &task, false, at)? {
        return Ok(Err(words));
    }
    tx.execute(
        "INSERT INTO society_message (request, kind, runtime, key, \
         sender, receiver, from_hamlet, from_number, to_hamlet, \
         to_number, address, task, text, at, input) VALUES (?1, ?2, \
         ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            request,
            if kind == Kind::Letters {
                "letter"
            } else {
                "question"
            },
            runtime,
            key,
            caller.binding.id.0,
            receiver.0,
            from.hamlet,
            i64::try_from(from.number)?,
            to.hamlet,
            i64::try_from(to.number)?,
            address,
            task,
            text,
            at,
            caller.input
        ],
    )?;
    let sent =
        sent_by(&tx, runtime, key)?.context("a message just sent")?;
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
    let db = service.home.store().connection();
    let known = db
        .query_row(
            "SELECT binding FROM society_peer WHERE hamlet = ?1 AND \
             number = ?2 AND kind = ?3 AND from_hamlet = ?4 AND \
             from_number = ?5",
            params![
                to.hamlet,
                i64::try_from(to.number)?,
                kind.name(),
                from.hamlet,
                i64::try_from(from.number)?
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(binding) = known {
        return Ok(BindingId(binding));
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
    let db = service.home.store().connection();
    db.execute(
        "INSERT INTO society_peer (hamlet, number, kind, \
         from_hamlet, from_number, binding) VALUES (?1, ?2, ?3, ?4, \
         ?5, ?6)",
        params![
            to.hamlet,
            i64::try_from(to.number)?,
            kind.name(),
            from.hamlet,
            i64::try_from(from.number)?,
            binding.0
        ],
    )?;
    db.execute(
        "INSERT INTO society_binding (binding, payer) VALUES (?1, \
         ?2) ON CONFLICT (binding) DO UPDATE SET payer = \
         excluded.payer",
        params![binding.0, from.hamlet],
    )?;
    Ok(binding)
}

/// The sender of a message as its receiver reads it: its name or
/// office now, following where its conversation went.
fn sender(
    db: &Connection,
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
    let db = service.home.store().connection();
    let (from, address) = sender(db, sent)?;
    let from = json!({"hamlet": from.hamlet, "number": from.number});
    let (content, mark) = if sent.question {
        (
            format!(
                "Question from `{address}` (waiting for your \
                 answer):\n{}",
                sent.text
            ),
            mark(
                "hamlet.question",
                json!({"from": from, "task": sent.task, "at": sent.at}),
            ),
        )
    } else {
        (
            format!("Letter from `{address}`:\n{}", sent.text),
            mark(
                "hamlet.letter",
                json!({"from": from, "task": sent.task, "at": sent.at, "replyTo": sent.sender.0}),
            ),
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

fn mark(kind: &str, data: Value) -> Mark {
    Mark {
        kind: kind.into(),
        data: serde_json::from_value(data).unwrap_or_default(),
    }
}

/// The agent a conversation is with while it lives in a living
/// hamlet; a message to or from a conversation whose agent left is
/// not carried on.
fn delivery_agent(
    db: &Connection,
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
    let db = service.home.store().connection();
    for sent in every(db, "answer IS NULL AND failure IS NULL")? {
        let Some(standing) = service
            .home
            .store()
            .standing(sent.receiver, &sent.request)?
        else {
            continue;
        };
        let db = service.home.store().connection();
        let status = standing.status.as_deref().unwrap_or_default();
        let (answer, failure) = match status {
            "done" => {
                match answer_of(db, sent.receiver, &sent.request)? {
                    Some(answer) => (Some(answer), None),
                    // Its answer is not kept yet; it is read once it
                    // is.
                    None => continue,
                }
            }
            "unanswered" => (
                None,
                Some(standing.reason.clone().unwrap_or_else(|| {
                    "It could not finish.".into()
                })),
            ),
            _ => continue,
        };
        let mut words = json!({"answer": answer, "failure": failure});
        service.credentials.scrub(&mut words);
        service.home.store().connection().execute(
            "UPDATE society_message SET answer = ?2, failure = ?3 \
             WHERE request = ?1",
            params![
                sent.request,
                words["answer"].as_str(),
                words["failure"].as_str()
            ],
        )?;
        if sent.question
            && let Some(asker) =
                service.home.store().binding(sent.sender)?
        {
            harnesses.wake(&asker.runtime, &sent.key).await;
        }
    }
    let db = service.home.store().connection();
    for sent in every(
        db,
        "kind = 'letter' AND replied = 0 AND (answer IS NOT NULL OR \
         failure IS NOT NULL)",
    )? {
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
    let db = service.home.store().connection();
    let forget =
        |service: &mut Service, request: &str| -> Result<()> {
            service.home.store().connection().execute(
                "UPDATE society_message SET replied = 1 WHERE \
                 request = ?1",
                [request],
            )?;
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
        let tx = service.home.store().db.transaction()?;
        carry_back(&tx, &sent)?;
        tx.execute(
            "UPDATE society_message SET carried = 1 WHERE request = \
             ?1",
            [&sent.request],
        )?;
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
    let mark = mark(
        "hamlet.reply",
        json!({"from": {"hamlet": sent.to.hamlet, "number": sent.to.number}, "task": sent.task, "at": society::now()}),
    );
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
    db: &Connection,
    binding: BindingId,
    request: &str,
) -> Result<Option<String>> {
    let items = {
        let mut query = db.prepare(
            "SELECT item FROM harness_item WHERE binding = ?1 ORDER \
             BY ordinal",
        )?;
        query
            .query_map([binding.0], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
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
                        crate::harness::link::Block::Text {
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
    let db = service.home.store().connection();
    for sent in every(db, "answer IS NULL AND failure IS NULL")? {
        let db = service.home.store().connection();
        if delivery_agent(db, sent.receiver)?.is_some() {
            submit(service, harnesses, &sent).await?;
        }
    }
    sweep(service, harnesses).await
}

/// The peer conversations of `agent`: with whom, which kind, how many
/// messages and when the last was (C22), latest first.
pub(crate) fn views(
    db: &Connection,
    agent: &AgentRef,
) -> Result<Vec<Value>> {
    let mut query = db.prepare(
        "SELECT kind, from_hamlet, from_number, binding FROM \
         society_peer WHERE hamlet = ?1 AND number = ?2",
    )?;
    let peers = query
        .query_map(
            params![agent.hamlet, i64::try_from(agent.number)?],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut views = Vec::new();
    for (kind, hamlet, number, binding) in peers {
        let mut query = db.prepare(
            "SELECT json_extract(item, '$.mark.kind'), \
             json_extract(item, '$.mark.data.at'), \
             json_extract(item, '$.at') FROM harness_item WHERE \
             binding = ?1",
        )?;
        let mut count = 0;
        let mut last = None::<i64>;
        for row in query.query_map([binding], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })? {
            let (kind, marked, at) = row?;
            if matches!(
                kind.as_deref(),
                Some(
                    "hamlet.letter"
                        | "hamlet.question"
                        | "hamlet.reply"
                )
            ) {
                count += 1;
                last = last.max(marked);
            }
            last = last.max(at);
        }
        views.push(json!({"with": {"hamlet": hamlet, "number": number}, "kind": kind, "count": count, "last": last}));
    }
    views.sort_by_key(|view| {
        std::cmp::Reverse(view["last"].as_i64().unwrap_or(0))
    });
    Ok(views)
}

/// The peer conversation of `agent` with `from`, of `kind`, if any.
pub(crate) fn peer_of(
    db: &Connection,
    agent: &AgentRef,
    kind: Kind,
    from: &AgentRef,
) -> Result<Option<BindingId>> {
    Ok(db
        .query_row(
            "SELECT binding FROM society_peer WHERE hamlet = ?1 AND \
             number = ?2 AND kind = ?3 AND from_hamlet = ?4 AND \
             from_number = ?5",
            params![
                agent.hamlet,
                i64::try_from(agent.number)?,
                kind.name(),
                from.hamlet,
                i64::try_from(from.number)?
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map(BindingId))
}

/// Who wrote or asked in the peer conversation of `binding`, if it is
/// one.
pub(crate) fn sender_of(
    db: &Connection,
    binding: BindingId,
) -> Result<Option<AgentRef>> {
    Ok(db
        .query_row(
            "SELECT from_hamlet, from_number FROM society_peer \
             WHERE binding = ?1",
            [binding.0],
            |row| {
                Ok(AgentRef {
                    hamlet: row.get(0)?,
                    number: u64::try_from(row.get::<_, i64>(1)?)
                        .unwrap_or_default(),
                })
            },
        )
        .optional()?)
}

/// An answer for a call that waits on a message.
pub(super) fn waiting() -> Answer {
    Answer::Waiting(
        "This call waits for the other agent's answer.".into(),
    )
}

/// The author a letter, question or reply shows: its sender's name
/// with its hamlet, following the sender where it moved.
fn author(db: &Connection, original: &AgentRef) -> Result<String> {
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
    db: &Connection,
    items: &mut [Item],
) -> Result<()> {
    for item in items {
        let Some(mark) = item.mark.as_mut() else {
            continue;
        };
        if !matches!(
            mark.kind.as_str(),
            "hamlet.letter" | "hamlet.question" | "hamlet.reply"
        ) {
            continue;
        }
        let Some(from) = mark.data.get("from").and_then(|from| {
            serde_json::from_value::<AgentRef>(from.clone()).ok()
        }) else {
            continue;
        };
        mark.data.insert("author".into(), author(db, &from)?.into());
    }
    Ok(())
}
