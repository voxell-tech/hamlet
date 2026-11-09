//! The gate's decision on each tool call, kept by its runtime and
//! key: reserved for one request, then allowed, refused or held on a
//! wait.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{ConversationId, WaitId};
use hamlet_store::schema::{tool_call, wait};

use crate::oversight::{Ruling, WaitKind};

/// Where a call stands with the gate.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum State {
    /// Reserved, not decided.
    Open,
    /// Let run without asking; made again each time, as grants and
    /// trust change.
    Allowed {
        net: bool,
    },
    Blocked(String),
    /// Waiting for a chief.
    Held(WaitId),
    /// A chief answered its wait.
    Ruled {
        ruling: Ruling,
        text: String,
        net: bool,
    },
    /// Held while a chief considers an objection to its turn's
    /// instruction.
    Objected,
}

/// The words of a call whose instruction a chief withdrew.
pub(crate) const WITHDRAWN: &str =
    "The chief withdrew the instruction, so this call did not run.";

/// A call's decision, with the change an edit makes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Kept {
    pub state: State,
    pub change: Option<String>,
    pub input: Option<String>,
    pub conversation: ConversationId,
}

/// Reserves `key` of `runtime` for the request `digest`. `false` when
/// it is reserved for another request.
pub(crate) fn reserve(
    db: &mut Db,
    runtime: &str,
    key: &str,
    conversation: ConversationId,
    digest: &[u8],
    input: Option<&str>,
    tool: &str,
) -> Result<bool> {
    let kept = tool_call::table
        .find((runtime, key))
        .select(tool_call::digest)
        .first::<Vec<u8>>(db)
        .optional()?;
    match kept {
        Some(kept) => Ok(kept == digest),
        None => {
            diesel::insert_into(tool_call::table)
                .values((
                    tool_call::runtime.eq(runtime),
                    tool_call::key.eq(key),
                    tool_call::conversation.eq(conversation),
                    tool_call::digest.eq(digest),
                    tool_call::input.eq(input),
                    tool_call::tool.eq(tool),
                ))
                .execute(db)?;
            Ok(true)
        }
    }
}

/// What the gate decided of `key` of `runtime`.
pub(crate) fn kept(
    db: &mut Db,
    runtime: &str,
    key: &str,
) -> Result<Option<Kept>> {
    let row = tool_call::table
        .find((runtime, key))
        .select((
            tool_call::conversation,
            tool_call::input,
            tool_call::net,
            tool_call::block,
            tool_call::wait,
            tool_call::change,
        ))
        .first::<(
            ConversationId,
            Option<String>,
            Option<bool>,
            Option<String>,
            Option<WaitId>,
            Option<String>,
        )>(db)
        .optional()?;
    let Some((conversation, input, net, block, held, change)) = row
    else {
        return Ok(None);
    };
    let state = if let Some(words) = block {
        State::Blocked(words)
    } else if let Some(id) = held {
        let (kind, open, how, text) = wait::table
            .find(id)
            .select((wait::kind, wait::open, wait::how, wait::text))
            .first::<(WaitKind, bool, Option<Ruling>, Option<String>)>(db)?;
        match (kind, open, how) {
            (WaitKind::Objection, true, _) => State::Objected,
            // A call held by an objection that stands is decided
            // afresh.
            (WaitKind::Objection, false, Some(Ruling::Go)) => {
                State::Open
            }
            (WaitKind::Objection, false, _) => {
                State::Blocked(WITHDRAWN.into())
            }
            (_, true, _) | (_, false, None) => State::Held(id),
            (_, false, Some(ruling)) => State::Ruled {
                ruling,
                text: text.unwrap_or_default(),
                net: net.unwrap_or(false),
            },
        }
    } else if let Some(net) = net {
        State::Allowed { net }
    } else {
        State::Open
    };
    Ok(Some(Kept {
        state,
        change,
        input,
        conversation,
    }))
}

pub(crate) fn allow(
    db: &mut Db,
    runtime: &str,
    key: &str,
    net: bool,
    change: Option<&str>,
) -> Result<()> {
    diesel::update(tool_call::table.find((runtime, key)))
        .set((tool_call::net.eq(net), tool_call::change.eq(change)))
        .execute(db)?;
    Ok(())
}

pub(crate) fn block(
    db: &mut Db,
    runtime: &str,
    key: &str,
    words: &str,
    change: Option<&str>,
) -> Result<()> {
    diesel::update(tool_call::table.find((runtime, key)))
        .set((
            tool_call::block.eq(words),
            tool_call::change.eq(change),
        ))
        .execute(db)?;
    Ok(())
}

pub(crate) fn hold(
    db: &mut Db,
    runtime: &str,
    key: &str,
    on: WaitId,
    change: Option<&str>,
) -> Result<()> {
    diesel::update(tool_call::table.find((runtime, key)))
        .set((tool_call::wait.eq(on), tool_call::change.eq(change)))
        .execute(db)?;
    Ok(())
}

/// A call a wait holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Holding {
    pub runtime: String,
    pub key: String,
    pub conversation: ConversationId,
    pub input: Option<String>,
}

/// The calls `on` holds.
pub(crate) fn holding(
    db: &mut Db,
    on: WaitId,
) -> Result<Vec<Holding>> {
    Ok(tool_call::table
        .filter(tool_call::wait.eq(on))
        .select((
            tool_call::runtime,
            tool_call::key,
            tool_call::conversation,
            tool_call::input,
        ))
        .load::<(String, String, ConversationId, Option<String>)>(db)?
        .into_iter()
        .map(|(runtime, key, conversation, input)| Holding {
            runtime,
            key,
            conversation,
            input,
        })
        .collect())
}

/// Sets the network a call a chief allowed runs with.
pub(crate) fn set_net(
    db: &mut Db,
    runtime: &str,
    key: &str,
    net: bool,
) -> Result<()> {
    diesel::update(tool_call::table.find((runtime, key)))
        .set(tool_call::net.eq(net))
        .execute(db)?;
    Ok(())
}
