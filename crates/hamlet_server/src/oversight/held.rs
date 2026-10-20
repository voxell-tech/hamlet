//! The gate's kept decision of each tool call and the waits that hold
//! them, as the realm database keeps them in `gate_call` and `wait`.

use anyhow::Result;
use rusqlite::types::{
    FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef,
};
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde_json::Value;

use super::{
    ChatPlace, Danger, Raising, Ruling, Wait, WaitKind, Woken,
};
use crate::harness::BindingId;
use crate::record::AgentRef;
use crate::{OperationId, RuntimeInstance};

/// What the gate keeps of one tool call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeldCall {
    pub binding: BindingId,
    pub input: Option<String>,
    /// The change an edit makes, shown with its result.
    pub change: Option<Value>,
    pub state: CallState,
}

/// Where a call stands at the gate.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CallState {
    /// Reserved; nothing decided stands.
    Open,
    /// The gate let it run, by grants and trust that may change
    /// before it runs.
    Allowed {
        net: bool,
    },
    Blocked(String),
    /// The open wait `wait` holds it.
    Held {
        wait: String,
    },
    /// The chief ruled on it through `wait`.
    Ruled {
        wait: String,
        ruling: Ruling,
        text: String,
        net: bool,
        /// Its wait asked structured questions.
        structured: bool,
    },
}

/// Reserves the call `id` for `binding` and the request `digest`;
/// gives whether the kept reservation is that one.
pub(crate) fn reserve(
    db: &Connection,
    id: &OperationId,
    binding: BindingId,
    digest: &[u8],
) -> Result<bool> {
    let (runtime, key) = (id.runtime.as_str(), id.key.as_str());
    // A call's marker is written only after its reservation.
    db.execute(
        "INSERT INTO gate_call (runtime, key, binding, digest) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
        params![runtime, key, binding.0, digest],
    )?;
    let (kept, kept_digest) = db.query_row(
        "SELECT binding, digest FROM gate_call WHERE runtime = ?1 \
         AND key = ?2",
        params![runtime, key],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
    )?;
    Ok(kept == binding.0 && kept_digest == digest)
}

/// Keeps that the gate let the call `id` run, with the network when
/// `net`.
pub(crate) fn allow(
    db: &Connection,
    id: &OperationId,
    binding: BindingId,
    input: Option<&str>,
    net: bool,
    change: Option<&Value>,
) -> Result<()> {
    decide(db, id, binding, input, Some(net), None, change)
}

/// Keeps that the call `id` may not run, in `words`.
pub(crate) fn block(
    db: &Connection,
    id: &OperationId,
    binding: BindingId,
    input: Option<&str>,
    words: &str,
    change: Option<&Value>,
) -> Result<()> {
    decide(db, id, binding, input, None, Some(words), change)
}

fn decide(
    db: &Connection,
    id: &OperationId,
    binding: BindingId,
    input: Option<&str>,
    net: Option<bool>,
    block: Option<&str>,
    change: Option<&Value>,
) -> Result<()> {
    db.execute(
        "UPDATE gate_call SET input = ?4, net = ?5, block = ?6, \
         change = ?7 WHERE runtime = ?1 AND key = ?2 AND binding = \
         ?3",
        params![
            id.runtime.as_str(),
            id.key,
            binding.0,
            input,
            net,
            block,
            change.map(Value::to_string)
        ],
    )?;
    Ok(())
}

/// What the gate keeps of the call `id`.
pub(crate) fn decision(
    db: &Connection,
    id: &OperationId,
) -> Result<Option<HeldCall>> {
    db.query_row(
        "SELECT binding, input, change, net, block, wait, how, \
         text, (SELECT questions IS NOT NULL FROM wait WHERE id = \
         gate_call.wait) FROM gate_call WHERE runtime = ?1 AND key \
         = ?2",
        params![id.runtime.as_str(), id.key],
        |row| {
            let net = row.get::<_, Option<bool>>(3)?;
            let block = row.get::<_, Option<String>>(4)?;
            let wait = row.get::<_, Option<String>>(5)?;
            let ruling = row.get::<_, Option<Ruling>>(6)?;
            let state = match (block, wait, ruling) {
                (Some(words), ..) => CallState::Blocked(words),
                (None, Some(wait), Some(ruling)) => {
                    CallState::Ruled {
                        wait,
                        ruling,
                        text: row
                            .get::<_, Option<String>>(7)?
                            .unwrap_or_default(),
                        net: net.unwrap_or(false),
                        structured: row
                            .get::<_, Option<bool>>(8)?
                            .unwrap_or(false),
                    }
                }
                (None, Some(wait), None) => CallState::Held { wait },
                (None, None, _) => match net {
                    Some(net) => CallState::Allowed { net },
                    None => CallState::Open,
                },
            };
            Ok((
                BindingId(row.get(0)?),
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                state,
            ))
        },
    )
    .optional()?
    .map(|(binding, input, change, state)| {
        Ok(HeldCall {
            binding,
            input,
            change: change
                .map(|change| serde_json::from_str(&change))
                .transpose()?,
            state,
        })
    })
    .transpose()
}

/// Sets whether the call `id`, allowed by a chief, has the network.
pub(crate) fn set_net(
    db: &Connection,
    id: &OperationId,
    net: bool,
) -> Result<()> {
    db.execute(
        "UPDATE gate_call SET net = ?3 WHERE runtime = ?1 AND key = \
         ?2",
        params![id.runtime.as_str(), id.key, net],
    )?;
    Ok(())
}

/// Refuses, in `words`, every call of `binding` not yet carried out
/// or refused. A marked result stands.
pub(crate) fn block_unrun(
    db: &Connection,
    binding: BindingId,
    words: &str,
) -> Result<()> {
    let open = db
        .prepare(
            "SELECT runtime, key FROM gate_call WHERE binding = ?1 \
             AND block IS NULL",
        )?
        .query_map([binding.0], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (runtime, key) in open {
        if !crate::operation::is_marked(db, &runtime, &key)? {
            db.execute(
                "UPDATE gate_call SET block = ?3 WHERE runtime = ?1 \
                 AND key = ?2",
                params![runtime, key, words],
            )?;
        }
    }
    Ok(())
}

/// A call the wait `wait` holds and nobody ruled on yet.
pub(crate) struct Unruled {
    pub id: OperationId,
    pub binding: BindingId,
    pub input: Option<String>,
}

/// The calls the wait `wait` holds and nobody ruled on yet.
pub(crate) fn unruled(
    db: &Connection,
    wait: &str,
) -> Result<Vec<Unruled>> {
    db.prepare(
        "SELECT runtime, key, binding, input FROM gate_call WHERE \
         wait = ?1 AND how IS NULL",
    )?
    .query_map([wait], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            BindingId(row.get(2)?),
            row.get::<_, Option<String>>(3)?,
        ))
    })?
    .map(|row| {
        let (runtime, key, binding, input) = row?;
        Ok(Unruled {
            id: OperationId {
                runtime: RuntimeInstance::new(runtime)?,
                key,
            },
            binding,
            input,
        })
    })
    .collect()
}

/// Rules on each call the wait `wait` holds that nobody ruled on
/// yet; gives them.
pub(crate) fn rule(
    db: &Connection,
    wait: &str,
    ruling: Ruling,
    text: &str,
) -> Result<Woken> {
    let woken = unruled(db, wait)?
        .into_iter()
        .map(|call| (call.id.runtime, call.id.key))
        .collect();
    db.execute(
        "UPDATE gate_call SET how = ?2, text = ?3 WHERE wait = ?1 \
         AND how IS NULL",
        params![wait, ruling, text],
    )?;
    Ok(woken)
}

/// The open waits that hold calls of `binding` not ruled on.
pub(crate) fn waits_holding(
    db: &Connection,
    binding: BindingId,
) -> Result<Vec<String>> {
    Ok(db
        .prepare(
            "SELECT DISTINCT wait FROM gate_call WHERE binding = ?1 \
             AND wait IS NOT NULL AND how IS NULL",
        )?
        .query_map([binding.0], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The open wait that holds the call `id`.
#[cfg(feature = "harness")]
pub(crate) fn open_wait_of(
    db: &Connection,
    id: &OperationId,
) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT g.wait FROM gate_call g JOIN wait w ON w.id = \
             g.wait WHERE g.runtime = ?1 AND g.key = ?2 AND w.open \
             = 1",
            params![id.runtime.as_str(), id.key],
            |row| row.get(0),
        )
        .optional()?)
}

/// Whether the call `key` of `runtime` belongs to a turn that still
/// runs, and was not carried out.
pub(crate) fn unfinished(
    db: &Connection,
    runtime: &str,
    key: &str,
) -> Result<bool> {
    let binding = db
        .query_row(
            "SELECT binding, input FROM gate_call WHERE runtime = \
             ?1 AND key = ?2",
            params![runtime, key],
            |row| {
                Ok((
                    BindingId(row.get(0)?),
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;
    let Some((binding, Some(input))) = binding else {
        return Ok(false);
    };
    Ok(crate::harness::input_runs(db, binding, &input)?
        && !crate::operation::is_marked(db, runtime, key)?)
}

/// Raises a wait, or joins the open one that waits for the same thing
/// by the same agent, and holds its call. A call already held finds
/// its wait again; one already ruled raises nothing.
pub(crate) fn raise(
    db: &Connection,
    raising: Raising,
    at: i64,
) -> Result<Option<String>> {
    if let Some(call) = &raising.call {
        match decision(db, &call.id)?.map(|kept| kept.state) {
            Some(CallState::Ruled { .. }) => return Ok(None),
            Some(CallState::Held { wait }) => return Ok(Some(wait)),
            _ => {}
        }
    }
    put(db, raising, at).map(Some)
}

/// Keeps a wait, or asks the open one like it again.
pub(crate) fn put(
    db: &Connection,
    raising: Raising,
    at: i64,
) -> Result<String> {
    let questions = raising
        .questions
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let (kind, danger) = (raising.kind.name(), raising.kind.danger());
    let same = db
        .query_row(
            "SELECT id FROM wait WHERE open = 1 AND kind = ?1 AND \
             danger IS ?2 AND by = ?3 AND about = ?4 AND questions \
             IS ?5",
            params![
                kind,
                danger,
                raising.by,
                raising.about,
                questions
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
                    kind,
                    danger,
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
    if let Some(call) = &raising.call {
        db.execute(
            "UPDATE gate_call SET input = ?4, net = NULL, block = \
             NULL, change = ?5, wait = ?6 WHERE runtime = ?1 AND \
             key = ?2 AND binding = ?3",
            params![
                call.id.runtime.as_str(),
                call.id.key,
                call.binding.0,
                call.input,
                call.change.as_ref().map(Value::to_string),
                id
            ],
        )?;
    }
    Ok(id)
}

/// Ends the open wait `id` with `ruling`.
pub(crate) fn close(
    db: &Connection,
    id: &str,
    ruling: Ruling,
    text: &str,
    chief: Option<&str>,
    at: i64,
) -> Result<()> {
    db.execute(
        "UPDATE wait SET open = 0, how = ?2, text = ?3, chief = ?4, \
         settled = ?5 WHERE id = ?1",
        params![id, ruling, text, chief, at],
    )?;
    Ok(())
}

/// Lets the open wait `id` be only dismissed, with `note` added to
/// its text.
pub(crate) fn only_dismiss(
    db: &Connection,
    id: &str,
    note: &str,
) -> Result<()> {
    db.execute(
        "UPDATE wait SET answers = '[\"dismiss\"]', about = about \
         || char(10) || ?2 WHERE id = ?1 AND open = 1 AND answers \
         != '[\"dismiss\"]'",
        params![id, note],
    )?;
    Ok(())
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

/// How the wait `id` ended, if it did and that was kept.
pub(crate) fn ended(
    db: &Connection,
    id: &str,
) -> Result<Option<Ruling>> {
    Ok(db
        .query_row(
            "SELECT how FROM wait WHERE id = ?1 AND open = 0",
            [id],
            |row| row.get::<_, Option<Ruling>>(0),
        )
        .optional()?
        .flatten())
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
    let danger = row.get::<_, Option<Danger>>(2)?;
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
            kind: WaitKind::of(&kind, danger)?,
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

impl ToSql for Ruling {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Ruling {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        Ruling::named(value.as_str()?)
            .ok_or(FromSqlError::InvalidType)
    }
}

impl ToSql for Danger {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Danger {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        Danger::named(value.as_str()?)
            .ok_or(FromSqlError::InvalidType)
    }
}
