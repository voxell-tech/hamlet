//! The chats, sessions and conversations the realm database keeps for
//! each agent.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::harness::BindingId;
use crate::record::AgentRef;

/// Whom a conversation is with: an agent, the chief who talks to it,
/// and its session number in their chat (C22, C24).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Place {
    pub agent: AgentRef,
    /// `None` for a conversation with no chief in it.
    pub chief: Option<String>,
    pub session: i64,
}

/// One session of a chat, by its number.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Session {
    pub number: i64,
    pub binding: BindingId,
    pub began: i64,
}

/// What the server keeps of one conversation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Kept {
    pub place: Place,
    pub previous: Option<BindingId>,
    pub unreflected: i64,
    pub retiring: bool,
    pub entered: Option<String>,
    pub last: Option<String>,
    pub last_input: Option<String>,
}

/// The session of `chief`'s chat with `agent` in use, if the chat was
/// begun.
pub(crate) fn current(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
) -> Result<Option<Session>> {
    let number = db
        .query_row(
            "SELECT current FROM chat WHERE hamlet = ?1 AND number \
             = ?2 AND chief = ?3",
            params![agent.hamlet, number_of(agent)?, chief],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    match number {
        Some(number) => session(db, agent, chief, number),
        None => Ok(None),
    }
}

/// The session `number` of `chief`'s chat with `agent`.
pub(crate) fn session(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
    number: i64,
) -> Result<Option<Session>> {
    Ok(db
        .query_row(
            "SELECT session, binding, began FROM chat_session WHERE \
             hamlet = ?1 AND number = ?2 AND chief = ?3 AND session \
             = ?4",
            params![agent.hamlet, number_of(agent)?, chief, number],
            |row| {
                Ok(Session {
                    number: row.get(0)?,
                    binding: BindingId(row.get(1)?),
                    began: row.get(2)?,
                })
            },
        )
        .optional()?)
}

/// Every session of `chief`'s chat with `agent`, oldest first.
pub(crate) fn sessions(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
) -> Result<Vec<Session>> {
    let mut query = db.prepare(
        "SELECT session, binding, began FROM chat_session WHERE \
         hamlet = ?1 AND number = ?2 AND chief = ?3 ORDER BY session",
    )?;
    Ok(query
        .query_map(
            params![agent.hamlet, number_of(agent)?, chief],
            |row| {
                Ok(Session {
                    number: row.get(0)?,
                    binding: BindingId(row.get(1)?),
                    began: row.get(2)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The session number of the chat's current conversation.
pub(crate) fn current_number(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
) -> Result<Option<i64>> {
    Ok(db
        .query_row(
            "SELECT current FROM chat WHERE hamlet = ?1 AND number \
             = ?2 AND chief = ?3",
            params![agent.hamlet, number_of(agent)?, chief],
            |row| row.get(0),
        )
        .optional()?)
}

/// Keeps `binding` as the session `number` of the chat and points the
/// chat at it, beginning the chat when it is new.
pub(crate) fn point(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
    number: i64,
    binding: BindingId,
    at: i64,
) -> Result<()> {
    let n = number_of(agent)?;
    db.execute(
        "INSERT INTO chat (hamlet, number, chief, current) VALUES \
         (?1, ?2, ?3, ?4) ON CONFLICT (hamlet, number, chief) DO \
         UPDATE SET current = excluded.current",
        params![agent.hamlet, n, chief, number],
    )?;
    db.execute(
        "INSERT INTO chat_session (hamlet, number, chief, session, \
         binding, began) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON \
         CONFLICT (hamlet, number, chief, session) DO UPDATE SET \
         binding = excluded.binding",
        params![agent.hamlet, n, chief, number, binding.0, at],
    )?;
    Ok(())
}

/// Points the chat at its session `number`, which it has.
pub(crate) fn select(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
    number: i64,
) -> Result<()> {
    db.execute(
        "UPDATE chat SET current = ?4 WHERE hamlet = ?1 AND number \
         = ?2 AND chief = ?3",
        params![agent.hamlet, number_of(agent)?, chief, number],
    )?;
    Ok(())
}

/// The next reflection's number in the chat, reserved now.
pub(crate) fn reserve_reflection(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
) -> Result<i64> {
    let n = number_of(agent)?;
    db.execute(
        "UPDATE chat SET reflections = reflections + 1 WHERE hamlet \
         = ?1 AND number = ?2 AND chief = ?3",
        params![agent.hamlet, n, chief],
    )?;
    Ok(db.query_row(
        "SELECT reflections FROM chat WHERE hamlet = ?1 AND number \
         = ?2 AND chief = ?3",
        params![agent.hamlet, n, chief],
        |row| row.get(0),
    )?)
}

/// Keeps whom the conversation of `binding` is with.
pub(crate) fn keep(
    db: &Connection,
    binding: BindingId,
    place: &Place,
    previous: Option<BindingId>,
    unreflected: i64,
) -> Result<()> {
    db.execute(
        "INSERT INTO chat_conversation (binding, hamlet, number, \
         chief, session, previous, unreflected) VALUES (?1, ?2, ?3, \
         ?4, ?5, ?6, ?7)",
        params![
            binding.0,
            place.agent.hamlet,
            number_of(&place.agent)?,
            place.chief,
            place.session,
            previous.map(|p| p.0),
            unreflected
        ],
    )?;
    Ok(())
}

/// What the server keeps of the conversation of `binding`.
pub(crate) fn kept(
    db: &Connection,
    binding: BindingId,
) -> Result<Option<Kept>> {
    db.query_row(
        "SELECT hamlet, number, chief, session, previous, \
         unreflected, retiring, entered, last, last_input FROM \
         chat_conversation WHERE binding = ?1",
        [binding.0],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        },
    )
    .optional()?
    .map(
        |(
            hamlet,
            number,
            chief,
            session,
            previous,
            unreflected,
            retiring,
            entered,
            last,
            last_input,
        )| {
            Ok(Kept {
                place: Place {
                    agent: AgentRef {
                        hamlet,
                        number: u64::try_from(number)?,
                    },
                    chief,
                    session,
                },
                previous: previous.map(BindingId),
                unreflected,
                retiring,
                entered,
                last,
                last_input,
            })
        },
    )
    .transpose()
}

/// Every conversation of `agent`, oldest first.
pub(crate) fn conversations_of(
    db: &Connection,
    agent: &AgentRef,
) -> Result<Vec<BindingId>> {
    let mut query = db.prepare(
        "SELECT binding FROM chat_conversation WHERE hamlet = ?1 \
         AND number = ?2 ORDER BY binding",
    )?;
    Ok(query
        .query_map(params![agent.hamlet, number_of(agent)?], |row| {
            Ok(BindingId(row.get(0)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every conversation kept for an agent, oldest first.
pub(crate) fn all(db: &Connection) -> Result<Vec<BindingId>> {
    let mut query = db.prepare(
        "SELECT binding FROM chat_conversation ORDER BY binding",
    )?;
    Ok(query
        .query_map([], |row| Ok(BindingId(row.get(0)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The conversations that led to `binding` through rewinds and forks,
/// from `binding` back.
pub(crate) fn lineage(
    db: &Connection,
    binding: BindingId,
) -> Result<Vec<BindingId>> {
    let mut chain = vec![binding];
    let mut at = binding;
    loop {
        let previous = match db
            .query_row(
                "SELECT previous FROM chat_conversation WHERE \
                 binding = ?1",
                [at.0],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
        {
            Some(previous) => Some(BindingId(previous)),
            None => crate::harness::fork_of(db, at)?,
        };
        match previous {
            Some(previous) if !chain.contains(&previous) => {
                chain.push(previous);
                at = previous;
            }
            _ => return Ok(chain),
        }
    }
}

pub(crate) fn set_retiring(
    db: &Connection,
    binding: BindingId,
    retiring: bool,
) -> Result<()> {
    db.execute(
        "UPDATE chat_conversation SET retiring = ?2 WHERE binding = \
         ?1",
        params![binding.0, retiring],
    )?;
    Ok(())
}

/// A chief's instruction to an agent, kept with the latest twenty.
pub(crate) fn instruct(
    db: &Connection,
    agent: &AgentRef,
    task: &str,
    chief: &str,
    text: &str,
    at: i64,
) -> Result<()> {
    let n = number_of(agent)?;
    db.execute(
        "INSERT INTO chat_instruction (hamlet, number, task, chief, \
         text, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT DO \
         NOTHING",
        params![agent.hamlet, n, task, chief, text, at],
    )?;
    db.execute(
        "DELETE FROM chat_instruction WHERE hamlet = ?1 AND number \
         = ?2 AND task NOT IN (SELECT task FROM chat_instruction \
         WHERE hamlet = ?1 AND number = ?2 ORDER BY at DESC LIMIT \
         20)",
        params![agent.hamlet, n],
    )?;
    Ok(())
}

/// Keeps that the instruction `task` to `agent` was carried out.
pub(crate) fn instruction_done(
    db: &Connection,
    agent: &AgentRef,
    task: &str,
    at: i64,
) -> Result<()> {
    db.execute(
        "UPDATE chat_instruction SET done = ?4 WHERE hamlet = ?1 \
         AND number = ?2 AND task = ?3",
        params![agent.hamlet, number_of(agent)?, task, at],
    )?;
    Ok(())
}

/// The instructions to the agents of `hamlet`, each agent's oldest
/// first.
pub(crate) fn instructions(
    db: &Connection,
    hamlet: &str,
) -> Result<Vec<crate::prompt::Instruction>> {
    let mut query = db.prepare(
        "SELECT number, chief, text, at FROM chat_instruction WHERE \
         hamlet = ?1 ORDER BY number, at",
    )?;
    Ok(query
        .query_map([hamlet], |row| {
            Ok(crate::prompt::Instruction {
                agent: row.get(0)?,
                chief: row.get(1)?,
                text: row.get(2)?,
                at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn number_of(agent: &AgentRef) -> Result<i64> {
    i64::try_from(agent.number).context("an agent's number")
}

/// Keeps how the last followed input of `binding` ended.
pub(crate) fn followed(
    db: &Connection,
    binding: BindingId,
    last: &str,
    input: Option<&str>,
) -> Result<()> {
    db.execute(
        "UPDATE chat_conversation SET last = ?2, last_input = ?3 \
         WHERE binding = ?1",
        params![binding.0, last, input],
    )?;
    Ok(())
}

/// Adds `by` to the answered turns of `binding` since its last
/// reflection, or sets them to none.
pub(crate) fn count_turns(
    db: &Connection,
    binding: BindingId,
    by: Option<i64>,
) -> Result<()> {
    match by {
        Some(by) => db.execute(
            "UPDATE chat_conversation SET unreflected = unreflected \
             + ?2 WHERE binding = ?1",
            params![binding.0, by],
        )?,
        None => db.execute(
            "UPDATE chat_conversation SET unreflected = 0 WHERE \
             binding = ?1",
            [binding.0],
        )?,
    };
    Ok(())
}

/// Keeps that the reflection input `input` of `binding` was entered.
pub(crate) fn enter(
    db: &Connection,
    binding: BindingId,
    input: &str,
) -> Result<bool> {
    Ok(db.execute(
        "UPDATE chat_conversation SET entered = ?2, unreflected = 0 \
         WHERE binding = ?1 AND entered IS NOT ?2",
        params![binding.0, input],
    )? == 1)
}

/// The budget words kept for the turn of `input`, if any.
pub(crate) fn budget_words(
    db: &Connection,
    binding: BindingId,
    input: &str,
) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT budget_words FROM chat_conversation WHERE \
             binding = ?1 AND budget_input = ?2",
            params![binding.0, input],
            |row| row.get(0),
        )
        .optional()?
        .flatten())
}

/// Keeps the budget words shown through the turn of `input`.
pub(crate) fn keep_budget_words(
    db: &Connection,
    binding: BindingId,
    input: &str,
    words: &str,
) -> Result<()> {
    db.execute(
        "UPDATE chat_conversation SET budget_input = ?2, \
         budget_words = ?3 WHERE binding = ?1",
        params![binding.0, input, words],
    )?;
    Ok(())
}
