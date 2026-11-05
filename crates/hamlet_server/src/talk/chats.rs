//! Chief chats: each chief's conversation with an agent, in numbered
//! sessions (C22, C24).

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::ids::{AgentId, ChiefId, ConversationId, TaskId};
use hamlet_store::schema::{chat, chat_session};

use super::conversations::{self, Begin, Kind};
use super::inputs::{self, InputKind, Keep};
use super::marks::Mark;
use super::tasks::{self, Origin};
use super::{Env, models, runtimes};
use crate::change::Change;
use crate::society::{agents, chiefs};

/// A chief's line, kept.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Said {
    pub conversation: ConversationId,
    pub task: TaskId,
    pub request: String,
}

/// The conversation of the current session of `chief`'s chat with
/// `agent`, begun when the chat is new.
pub(crate) fn current(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
) -> Result<ConversationId> {
    let session = chat::table
        .find((agent, chief))
        .select(chat::current)
        .first::<i64>(cx.db)
        .optional()?;
    if let Some(session) = session {
        return Ok(chat_session::table
            .find((agent, chief, session))
            .select(chat_session::conversation)
            .first(cx.db)?);
    }
    let conversation = begin(cx, env, agent, chief, 1)?;
    diesel::insert_into(chat::table)
        .values((
            chat::agent.eq(agent),
            chat::chief.eq(chief),
            chat::current.eq(1),
        ))
        .execute(cx.db)?;
    diesel::insert_into(chat_session::table)
        .values((
            chat_session::agent.eq(agent),
            chat_session::chief.eq(chief),
            chat_session::session.eq(1),
            chat_session::conversation.eq(conversation),
            chat_session::began.eq(cx.at),
        ))
        .execute(cx.db)?;
    Ok(conversation)
}

/// Begins session `session` of `chief`'s chat with `agent`, in the
/// runtime of the agent's harness, with the model it runs now.
fn begin(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
    session: i64,
) -> Result<ConversationId> {
    let harness = agents::get(cx.db, agent)?.harness;
    let runtime = runtimes::of_harness(cx.db, &harness)?;
    let choice =
        models::choose(cx.db, &env.settings, &env.overrides, agent)?;
    let mut opening = Begin::new(agent, Kind::Chat);
    opening.chief = Some(chief);
    opening.session = Some(session);
    conversations::begin(cx, opening, &runtime, &choice.model, None)
}

/// Keeps `chief`'s line `text` for `agent` as a task of its own, an
/// input to its chat's current session.
pub(crate) fn say(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
    request: &str,
    text: &str,
) -> Result<Said> {
    let conversation = current(cx, env, agent, chief)?;
    let name = chiefs::find(cx.db, chief)?
        .context("the chief is not in the realm")?
        .name;
    let task = tasks::mint(
        cx,
        Origin::Chief,
        agent,
        Some(chief),
        Some(text),
    )?;
    let request = format!("chief:{request}");
    inputs::keep(
        cx,
        Keep {
            conversation,
            request: request.clone(),
            kind: InputKind::Chief,
            task: Some(task),
            content: text.to_owned(),
            mark: Mark::Chief {
                chief: name,
                text: text.to_owned(),
                at: cx.at,
            },
            steer: false,
            boundary: false,
        },
    )?;
    Ok(Said {
        conversation,
        task,
        request,
    })
}
