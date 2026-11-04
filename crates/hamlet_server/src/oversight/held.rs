//! The gate's kept decision of each tool call and the waits that hold
//! them, as the realm database keeps them in `gate_call` and `wait`.

use anyhow::Result;
use diesel::prelude::*;
use serde_json::Value;

use super::{
    ChatPlace, Danger, Raising, Ruling, Wait, WaitKind, Woken,
};
use crate::harness::BindingId;
use crate::record::AgentRef;
use crate::schema::{gate_call, wait};
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
    db: &mut SqliteConnection,
    id: &OperationId,
    binding: BindingId,
    digest: &[u8],
) -> Result<bool> {
    let (runtime, key) = (id.runtime.as_str(), id.key.as_str());
    // A call's marker is written only after its reservation.
    diesel::insert_into(gate_call::table)
        .values((
            gate_call::runtime.eq(runtime),
            gate_call::key.eq(key),
            gate_call::binding.eq(binding.0),
            gate_call::digest.eq(digest),
        ))
        .on_conflict_do_nothing()
        .execute(db)?;
    let (kept, kept_digest) = gate_call::table
        .find((runtime, key))
        .select((gate_call::binding, gate_call::digest))
        .first::<(i64, Vec<u8>)>(db)?;
    Ok(kept == binding.0 && kept_digest == digest)
}

/// Keeps that the gate let the call `id` run, with the network when
/// `net`.
pub(crate) fn allow(
    db: &mut SqliteConnection,
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
    db: &mut SqliteConnection,
    id: &OperationId,
    binding: BindingId,
    input: Option<&str>,
    words: &str,
    change: Option<&Value>,
) -> Result<()> {
    decide(db, id, binding, input, None, Some(words), change)
}

fn decide(
    db: &mut SqliteConnection,
    id: &OperationId,
    binding: BindingId,
    input: Option<&str>,
    net: Option<bool>,
    block: Option<&str>,
    change: Option<&Value>,
) -> Result<()> {
    diesel::update(
        gate_call::table
            .find((id.runtime.as_str(), &id.key))
            .filter(gate_call::binding.eq(binding.0)),
    )
    .set((
        gate_call::input.eq(input),
        gate_call::net.eq(net),
        gate_call::block.eq(block),
        gate_call::change.eq(change.map(Value::to_string)),
    ))
    .execute(db)?;
    Ok(())
}

/// What the gate keeps of the call `id`.
pub(crate) fn decision(
    db: &mut SqliteConnection,
    id: &OperationId,
) -> Result<Option<HeldCall>> {
    gate_call::table
        .left_join(wait::table)
        .filter(gate_call::runtime.eq(id.runtime.as_str()))
        .filter(gate_call::key.eq(&id.key))
        .select((
            gate_call::binding,
            gate_call::input,
            gate_call::change,
            gate_call::net,
            gate_call::block,
            gate_call::wait,
            gate_call::how,
            gate_call::text,
            wait::questions.is_not_null().nullable(),
        ))
        .first::<(
            i64,
            Option<String>,
            Option<String>,
            Option<bool>,
            Option<String>,
            Option<String>,
            Option<Ruling>,
            Option<String>,
            Option<bool>,
        )>(db)
        .optional()?
        .map(
            |(
                binding,
                input,
                change,
                net,
                block,
                wait,
                ruling,
                text,
                structured,
            )| {
                let state = match (block, wait, ruling) {
                    (Some(words), ..) => CallState::Blocked(words),
                    (None, Some(wait), Some(ruling)) => {
                        CallState::Ruled {
                            wait,
                            ruling,
                            text: text.unwrap_or_default(),
                            net: net.unwrap_or(false),
                            structured: structured.unwrap_or(false),
                        }
                    }
                    (None, Some(wait), None) => {
                        CallState::Held { wait }
                    }
                    (None, None, _) => match net {
                        Some(net) => CallState::Allowed { net },
                        None => CallState::Open,
                    },
                };
                Ok(HeldCall {
                    binding: BindingId(binding),
                    input,
                    change: change
                        .map(|change| serde_json::from_str(&change))
                        .transpose()?,
                    state,
                })
            },
        )
        .transpose()
}

/// Sets whether the call `id`, allowed by a chief, has the network.
pub(crate) fn set_net(
    db: &mut SqliteConnection,
    id: &OperationId,
    net: bool,
) -> Result<()> {
    diesel::update(
        gate_call::table.find((id.runtime.as_str(), &id.key)),
    )
    .set(gate_call::net.eq(net))
    .execute(db)?;
    Ok(())
}

/// Refuses, in `words`, every call of `binding` not yet carried out
/// or refused. A marked result stands.
pub(crate) fn block_unrun(
    db: &mut SqliteConnection,
    binding: BindingId,
    words: &str,
) -> Result<()> {
    let open = gate_call::table
        .filter(gate_call::binding.eq(binding.0))
        .filter(gate_call::block.is_null())
        .select((gate_call::runtime, gate_call::key))
        .load::<(String, String)>(db)?;
    for (runtime, key) in open {
        if !crate::operation::is_marked(db, &runtime, &key)? {
            diesel::update(gate_call::table.find((&runtime, &key)))
                .set(gate_call::block.eq(words))
                .execute(db)?;
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
    db: &mut SqliteConnection,
    wait: &str,
) -> Result<Vec<Unruled>> {
    gate_call::table
        .filter(gate_call::wait.eq(wait))
        .filter(gate_call::how.is_null())
        .select((
            gate_call::runtime,
            gate_call::key,
            gate_call::binding,
            gate_call::input,
        ))
        .load::<(String, String, i64, Option<String>)>(db)?
        .into_iter()
        .map(|(runtime, key, binding, input)| {
            Ok(Unruled {
                id: OperationId {
                    runtime: RuntimeInstance::new(runtime)?,
                    key,
                },
                binding: BindingId(binding),
                input,
            })
        })
        .collect()
}

/// Rules on each call the wait `wait` holds that nobody ruled on
/// yet; gives them.
pub(crate) fn rule(
    db: &mut SqliteConnection,
    wait: &str,
    ruling: Ruling,
    text: &str,
) -> Result<Woken> {
    let woken = unruled(db, wait)?
        .into_iter()
        .map(|call| (call.id.runtime, call.id.key))
        .collect();
    diesel::update(
        gate_call::table
            .filter(gate_call::wait.eq(wait))
            .filter(gate_call::how.is_null()),
    )
    .set((gate_call::how.eq(ruling), gate_call::text.eq(text)))
    .execute(db)?;
    Ok(woken)
}

/// The open waits that hold calls of `binding` not ruled on.
pub(crate) fn waits_holding(
    db: &mut SqliteConnection,
    binding: BindingId,
) -> Result<Vec<String>> {
    Ok(gate_call::table
        .filter(gate_call::binding.eq(binding.0))
        .filter(gate_call::wait.is_not_null())
        .filter(gate_call::how.is_null())
        .select(gate_call::wait.assume_not_null())
        .distinct()
        .load(db)?)
}

/// The open wait that holds the call `id`.
#[cfg(feature = "harness")]
pub(crate) fn open_wait_of(
    db: &mut SqliteConnection,
    id: &OperationId,
) -> Result<Option<String>> {
    Ok(gate_call::table
        .inner_join(wait::table)
        .filter(gate_call::runtime.eq(id.runtime.as_str()))
        .filter(gate_call::key.eq(&id.key))
        .filter(wait::open.eq(true))
        .select(gate_call::wait.assume_not_null())
        .first(db)
        .optional()?)
}

/// Whether the call `key` of `runtime` belongs to a turn that still
/// runs, and was not carried out.
pub(crate) fn unfinished(
    db: &mut SqliteConnection,
    runtime: &str,
    key: &str,
) -> Result<bool> {
    let binding = gate_call::table
        .find((runtime, key))
        .select((gate_call::binding, gate_call::input))
        .first::<(i64, Option<String>)>(db)
        .optional()?;
    let Some((binding, Some(input))) = binding else {
        return Ok(false);
    };
    Ok(crate::harness::input_runs(db, BindingId(binding), &input)?
        && !crate::operation::is_marked(db, runtime, key)?)
}

/// Raises a wait, or joins the open one that waits for the same thing
/// by the same agent, and holds its call. A call already held finds
/// its wait again; one already ruled raises nothing.
pub(crate) fn raise(
    db: &mut SqliteConnection,
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
    db: &mut SqliteConnection,
    raising: Raising,
    at: i64,
) -> Result<String> {
    let questions = raising
        .kind
        .questions()
        .map(serde_json::to_string)
        .transpose()?;
    let (kind, danger) = (raising.kind.name(), raising.kind.danger());
    let same = wait::table
        .filter(wait::open.eq(true))
        .filter(wait::kind.eq(kind))
        .filter(wait::danger.is(danger))
        .filter(wait::by.eq(&raising.by))
        .filter(wait::about.eq(&raising.about))
        .filter(wait::questions.is(&questions))
        .select(wait::id)
        .first::<String>(db)
        .optional()?;
    let id = match same {
        Some(id) => {
            diesel::update(wait::table.find(&id))
                .set(wait::asked.eq(wait::asked + 1))
                .execute(db)?;
            id
        }
        None => {
            let ordinal = wait::table
                .select(diesel::dsl::max(wait::ordinal))
                .first::<Option<i64>>(db)?
                .unwrap_or(0)
                + 1;
            let id = format!("{at}-{ordinal}");
            let chat = raising.chat.as_ref();
            diesel::insert_into(wait::table)
                .values((
                    wait::id.eq(&id),
                    wait::ordinal.eq(ordinal),
                    wait::kind.eq(kind),
                    wait::danger.eq(danger),
                    wait::by.eq(&raising.by),
                    wait::hamlet.eq(raising
                        .agent
                        .as_ref()
                        .map(|a| &a.hamlet)),
                    wait::number.eq(raising
                        .agent
                        .as_ref()
                        .map(|a| i64::try_from(a.number))
                        .transpose()?),
                    wait::about.eq(&raising.about),
                    wait::change.eq(raising
                        .change
                        .as_ref()
                        .map(Value::to_string)),
                    wait::answers
                        .eq(serde_json::to_string(&raising.answers)?),
                    wait::since.eq(at),
                    wait::asked.eq(1),
                    wait::chat_hamlet.eq(chat.map(|c| &c.hamlet)),
                    wait::chat_number.eq(chat
                        .map(|c| i64::try_from(c.number))
                        .transpose()?),
                    wait::chat_chief.eq(chat.map(|c| &c.chief)),
                    wait::chat_session.eq(chat.map(|c| c.session)),
                    wait::questions.eq(&questions),
                ))
                .execute(db)?;
            id
        }
    };
    if let Some(call) = &raising.call {
        diesel::update(
            gate_call::table
                .find((call.id.runtime.as_str(), &call.id.key))
                .filter(gate_call::binding.eq(call.binding.0)),
        )
        .set((
            gate_call::input.eq(&call.input),
            gate_call::net.eq(None::<bool>),
            gate_call::block.eq(None::<String>),
            gate_call::change
                .eq(call.change.as_ref().map(Value::to_string)),
            gate_call::wait.eq(&id),
        ))
        .execute(db)?;
    }
    Ok(id)
}

/// Ends the open wait `id` with `ruling`.
pub(crate) fn close(
    db: &mut SqliteConnection,
    id: &str,
    ruling: Ruling,
    text: &str,
    chief: Option<&str>,
    at: i64,
) -> Result<()> {
    diesel::update(wait::table.find(id))
        .set((
            wait::open.eq(false),
            wait::how.eq(ruling),
            wait::text.eq(text),
            wait::chief.eq(chief),
            wait::settled.eq(at),
        ))
        .execute(db)?;
    Ok(())
}

/// Lets the open wait `id` be only dismissed, with `note` added to
/// its text.
pub(crate) fn only_dismiss(
    db: &mut SqliteConnection,
    id: &str,
    note: &str,
) -> Result<()> {
    const DISMISS: &str = r#"["dismiss"]"#;
    diesel::update(
        wait::table
            .find(id)
            .filter(wait::open.eq(true))
            .filter(wait::answers.ne(DISMISS)),
    )
    .set((
        wait::answers.eq(DISMISS),
        wait::about.eq(wait::about.concat("\n").concat(note)),
    ))
    .execute(db)?;
    Ok(())
}

/// The open waits in the order they were raised.
pub(crate) fn open(db: &mut SqliteConnection) -> Result<Vec<Wait>> {
    wait::table
        .filter(wait::open.eq(true))
        .order(wait::ordinal)
        .select(WaitRow::as_select())
        .load(db)?
        .into_iter()
        .map(WaitRow::wait)
        .collect()
}

/// The open wait `id`.
pub(crate) fn find(
    db: &mut SqliteConnection,
    id: &str,
) -> Result<Option<Wait>> {
    wait::table
        .find(id)
        .filter(wait::open.eq(true))
        .select(WaitRow::as_select())
        .first(db)
        .optional()?
        .map(WaitRow::wait)
        .transpose()
}

/// How the wait `id` ended, if it did and that was kept.
pub(crate) fn ended(
    db: &mut SqliteConnection,
    id: &str,
) -> Result<Option<Ruling>> {
    Ok(wait::table
        .find(id)
        .filter(wait::open.eq(false))
        .select(wait::how)
        .first::<Option<Ruling>>(db)
        .optional()?
        .flatten())
}

/// A row of `wait`, as [`Wait`] reads it.
#[derive(Queryable, Selectable)]
#[diesel(table_name = wait)]
struct WaitRow {
    id: String,
    kind: String,
    danger: Option<Danger>,
    by: String,
    hamlet: Option<String>,
    number: Option<i64>,
    about: String,
    change: Option<String>,
    answers: String,
    since: i64,
    asked: i64,
    chat_hamlet: Option<String>,
    chat_number: Option<i64>,
    chat_chief: Option<String>,
    chat_session: Option<i64>,
    questions: Option<String>,
}

impl WaitRow {
    fn wait(self) -> Result<Wait> {
        Ok(Wait {
            id: self.id,
            kind: WaitKind::of(
                &self.kind,
                self.danger,
                self.questions
                    .map(|q| serde_json::from_str(&q))
                    .transpose()?,
            )?,
            by: self.by,
            agent: match (self.hamlet, self.number) {
                (Some(hamlet), Some(number)) => Some(AgentRef {
                    hamlet,
                    number: u64::try_from(number)?,
                }),
                _ => None,
            },
            about: self.about,
            change: self
                .change
                .map(|c| serde_json::from_str(&c))
                .transpose()?,
            answers: serde_json::from_str(&self.answers)?,
            since: self.since,
            asked: self.asked,
            chat: match (
                self.chat_hamlet,
                self.chat_number,
                self.chat_chief,
                self.chat_session,
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
    }
}
