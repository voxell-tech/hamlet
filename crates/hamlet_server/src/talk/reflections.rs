//! Reflection (C5): an agent asked to keep what a chat taught it, in
//! its memory and its skills, after enough answered turns, when its
//! session is put away, or when a chief asks.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::ids::ConversationId;
use hamlet_store::schema::{chat, conversation, reflection};

use super::chats;
use super::conversations::{self, Kind};
use super::inputs::{self, InputKind, Keep};
use super::marks::Mark;
use super::tasks::{self, Origin};
use crate::change::Change;

/// The automatic instruction to keep a session's lessons (C5).
pub(crate) const REFLECT: &str =
    "Reflection time (automatic, not from a chief). Look back over \
     this session. Keep what is worth remembering about the chiefs \
     and the lessons learned in your memory/ folder, and save or \
     improve a skill in skills/ for anything you did that is likely \
     to come up again. If nothing is worth keeping, say so in one \
     line.";

/// Counts a chief's turn of `id` that was answered, and asks a
/// reflection once `every` turns were, or at once when its session
/// was put away. Returns the reflection's request when one was asked.
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
    // A session put away while this turn was ending owed nothing
    // then, so it reflects now.
    let put_away = match kept.chief {
        Some(chief) => {
            let current =
                chats::current_number(cx.db, kept.agent, chief)?
                    .map(|number| {
                        chats::session(
                            cx.db, kept.agent, chief, number,
                        )
                    })
                    .transpose()?
                    .flatten();
            current.is_some_and(|current| current != id)
        }
        None => false,
    };
    if !put_away && (every == 0 || kept.unreflected < every) {
        return Ok(None);
    }
    ask(cx, id)
}

/// Asks the chat `id` to reflect on the turns it answered since its
/// last reflection, unless one is pending or none were answered.
pub(crate) fn owed(
    cx: &mut Change<'_>,
    id: ConversationId,
) -> Result<Option<String>> {
    if conversations::get(cx.db, id)?.unreflected == 0 {
        return Ok(None);
    }
    ask(cx, id)
}

/// Asks the chat `id` to reflect now.
pub(crate) fn ask(
    cx: &mut Change<'_>,
    id: ConversationId,
) -> Result<Option<String>> {
    let kept = conversations::get(cx.db, id)?;
    let (Kind::Chat, Some(chief)) = (kept.kind, kept.chief) else {
        return Ok(None);
    };
    let pending = reflection::table
        .filter(reflection::conversation.eq(id))
        .count()
        .get_result::<i64>(cx.db)?
        > 0;
    if pending {
        return Ok(None);
    }
    let number = chat::table
        .find((kept.agent, chief))
        .select(chat::reflections)
        .first::<i64>(cx.db)
        .optional()?
        .unwrap_or(0)
        + 1;
    let request = format!("reflection:{id}:{number}");
    let task = tasks::mint(
        cx,
        Origin::Reflection,
        kept.agent,
        Some(chief),
        None,
    )?;
    let kept_input = inputs::keep(
        cx,
        Keep {
            conversation: id,
            request: request.clone(),
            kind: InputKind::Reflection,
            task: Some(task),
            content: REFLECT.into(),
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
    diesel::update(chat::table.find((kept.agent, chief)))
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
