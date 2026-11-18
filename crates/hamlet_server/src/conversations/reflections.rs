//! Reflection, an agent keeping what its conversation taught it (C5).

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ConversationId};
use hamlet_store::schema::{chat, conversation, import, reflection};

use super::chats;
use super::inputs::{self, InputKind, Keep};
use super::marks::Mark;
use super::tasks::{self, Origin};
use crate::conversations::{self, Kind};
use crate::realm::change::Change;

/// The automatic instruction to keep a conversation's lessons (C5).
pub(crate) const REFLECT: &str =
    "Reflection time (automatic, not from a chief). Look back over \
     your conversation since your last reflection. Keep what is \
     worth remembering about the chiefs and the lessons learned in \
     your memory/ folder, and save or improve a skill in skills/ \
     for anything you did that is likely to come up again. If \
     nothing is worth keeping, say so in one line.";

/// The instruction to reflect before an agent's last dream.
const LAST_REFLECT: &str =
    "Last reflection (automatic, not from a chief). You were ended, \
     and your last dream brings home to your profile only what your \
     worktree holds. Look back over your whole conversation, \
     including any part brought in from another program. Keep what \
     is worth remembering about the chiefs and the lessons learned \
     in your worktree's memory/ folder, and save or improve a skill \
     in its skills/ for anything likely to come up again. If \
     nothing is worth keeping, say so in one line.";

/// Counts an answered chief's turn of `id`, and requests a reflection
/// once `every` turns were.
pub(crate) fn answered(
    cx: &mut Change<'_>,
    id: ConversationId,
    every: u64,
) -> Result<Option<String>> {
    diesel::update(conversation::table.find(id))
        .set(
            conversation::unreflected
                .eq(conversation::unreflected + 1),
        )
        .execute(cx.db)?;
    let kept = conversations::get(cx.db, id)?;
    let every = i64::try_from(every)?;
    if every == 0 || kept.unreflected < every {
        return Ok(None);
    }
    reflect(cx, id)
}

/// Tells the conversation `id` to reflect now.
pub(crate) fn reflect(
    cx: &mut Change<'_>,
    id: ConversationId,
) -> Result<Option<String>> {
    request(cx, id, REFLECT)
}

/// Tells the conversation of `agent`, ended with a dream, to reflect
/// before the dream if it has anything new to reflect on.
pub(crate) fn before_last_dream(
    cx: &mut Change<'_>,
    agent: AgentId,
) -> Result<()> {
    let Some(id) = chats::of(cx.db, agent)? else {
        return Ok(());
    };
    let kept = conversations::get(cx.db, id)?;
    if kept.held {
        return Ok(());
    }
    let brought_in =
        import::table.find(id).count().get_result::<i64>(cx.db)? > 0;
    let unreflected = kept.unreflected > 0
        || (brought_in && reflections_of(cx.db, agent)? == 0);
    if unreflected {
        request(cx, id, LAST_REFLECT)?;
    }
    Ok(())
}

/// Whether the conversation of `agent` has a reflection to run.
pub(crate) fn is_reflecting(
    db: &mut Db,
    agent: AgentId,
) -> Result<bool> {
    match chats::of(db, agent)? {
        Some(id) => is_pending(db, id),
        None => Ok(false),
    }
}

/// Whether the conversation `id` has a reflection to run.
fn is_pending(db: &mut Db, id: ConversationId) -> Result<bool> {
    Ok(reflection::table
        .filter(reflection::conversation.eq(id))
        .count()
        .get_result::<i64>(db)?
        > 0)
}

/// How many reflections `agent`'s conversations were told to make.
fn reflections_of(db: &mut Db, agent: AgentId) -> Result<i64> {
    Ok(chat::table
        .find(agent)
        .select(chat::reflections)
        .first::<i64>(db)
        .optional()?
        .unwrap_or(0))
}

/// Tells the conversation `id` to reflect now, told `content`.
fn request(
    cx: &mut Change<'_>,
    id: ConversationId,
    content: &str,
) -> Result<Option<String>> {
    let kept = conversations::get(cx.db, id)?;
    if kept.kind != Kind::Chat {
        return Ok(None);
    }
    let chief = crate::society::chiefs::first(cx.db)?;
    if is_pending(cx.db, id)? {
        return Ok(None);
    }
    let number = reflections_of(cx.db, kept.agent)? + 1;
    let request = format!("reflection:{id}:{number}");
    let task =
        tasks::mint(cx, Origin::Reflection, kept.agent, chief, None)?;
    let kept_input = inputs::keep(
        cx,
        Keep {
            conversation: id,
            request: request.clone(),
            kind: InputKind::Reflection,
            task: Some(task),
            content: content.into(),
            mark: Mark::Reflection { at: cx.at },
            steer: false,
            boundary: false,
        },
    )?;
    if !kept_input {
        return Ok(None);
    }
    let ordinal = inputs::find(cx.db, id, &request)?
        .map_or(0, |input| input.ordinal);
    diesel::insert_into(reflection::table)
        .values((
            reflection::conversation.eq(id),
            reflection::request.eq(&request),
            reflection::ordinal.eq(ordinal),
            reflection::count.eq(kept.unreflected),
            reflection::task.eq(task),
            reflection::at.eq(cx.at),
        ))
        .execute(cx.db)?;
    diesel::update(conversation::table.find(id))
        .set(conversation::unreflected.eq(0))
        .execute(cx.db)?;
    diesel::update(chat::table.find(kept.agent))
        .set(chat::reflections.eq(number))
        .execute(cx.db)?;
    cx.touched.agents.insert(kept.agent);
    Ok(Some(request))
}

/// Keeps that the reflection `request` of `id` ended.
pub(crate) fn ended(
    cx: &mut Change<'_>,
    id: ConversationId,
    request: &str,
) -> Result<()> {
    diesel::delete(reflection::table.find((id, request)))
        .execute(cx.db)?;
    Ok(())
}
