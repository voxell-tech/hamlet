//! The chats, sessions and conversations the realm database keeps for
//! each agent.

use anyhow::{Context, Result};
use diesel::prelude::*;

use crate::harness::BindingId;
use crate::record::AgentRef;
use crate::schema::{
    chat, chat_conversation, chat_instruction, chat_session,
};

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
pub(crate) struct ChatSession {
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
    db: &mut SqliteConnection,
    agent: &AgentRef,
    chief: &str,
) -> Result<Option<ChatSession>> {
    match current_number(db, agent, chief)? {
        Some(number) => session(db, agent, chief, number),
        None => Ok(None),
    }
}

/// The session `number` of `chief`'s chat with `agent`.
pub(crate) fn session(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    chief: &str,
    number: i64,
) -> Result<Option<ChatSession>> {
    Ok(chat_session::table
        .find((&agent.hamlet, number_of(agent)?, chief, number))
        .select((
            chat_session::session,
            chat_session::binding,
            chat_session::began,
        ))
        .first::<(i64, i64, i64)>(db)
        .optional()?
        .map(chat_session_of))
}

/// Every session of `chief`'s chat with `agent`, oldest first.
pub(crate) fn sessions(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    chief: &str,
) -> Result<Vec<ChatSession>> {
    Ok(chat_session::table
        .filter(chat_session::hamlet.eq(&agent.hamlet))
        .filter(chat_session::number.eq(number_of(agent)?))
        .filter(chat_session::chief.eq(chief))
        .order(chat_session::session)
        .select((
            chat_session::session,
            chat_session::binding,
            chat_session::began,
        ))
        .load::<(i64, i64, i64)>(db)?
        .into_iter()
        .map(chat_session_of)
        .collect())
}

fn chat_session_of(
    (number, binding, began): (i64, i64, i64),
) -> ChatSession {
    ChatSession {
        number,
        binding: BindingId(binding),
        began,
    }
}

/// The session number of the chat's current conversation.
pub(crate) fn current_number(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    chief: &str,
) -> Result<Option<i64>> {
    Ok(chat::table
        .find((&agent.hamlet, number_of(agent)?, chief))
        .select(chat::current)
        .first(db)
        .optional()?)
}

/// Keeps `binding` as the session `number` of the chat and points the
/// chat at it, beginning the chat when it is new.
pub(crate) fn point(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    chief: &str,
    number: i64,
    binding: BindingId,
    at: i64,
) -> Result<()> {
    let n = number_of(agent)?;
    diesel::insert_into(chat::table)
        .values((
            chat::hamlet.eq(&agent.hamlet),
            chat::number.eq(n),
            chat::chief.eq(chief),
            chat::current.eq(number),
        ))
        .on_conflict((chat::hamlet, chat::number, chat::chief))
        .do_update()
        .set(
            chat::current.eq(diesel::upsert::excluded(chat::current)),
        )
        .execute(db)?;
    diesel::insert_into(chat_session::table)
        .values((
            chat_session::hamlet.eq(&agent.hamlet),
            chat_session::number.eq(n),
            chat_session::chief.eq(chief),
            chat_session::session.eq(number),
            chat_session::binding.eq(binding.0),
            chat_session::began.eq(at),
        ))
        .on_conflict((
            chat_session::hamlet,
            chat_session::number,
            chat_session::chief,
            chat_session::session,
        ))
        .do_update()
        .set(
            chat_session::binding
                .eq(diesel::upsert::excluded(chat_session::binding)),
        )
        .execute(db)?;
    Ok(())
}

/// Points the chat at its session `number`, which it has.
pub(crate) fn select(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    chief: &str,
    number: i64,
) -> Result<()> {
    diesel::update(chat::table.find((
        &agent.hamlet,
        number_of(agent)?,
        chief,
    )))
    .set(chat::current.eq(number))
    .execute(db)?;
    Ok(())
}

/// The next reflection's number in the chat, reserved now.
pub(crate) fn reserve_reflection(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    chief: &str,
) -> Result<i64> {
    let n = number_of(agent)?;
    diesel::update(chat::table.find((&agent.hamlet, n, chief)))
        .set(chat::reflections.eq(chat::reflections + 1))
        .execute(db)?;
    Ok(chat::table
        .find((&agent.hamlet, n, chief))
        .select(chat::reflections)
        .first(db)?)
}

/// Keeps whom the conversation of `binding` is with.
pub(crate) fn keep(
    db: &mut SqliteConnection,
    binding: BindingId,
    place: &Place,
    previous: Option<BindingId>,
    unreflected: i64,
) -> Result<()> {
    diesel::insert_into(chat_conversation::table)
        .values((
            chat_conversation::binding.eq(binding.0),
            chat_conversation::hamlet.eq(&place.agent.hamlet),
            chat_conversation::number.eq(number_of(&place.agent)?),
            chat_conversation::chief.eq(&place.chief),
            chat_conversation::session.eq(place.session),
            chat_conversation::previous.eq(previous.map(|p| p.0)),
            chat_conversation::unreflected.eq(unreflected),
        ))
        .execute(db)?;
    Ok(())
}

/// What the server keeps of the conversation of `binding`.
pub(crate) fn kept(
    db: &mut SqliteConnection,
    binding: BindingId,
) -> Result<Option<Kept>> {
    chat_conversation::table
        .find(binding.0)
        .select((
            chat_conversation::hamlet,
            chat_conversation::number,
            chat_conversation::chief,
            chat_conversation::session,
            chat_conversation::previous,
            chat_conversation::unreflected,
            chat_conversation::retiring,
            chat_conversation::entered,
            chat_conversation::last,
            chat_conversation::last_input,
        ))
        .first::<(
            String,
            i64,
            Option<String>,
            i64,
            Option<i64>,
            i64,
            bool,
            Option<String>,
            Option<String>,
            Option<String>,
        )>(db)
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
    db: &mut SqliteConnection,
    agent: &AgentRef,
) -> Result<Vec<BindingId>> {
    Ok(chat_conversation::table
        .filter(chat_conversation::hamlet.eq(&agent.hamlet))
        .filter(chat_conversation::number.eq(number_of(agent)?))
        .order(chat_conversation::binding)
        .select(chat_conversation::binding)
        .load::<i64>(db)?
        .into_iter()
        .map(BindingId)
        .collect())
}

/// Every conversation kept for an agent, oldest first.
pub(crate) fn all(
    db: &mut SqliteConnection,
) -> Result<Vec<BindingId>> {
    Ok(chat_conversation::table
        .order(chat_conversation::binding)
        .select(chat_conversation::binding)
        .load::<i64>(db)?
        .into_iter()
        .map(BindingId)
        .collect())
}

/// The conversations that led to `binding` through rewinds and forks,
/// from `binding` back.
pub(crate) fn lineage(
    db: &mut SqliteConnection,
    binding: BindingId,
) -> Result<Vec<BindingId>> {
    let mut chain = vec![binding];
    let mut at = binding;
    loop {
        let previous = match chat_conversation::table
            .find(at.0)
            .select(chat_conversation::previous)
            .first::<Option<i64>>(db)
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
    db: &mut SqliteConnection,
    binding: BindingId,
    retiring: bool,
) -> Result<()> {
    diesel::update(chat_conversation::table.find(binding.0))
        .set(chat_conversation::retiring.eq(retiring))
        .execute(db)?;
    Ok(())
}

/// A chief's instruction to an agent, kept with the latest twenty.
pub(crate) fn instruct(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    task: &str,
    chief: &str,
    text: &str,
    at: i64,
) -> Result<()> {
    let n = number_of(agent)?;
    diesel::insert_into(chat_instruction::table)
        .values((
            chat_instruction::hamlet.eq(&agent.hamlet),
            chat_instruction::number.eq(n),
            chat_instruction::task.eq(task),
            chat_instruction::chief.eq(chief),
            chat_instruction::text.eq(text),
            chat_instruction::at.eq(at),
        ))
        .on_conflict_do_nothing()
        .execute(db)?;
    let latest = chat_instruction::table
        .filter(chat_instruction::hamlet.eq(&agent.hamlet))
        .filter(chat_instruction::number.eq(n))
        .order(chat_instruction::at.desc())
        .limit(20)
        .select(chat_instruction::task)
        .load::<String>(db)?;
    diesel::delete(
        chat_instruction::table
            .filter(chat_instruction::hamlet.eq(&agent.hamlet))
            .filter(chat_instruction::number.eq(n))
            .filter(chat_instruction::task.ne_all(latest)),
    )
    .execute(db)?;
    Ok(())
}

/// Keeps that the instruction `task` to `agent` was carried out.
pub(crate) fn instruction_done(
    db: &mut SqliteConnection,
    agent: &AgentRef,
    task: &str,
    at: i64,
) -> Result<()> {
    diesel::update(chat_instruction::table.find((
        &agent.hamlet,
        number_of(agent)?,
        task,
    )))
    .set(chat_instruction::done.eq(at))
    .execute(db)?;
    Ok(())
}

/// The instructions to the agents of `hamlet`, each agent's oldest
/// first.
pub(crate) fn instructions(
    db: &mut SqliteConnection,
    hamlet: &str,
) -> Result<Vec<crate::prompt::Instruction>> {
    Ok(chat_instruction::table
        .filter(chat_instruction::hamlet.eq(hamlet))
        .order((chat_instruction::number, chat_instruction::at))
        .select((
            chat_instruction::number,
            chat_instruction::chief,
            chat_instruction::text,
            chat_instruction::at,
        ))
        .load::<(i64, String, String, i64)>(db)?
        .into_iter()
        .map(|(agent, chief, text, at)| crate::prompt::Instruction {
            agent,
            chief,
            text,
            at,
        })
        .collect())
}

fn number_of(agent: &AgentRef) -> Result<i64> {
    i64::try_from(agent.number).context("an agent's number")
}

/// Keeps how the last followed input of `binding` ended.
pub(crate) fn followed(
    db: &mut SqliteConnection,
    binding: BindingId,
    last: &str,
    input: Option<&str>,
) -> Result<()> {
    diesel::update(chat_conversation::table.find(binding.0))
        .set((
            chat_conversation::last.eq(last),
            chat_conversation::last_input.eq(input),
        ))
        .execute(db)?;
    Ok(())
}

/// Adds `by` to the answered turns of `binding` since its last
/// reflection, or sets them to none.
pub(crate) fn count_turns(
    db: &mut SqliteConnection,
    binding: BindingId,
    by: Option<i64>,
) -> Result<()> {
    let conversation = chat_conversation::table.find(binding.0);
    match by {
        Some(by) => diesel::update(conversation)
            .set(
                chat_conversation::unreflected
                    .eq(chat_conversation::unreflected + by),
            )
            .execute(db)?,
        None => diesel::update(conversation)
            .set(chat_conversation::unreflected.eq(0))
            .execute(db)?,
    };
    Ok(())
}

/// Keeps that the reflection input `input` of `binding` was entered.
pub(crate) fn enter(
    db: &mut SqliteConnection,
    binding: BindingId,
    input: &str,
) -> Result<bool> {
    Ok(diesel::update(
        chat_conversation::table
            .find(binding.0)
            .filter(chat_conversation::entered.is_not(input)),
    )
    .set((
        chat_conversation::entered.eq(input),
        chat_conversation::unreflected.eq(0),
    ))
    .execute(db)?
        == 1)
}

/// The budget words kept for the turn of `input`, if any.
pub(crate) fn budget_words(
    db: &mut SqliteConnection,
    binding: BindingId,
    input: &str,
) -> Result<Option<String>> {
    Ok(chat_conversation::table
        .find(binding.0)
        .filter(chat_conversation::budget_input.eq(input))
        .select(chat_conversation::budget_words)
        .first::<Option<String>>(db)
        .optional()?
        .flatten())
}

/// Keeps the budget words shown through the turn of `input`.
pub(crate) fn keep_budget_words(
    db: &mut SqliteConnection,
    binding: BindingId,
    input: &str,
    words: &str,
) -> Result<()> {
    diesel::update(chat_conversation::table.find(binding.0))
        .set((
            chat_conversation::budget_input.eq(input),
            chat_conversation::budget_words.eq(words),
        ))
        .execute(db)?;
    Ok(())
}
