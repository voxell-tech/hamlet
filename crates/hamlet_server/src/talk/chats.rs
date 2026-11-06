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
/// `agent`, begun when the chat is new. A worker's one conversation
/// is every chief's chat with it.
pub(crate) fn current(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
) -> Result<ConversationId> {
    if agents::get(cx.db, agent)?.kind == agents::AgentKind::Worker
        && let Some(worked) = conversations::of_agent(cx.db, agent)?
            .into_iter()
            .find(|kept| kept.kind == Kind::Worker)
    {
        return Ok(worked.id);
    }
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
    crate::society::hires::take_purpose(cx, agent, text)?;
    Ok(Said {
        conversation,
        task,
        request,
    })
}

/// The session `number` of `chief`'s chat with `agent`, if there is
/// one.
pub(crate) fn session(
    db: &mut hamlet_store::Db,
    agent: AgentId,
    chief: ChiefId,
    number: i64,
) -> Result<Option<ConversationId>> {
    Ok(chat_session::table
        .find((agent, chief, number))
        .select(chat_session::conversation)
        .first(db)
        .optional()?)
}

/// The current session's number of `chief`'s chat with `agent`.
pub(crate) fn current_number(
    db: &mut hamlet_store::Db,
    agent: AgentId,
    chief: ChiefId,
) -> Result<Option<i64>> {
    Ok(chat::table
        .find((agent, chief))
        .select(chat::current)
        .first(db)
        .optional()?)
}

/// Every session of `chief`'s chat with `agent`: its number, its
/// conversation and when it began.
pub(crate) fn sessions(
    db: &mut hamlet_store::Db,
    agent: AgentId,
    chief: ChiefId,
) -> Result<Vec<(i64, ConversationId, i64)>> {
    Ok(chat_session::table
        .filter(chat_session::agent.eq(agent))
        .filter(chat_session::chief.eq(chief))
        .order(chat_session::session)
        .select((
            chat_session::session,
            chat_session::conversation,
            chat_session::began,
        ))
        .load(db)?)
}

/// Writes `text` into `conversation` for people alone (C30).
pub(crate) fn notice(
    cx: &mut Change<'_>,
    conversation: ConversationId,
    text: &str,
) -> Result<()> {
    inputs::keep(
        cx,
        Keep {
            conversation,
            request: format!("notice:{}", uuid::Uuid::new_v4()),
            kind: InputKind::Notice,
            task: None,
            content: String::new(),
            mark: Mark::Notice {
                text: text.to_owned(),
                at: cx.at,
            },
            steer: false,
            boundary: false,
        },
    )?;
    Ok(())
}

/// Moves `chief`'s chat with `agent` to a new session at once, while
/// the old one finishes its queue (C24). Returns its number.
pub(crate) fn new_session(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
) -> Result<i64> {
    let leaving = current(cx, env, agent, chief)?;
    super::reflections::owed(cx, leaving)?;
    let number = sessions(cx.db, agent, chief)?
        .last()
        .map_or(1, |(number, _, _)| number + 1);
    let conversation = begin(cx, env, agent, chief, number)?;
    diesel::insert_into(chat_session::table)
        .values((
            chat_session::agent.eq(agent),
            chat_session::chief.eq(chief),
            chat_session::session.eq(number),
            chat_session::conversation.eq(conversation),
            chat_session::began.eq(cx.at),
        ))
        .execute(cx.db)?;
    diesel::update(chat::table.find((agent, chief)))
        .set(chat::current.eq(number))
        .execute(cx.db)?;
    notice(cx, conversation, "Started a new session.")?;
    cx.touched.agents.insert(agent);
    Ok(number)
}

/// Goes back to session `number` of `chief`'s chat with `agent`
/// (C24).
pub(crate) fn resume(
    cx: &mut Change<'_>,
    agent: AgentId,
    chief: ChiefId,
    number: i64,
) -> crate::change::Done<()> {
    let Some(conversation) = session(cx.db, agent, chief, number)?
    else {
        crate::change::refuse!(
            crate::society::refusal::Refusal::NoSession { number }
        );
    };
    if let Some(leaving) = current_number(cx.db, agent, chief)?
        .map(|current| session(cx.db, agent, chief, current))
        .transpose()?
        .flatten()
        && leaving != conversation
    {
        super::reflections::owed(cx, leaving)?;
    }
    diesel::update(chat::table.find((agent, chief)))
        .set(chat::current.eq(number))
        .execute(cx.db)?;
    // A session gone back to asks again what it was trusted with
    // (H11).
    crate::oversight::gate::reset_trust(
        cx.db,
        crate::oversight::gate::Session {
            agent,
            chief,
            session: number,
        },
    )?;
    notice(cx, conversation, "Went back to an earlier session.")?;
    cx.touched.agents.insert(agent);
    Ok(Ok(()))
}

/// Takes `chief`'s chat with `agent` back to before the chief's line
/// shown as `item` (C27): the session points at a fork of its
/// conversation made at the entry before that line, and what waited
/// since lapses. Returns the line, for the chief to send again.
pub(crate) fn rewind(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
    item: &str,
) -> crate::change::Done<String> {
    use crate::society::refusal::Refusal;
    let not_a_line = || Refusal::Usage {
        words: "That is not one of your lines in this conversation."
            .into(),
    };
    let conversation = current(cx, env, agent, chief)?;
    if inputs::is_busy(cx.db, conversation)? {
        crate::change::refuse!(Refusal::TurnRunning {
            then: "rewind".into()
        });
    }
    let entry = item.strip_suffix(":0").unwrap_or(item);
    let items =
        super::items::page(cx.db, conversation, None, i64::MAX)?;
    let Some(target) =
        items.iter().position(|(_, kept)| kept.entry == entry)
    else {
        crate::change::refuse!(not_a_line());
    };
    let name = chiefs::find(cx.db, chief)?
        .context("the chief is not in the realm")?
        .name;
    let mark = items[target]
        .1
        .mark
        .as_ref()
        .map(Mark::of)
        .transpose()?
        .flatten();
    let Some(Mark::Chief {
        chief: by, text, ..
    }) = mark
    else {
        crate::change::refuse!(not_a_line());
    };
    if by != name {
        crate::change::refuse!(not_a_line());
    }
    let kept = conversations::get(cx.db, conversation)?;
    let session = kept.session.unwrap_or(1);
    crate::oversight::lapse_in(
        cx,
        conversation,
        crate::oversight::Ruling::Rewound,
    )?;
    let fork = match target.checked_sub(1).map(|at| &items[at].1) {
        Some(previous) => {
            conversations::fork(cx, &kept, &previous.entry)?
        }
        None => begin(cx, env, agent, chief, session)?,
    };
    diesel::update(chat_session::table.find((agent, chief, session)))
        .set(chat_session::conversation.eq(fork))
        .execute(cx.db)?;
    cx.touched.agents.insert(agent);
    Ok(Ok(text))
}

/// Carries on the current session's failed or stopped turn as a new
/// task (C26). Returns the input's request.
pub(crate) fn retry(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
) -> crate::change::Done<Said> {
    use crate::society::refusal::Refusal;
    let conversation = current(cx, env, agent, chief)?;
    if inputs::is_busy(cx.db, conversation)? {
        crate::change::refuse!(Refusal::TurnRunning {
            then: "retry".into()
        });
    }
    let answered = super::items::last(cx.db, conversation)?
        .is_some_and(|item| {
            item.kind == hamlet_protocol::runtime::ItemKind::Assistant
                && matches!(
                    item.stop.as_deref(),
                    Some("stop" | "length")
                )
        });
    if answered {
        crate::change::refuse!(Refusal::Answered);
    }
    let task =
        tasks::mint(cx, Origin::Retry, agent, Some(chief), None)?;
    let request = format!("retry:{}", uuid::Uuid::new_v4());
    inputs::keep(
        cx,
        Keep {
            conversation,
            request: request.clone(),
            kind: InputKind::Retry,
            task: Some(task),
            content: "Carry on from where you stopped.".into(),
            mark: Mark::Retry { at: cx.at },
            steer: false,
            boundary: false,
        },
    )?;
    Ok(Ok(Said {
        conversation,
        task,
        request,
    }))
}

/// Stops every running turn of `agent` and drops what it had queued
/// (C25). Returns how many conversations it stopped.
pub(crate) fn stop(
    cx: &mut Change<'_>,
    agent: AgentId,
) -> Result<usize> {
    let mut stopped = 0;
    for kept in conversations::of_agent(cx.db, agent)? {
        if !inputs::is_busy(cx.db, kept.id)? {
            continue;
        }
        inputs::drop_kept(cx.db, kept.id)?;
        if kept.handle.is_some() {
            conversations::set_cancelling(cx.db, kept.id, true)?;
            conversations::set_stopped(cx.db, kept.id, true)?;
        }
        stopped += 1;
    }
    crate::oversight::lapse(
        cx,
        agent,
        crate::oversight::Ruling::Stopped,
    )?;
    cx.touched.agents.insert(agent);
    Ok(stopped)
}
