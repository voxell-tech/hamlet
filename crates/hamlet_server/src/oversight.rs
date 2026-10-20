//! What waits for a chief (H11, H34): approvals of held tool calls,
//! faults, and questions, each kept until it is answered or lapses.

use anyhow::{Context, Result};
use hamlet_protocol::asking::Questions;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::RuntimeInstance;
use crate::change::clip;
use crate::record::{AgentRef, Hamlet};

/// A chief's chat a wait was raised in, with its session number.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChatPlace {
    pub hamlet: String,
    pub number: u64,
    pub chief: String,
    pub session: i64,
}

/// Something that waits for a chief.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Wait {
    pub id: String,
    /// `approval`, `fault`, `question` or `objection`.
    pub kind: String,
    pub danger: Option<String>,
    /// The name of the agent that raised it.
    pub by: String,
    pub agent: Option<AgentRef>,
    /// The exact text the chief reads.
    pub about: String,
    pub change: Option<Value>,
    pub questions: Option<Questions>,
    pub answers: Vec<String>,
    pub since: i64,
    /// Times it was asked while it waited.
    pub asked: i64,
    pub chat: Option<ChatPlace>,
}

/// A wait to raise, and the tool call it holds.
pub(crate) struct Raising {
    pub kind: &'static str,
    pub danger: Option<String>,
    pub by: String,
    pub agent: Option<AgentRef>,
    pub about: String,
    pub change: Option<Value>,
    pub questions: Option<Questions>,
    pub answers: Vec<String>,
    pub chat: Option<ChatPlace>,
    pub call: Option<(RuntimeInstance, String)>,
}

/// A held call a settled wait ruled on, to be told at once.
pub(crate) type Woken = Vec<(RuntimeInstance, String)>;

/// Raises a wait, or joins the open one that waits for the same thing
/// by the same agent. A call already held finds its wait again; one
/// already ruled raises nothing.
pub(crate) fn raise(
    db: &Connection,
    raising: Raising,
    at: i64,
) -> Result<Option<String>> {
    if let Some((runtime, key)) = &raising.call {
        let held = db
            .query_row(
                "SELECT wait, how FROM gate_call WHERE runtime = ?1 \
                 AND key = ?2",
                params![runtime.as_str(), key],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()?;
        match held {
            Some((_, Some(_))) => return Ok(None),
            Some((Some(wait), None)) => return Ok(Some(wait)),
            _ => {}
        }
    }
    put(db, raising, at).map(Some)
}

/// Raises a fault: a turn that failed, which a chief can only
/// dismiss. The same fault again is asked again.
pub(crate) fn raise_fault(
    db: &Connection,
    by: String,
    agent: AgentRef,
    chat: Option<ChatPlace>,
    about: String,
    at: i64,
) -> Result<String> {
    put(
        db,
        Raising {
            kind: "fault",
            danger: None,
            by,
            agent: Some(agent),
            about,
            change: None,
            questions: None,
            answers: vec!["dismiss".into()],
            chat,
            call: None,
        },
        at,
    )
}

fn put(db: &Connection, raising: Raising, at: i64) -> Result<String> {
    let questions = raising
        .questions
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let same = db
        .query_row(
            "SELECT id FROM wait WHERE open = 1 AND kind = ?1 AND \
             danger IS ?2 AND by = ?3 AND about = ?4 AND questions \
             IS ?5",
            params![
                raising.kind,
                raising.danger,
                raising.by,
                raising.about,
                questions,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let id = match same {
        Some(id) => {
            db.execute(
                "UPDATE wait SET asked = asked + 1 WHERE id = ?1",
                [&id],
            )?;
            id
        }
        None => {
            let ordinal = db.query_row(
                "SELECT coalesce(max(ordinal), 0) + 1 FROM wait",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            let id = format!("{at}-{ordinal}");
            let chat = raising.chat.as_ref();
            db.execute(
                "INSERT INTO wait (id, ordinal, kind, danger, by, \
                 hamlet, number, about, change, answers, since, \
                 asked, chat_hamlet, chat_number, chat_chief, \
                 chat_session, questions) VALUES (?1, ?2, ?3, ?4, \
                 ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?13, ?14, \
                 ?15, ?16)",
                params![
                    id,
                    ordinal,
                    raising.kind,
                    raising.danger,
                    raising.by,
                    raising.agent.as_ref().map(|a| a.hamlet.clone()),
                    raising
                        .agent
                        .as_ref()
                        .map(|a| i64::try_from(a.number))
                        .transpose()?,
                    raising.about,
                    raising.change.as_ref().map(Value::to_string),
                    serde_json::to_string(&raising.answers)?,
                    at,
                    chat.map(|c| c.hamlet.clone()),
                    chat.map(|c| i64::try_from(c.number))
                        .transpose()?,
                    chat.map(|c| c.chief.clone()),
                    chat.map(|c| c.session),
                    questions,
                ],
            )?;
            id
        }
    };
    if let Some((runtime, key)) = &raising.call {
        db.execute(
            "UPDATE gate_call SET wait = ?3 WHERE runtime = ?1 AND \
             key = ?2",
            params![runtime.as_str(), key, id],
        )?;
    }
    Ok(id)
}

/// Ends the open wait `id` and rules on each call it held that has
/// not ended. Gives the wait and the calls to tell, or `None` when
/// nothing waits as `id`.
pub(crate) fn settle(
    db: &Connection,
    id: &str,
    how: &str,
    text: &str,
    chief: Option<&str>,
    at: i64,
) -> Result<Option<(Wait, Woken)>> {
    let Some(wait) = find(db, id)? else {
        return Ok(None);
    };
    let woken = held(db, id)?;
    db.execute(
        "UPDATE gate_call SET how = ?2, text = ?3 WHERE wait = ?1 \
         AND how IS NULL",
        params![id, how, text],
    )?;
    db.execute(
        "UPDATE wait SET open = 0, how = ?2, text = ?3, chief = ?4, \
         settled = ?5 WHERE id = ?1",
        params![id, how, text, chief, at],
    )?;
    Ok(Some((wait, woken)))
}

/// The calls the open wait `id` holds that are not ruled yet.
fn held(db: &Connection, id: &str) -> Result<Woken> {
    let mut query = db.prepare(
        "SELECT runtime, key FROM gate_call WHERE wait = ?1 AND how \
         IS NULL",
    )?;
    query
        .query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .map(|row| {
            let (runtime, key) = row?;
            Ok((RuntimeInstance::new(runtime)?, key))
        })
        .collect()
}

/// Ends the waits raised in the chat of `chief` with `agent`: as
/// `stopped`, all but faults; as `rewound`, those raised since
/// `since` in the session `session`.
pub(crate) fn lapse(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
    how: &str,
    since: i64,
    session: Option<i64>,
    at: i64,
) -> Result<Woken> {
    let ids = open(db)?
        .into_iter()
        .filter(|wait| {
            wait.chat.as_ref().is_some_and(|chat| {
                chat.hamlet == agent.hamlet
                    && chat.number == agent.number
                    && chat.chief == chief
                    && session.is_none_or(|s| s == chat.session)
            }) && wait.since >= since
                && !(how == "stopped" && wait.kind == "fault")
        })
        .map(|wait| wait.id)
        .collect::<Vec<_>>();
    let mut woken = Vec::new();
    for id in ids {
        if let Some((_, calls)) =
            settle(db, &id, how, "", Some(chief), at)?
        {
            woken.extend(calls);
        }
    }
    Ok(woken)
}

/// The open waits in the order they were raised.
pub(crate) fn open(db: &Connection) -> Result<Vec<Wait>> {
    let mut query = db.prepare(&format!(
        "{WAIT} WHERE open = 1 ORDER BY ordinal"
    ))?;
    query.query_map([], wait_row)?.map(|row| row?).collect()
}

/// The open wait `id`.
pub(crate) fn find(
    db: &Connection,
    id: &str,
) -> Result<Option<Wait>> {
    db.query_row(
        &format!("{WAIT} WHERE open = 1 AND id = ?1"),
        [id],
        wait_row,
    )
    .optional()?
    .transpose()
}

/// How the wait `id` ended, `lapsed` when that was not kept.
pub(crate) fn ended(db: &Connection, id: &str) -> Result<String> {
    Ok(db
        .query_row(
            "SELECT how FROM wait WHERE id = ?1 AND open = 0",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_else(|| "lapsed".into()))
}

const WAIT: &str = "SELECT id, kind, danger, by, hamlet, number, \
                    about, change, answers, since, asked, \
                    chat_hamlet, chat_number, chat_chief, \
                    chat_session, questions FROM wait";

fn wait_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<Wait>> {
    let id = row.get::<_, String>(0)?;
    let kind = row.get::<_, String>(1)?;
    let danger = row.get::<_, Option<String>>(2)?;
    let by = row.get::<_, String>(3)?;
    let hamlet = row.get::<_, Option<String>>(4)?;
    let number = row.get::<_, Option<i64>>(5)?;
    let about = row.get::<_, String>(6)?;
    let change = row.get::<_, Option<String>>(7)?;
    let answers = row.get::<_, String>(8)?;
    let since = row.get::<_, i64>(9)?;
    let asked = row.get::<_, i64>(10)?;
    let chat_hamlet = row.get::<_, Option<String>>(11)?;
    let chat_number = row.get::<_, Option<i64>>(12)?;
    let chat_chief = row.get::<_, Option<String>>(13)?;
    let chat_session = row.get::<_, Option<i64>>(14)?;
    let questions = row.get::<_, Option<String>>(15)?;
    Ok((|| {
        Ok(Wait {
            id,
            kind,
            danger,
            by,
            agent: match (hamlet, number) {
                (Some(hamlet), Some(number)) => Some(AgentRef {
                    hamlet,
                    number: u64::try_from(number)?,
                }),
                _ => None,
            },
            about,
            change: change
                .map(|c| serde_json::from_str(&c))
                .transpose()?,
            questions: questions
                .map(|q| serde_json::from_str(&q))
                .transpose()?,
            answers: serde_json::from_str(&answers)?,
            since,
            asked,
            chat: match (
                chat_hamlet,
                chat_number,
                chat_chief,
                chat_session,
            ) {
                (
                    Some(hamlet),
                    Some(number),
                    Some(chief),
                    Some(session),
                ) => Some(ChatPlace {
                    hamlet,
                    number: u64::try_from(number)?,
                    chief,
                    session,
                }),
                _ => None,
            },
        })
    })())
}

/// The waits raised by `agent`, across its chats.
pub(crate) fn for_agent<'a>(
    waits: &'a [Wait],
    agent: &AgentRef,
) -> impl Iterator<Item = &'a Wait> {
    waits
        .iter()
        .filter(move |wait| wait.agent.as_ref() == Some(agent))
}

/// The waits of the chat of `chief` with `agent`, across its
/// sessions.
pub(crate) fn for_chat<'a>(
    waits: &'a [Wait],
    agent: &'a AgentRef,
    chief: &'a str,
) -> impl Iterator<Item = &'a Wait> {
    waits.iter().filter(move |wait| {
        wait.chat.as_ref().is_some_and(|chat| {
            chat.hamlet == agent.hamlet
                && chat.number == agent.number
                && chat.chief == chief
        })
    })
}

/// A danger's words after the agent's name.
pub(crate) fn says(danger: Option<&str>) -> &'static str {
    match danger {
        Some("network") => "wants the network for",
        Some("unconfined") => "wants to run, with no sandbox,",
        Some("destructive") => {
            "wants to run, with no sandbox, the destructive"
        }
        Some("reserved") => "wants to do what you reserved:",
        Some("edit") => "wants to edit",
        Some("command") => "wants to run",
        _ => "",
    }
}

/// One line for a wait, quoting at most `most` characters of its
/// subject.
pub(crate) fn line(wait: &Wait, most: usize) -> String {
    let first = |text: &str| {
        text.split('\n')
            .next()
            .map(|line| line.trim_end_matches('\r').to_owned())
            .unwrap_or_default()
    };
    match wait.kind.as_str() {
        "approval" => {
            let subject = if wait.danger.as_deref() == Some("edit") {
                first(&wait.about)
            } else {
                wait.about.clone()
            };
            format!(
                "{} {} `{}`",
                wait.by,
                says(wait.danger.as_deref()),
                clip(&subject, most, true, true)
            )
        }
        "objection" => clip(
            wait.about
                .split('\n')
                .nth(1)
                .map(|l| l.trim_end_matches('\r'))
                .unwrap_or(&wait.about),
            most + wait.by.len(),
            true,
            true,
        ),
        "question" => format!(
            "{} asks: {}",
            wait.by,
            clip(&wait.about, most, true, true)
        ),
        _ => format!(
            "{} failed: {}",
            wait.by,
            clip(&wait.about, most, true, true)
        ),
    }
}

/// A wait in full, with its subject and the answers offered.
pub(crate) fn strip(wait: &Wait) -> String {
    format!(
        "[{}] {}\n    {}\n    answer with: {}",
        wait.kind,
        line(wait, 60),
        wait.about.replace('\n', "\n    "),
        wait.answers
            .iter()
            .map(|choice| format!("/{choice}"))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// A chief's view of `wait`; a wait raised in the current session of
/// its chat names no past session.
pub(crate) fn view(
    wait: &Wait,
    record: Option<&Hamlet>,
    current: Option<i64>,
) -> Result<Value> {
    let chat = match (&wait.chat, record) {
        (Some(chat), Some(record)) => {
            let mut chat_ref = json!({"with": {
                "hamlet": chat.hamlet,
                "agent": record.address_of(chat.number),
            }});
            if current != Some(chat.session) {
                chat_ref["past"] = chat.session.into();
            }
            chat_ref
        }
        _ => Value::Null,
    };
    let danger = if wait.kind == "approval" {
        Value::from(
            wait.danger
                .clone()
                .unwrap_or_else(|| "unconfined".into()),
        )
    } else {
        Value::Null
    };
    let mut view = json!({
        "id": wait.id,
        "kind": wait.kind,
        "danger": danger,
        "by": wait.by,
        "agent": wait.agent.as_ref().map(|a| json!({"hamlet": a.hamlet, "number": a.number})),
        "says": says(wait.danger.as_deref()),
        "about": wait.about,
        "change": wait.change,
        "answers": wait.answers,
        "since": wait.since,
        "asked": wait.asked,
        "chat": chat,
    });
    if let Some(questions) = &wait.questions {
        view["questions"] = serde_json::to_value(questions)
            .context("a wait's questions")?;
    }
    Ok(view)
}
