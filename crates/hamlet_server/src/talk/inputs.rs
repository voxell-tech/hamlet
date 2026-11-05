//! Inputs: what a conversation is given, kept before it is sent and
//! taken in once, in order (S23).

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ConversationId, TaskId};
use hamlet_store::schema::{conversation, input};

use super::marks::Mark;
use crate::change::Change;
use crate::text::text_enum;

text_enum! {
    pub(crate) enum InputKind {
        Chief = "chief",
        Retry = "retry",
        Reflection = "reflection",
        Notice = "notice",
        Letter = "letter",
        Question = "question",
        Reply = "reply",
        Errand = "errand",
        Worker = "worker",
        Dream = "dream",
    }
}

text_enum! {
    /// Where an input is on its way to its runtime.
    pub(crate) enum State {
        Kept = "kept",
        Admitted = "admitted",
        /// Never to be sent: its conversation was stopped first.
        Dropped = "dropped",
    }
}

text_enum! {
    /// What its runtime last said of an admitted input.
    pub(crate) enum Status {
        Queued = "queued",
        Placed = "placed",
        Done = "done",
        Unanswered = "unanswered",
    }
}

impl Status {
    pub fn is_ended(self) -> bool {
        matches!(self, Self::Done | Self::Unanswered)
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = input)]
pub(crate) struct Input {
    pub conversation: ConversationId,
    pub request: String,
    pub ordinal: i64,
    pub kind: InputKind,
    pub task: Option<TaskId>,
    pub content: String,
    pub mark: String,
    pub steer: bool,
    pub boundary: bool,
    pub state: State,
    pub submission: Option<String>,
    pub status: Option<Status>,
    pub reason: Option<String>,
    pub detail: Option<String>,
    pub followed: bool,
}

impl Input {
    pub fn mark(&self) -> Result<Mark> {
        Ok(serde_json::from_str(&self.mark)?)
    }

    /// Whether it is written into the transcript rather than
    /// answered.
    pub fn is_write(&self) -> bool {
        self.kind == InputKind::Notice
    }

    pub fn is_ended(&self) -> bool {
        self.status.is_some_and(Status::is_ended)
    }
}

/// An input to keep.
#[derive(Clone, Debug)]
pub(crate) struct Keep {
    pub conversation: ConversationId,
    /// The server's id for it, which its kind begins.
    pub request: String,
    pub kind: InputKind,
    pub task: Option<TaskId>,
    pub content: String,
    pub mark: Mark,
    /// Whether it steers a running turn rather than waiting for it.
    pub steer: bool,
    /// Whether a notice ends a turn's queue in the transcript.
    pub boundary: bool,
}

/// Keeps an input, after every other of its conversation. Returns
/// `false` when its request was kept already, which keeps nothing.
pub(crate) fn keep(cx: &mut Change<'_>, keep: Keep) -> Result<bool> {
    let ordinal = input::table
        .filter(input::conversation.eq(keep.conversation))
        .select(diesel::dsl::max(input::ordinal))
        .first::<Option<i64>>(cx.db)?
        .map_or(0, |last| last + 1);
    let kept = diesel::insert_into(input::table)
        .values((
            input::conversation.eq(keep.conversation),
            input::request.eq(&keep.request),
            input::ordinal.eq(ordinal),
            input::kind.eq(keep.kind),
            input::task.eq(keep.task),
            input::content.eq(&keep.content),
            input::mark.eq(serde_json::to_string(&keep.mark)?),
            input::steer.eq(keep.steer),
            input::boundary.eq(keep.boundary),
            input::state.eq(State::Kept),
        ))
        .on_conflict_do_nothing()
        .execute(cx.db)?;
    Ok(kept == 1)
}

pub(crate) fn find(
    db: &mut Db,
    conversation: ConversationId,
    request: &str,
) -> Result<Option<Input>> {
    Ok(input::table
        .find((conversation, request))
        .select(Input::as_select())
        .first(db)
        .optional()?)
}

/// The conversation the instruction of `task` was given in.
pub(crate) fn of_task(
    db: &mut Db,
    task: TaskId,
) -> Result<Option<ConversationId>> {
    Ok(input::table
        .filter(input::task.eq(task))
        .order(input::ordinal)
        .select(input::conversation)
        .first(db)
        .optional()?)
}

/// The input its runtime calls `submission`.
pub(crate) fn by_submission(
    db: &mut Db,
    conversation: ConversationId,
    submission: &str,
) -> Result<Option<Input>> {
    Ok(input::table
        .filter(input::conversation.eq(conversation))
        .filter(input::submission.eq(submission))
        .select(Input::as_select())
        .first(db)
        .optional()?)
}

/// What `conversation` is owed, in order: its kept inputs, and when
/// `again` also those admitted whose end was not heard, so their
/// runtime answers again.
pub(crate) fn owed(
    db: &mut Db,
    conversation: ConversationId,
    again: bool,
) -> Result<Vec<Input>> {
    let rows = input::table
        .filter(input::conversation.eq(conversation))
        .filter(input::state.ne(State::Dropped))
        .order(input::ordinal)
        .select(Input::as_select())
        .load::<Input>(db)?;
    Ok(rows
        .into_iter()
        .filter(|row| match row.state {
            State::Kept => true,
            State::Admitted => again && !row.is_ended(),
            State::Dropped => false,
        })
        .collect())
}

/// Keeps what its runtime said of an input it admitted. Returns
/// whether the input has ended now and had not before.
pub(crate) fn admitted(
    db: &mut Db,
    conversation: ConversationId,
    request: &str,
    submission: &str,
    status: Status,
    reason: Option<&str>,
    detail: Option<&str>,
) -> Result<bool> {
    let Some(kept) = find(db, conversation, request)? else {
        return Ok(false);
    };
    if kept.state == State::Dropped || kept.is_ended() {
        return Ok(false);
    }
    diesel::update(input::table.find((conversation, request)))
        .set((
            input::state.eq(State::Admitted),
            input::submission.eq(submission),
            input::status.eq(status),
            input::reason.eq(reason),
            input::detail.eq(detail),
        ))
        .execute(db)?;
    Ok(status.is_ended())
}

/// Keeps that the input `request` ended. Returns whether it had not
/// ended before.
pub(crate) fn settled(
    db: &mut Db,
    conversation: ConversationId,
    request: &str,
    status: Status,
    reason: Option<&str>,
    detail: Option<&str>,
) -> Result<bool> {
    let Some(kept) = find(db, conversation, request)? else {
        return Ok(false);
    };
    if kept.is_ended() {
        return Ok(false);
    }
    diesel::update(input::table.find((conversation, request)))
        .set((
            input::state.eq(State::Admitted),
            input::status.eq(status),
            input::reason.eq(reason),
            input::detail.eq(detail),
        ))
        .execute(db)?;
    Ok(true)
}

/// Drops every input of `conversation` not admitted yet, returning
/// their requests.
pub(crate) fn drop_kept(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<Vec<String>> {
    let requests = input::table
        .filter(input::conversation.eq(conversation))
        .filter(input::state.eq(State::Kept))
        .select(input::request)
        .load::<String>(db)?;
    diesel::update(
        input::table
            .filter(input::conversation.eq(conversation))
            .filter(input::state.eq(State::Kept)),
    )
    .set(input::state.eq(State::Dropped))
    .execute(db)?;
    Ok(requests)
}

/// Whether `conversation` has an input to answer not ended: kept, or
/// admitted with no end heard.
pub(crate) fn is_busy(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<bool> {
    Ok(open(db, &[conversation])? > 0)
}

/// Whether any conversation of `agent` has an input to answer not
/// ended.
pub(crate) fn is_agent_busy(
    db: &mut Db,
    agent: AgentId,
) -> Result<bool> {
    let ids = conversation::table
        .filter(conversation::agent.eq(agent))
        .select(conversation::id)
        .load::<ConversationId>(db)?;
    Ok(open(db, &ids)? > 0)
}

/// How many inputs to answer of `conversations` are neither dropped
/// nor ended.
fn open(
    db: &mut Db,
    conversations: &[ConversationId],
) -> Result<i64> {
    Ok(input::table
        .filter(input::conversation.eq_any(conversations))
        .filter(input::state.ne(State::Dropped))
        .filter(input::kind.ne(InputKind::Notice))
        .filter(
            input::status.is_null().or(input::status
                .ne(Status::Done)
                .and(input::status.ne(Status::Unanswered))),
        )
        .count()
        .get_result(db)?)
}

/// The inputs that ended and were not followed yet.
pub(crate) fn unfollowed(db: &mut Db) -> Result<Vec<Input>> {
    Ok(input::table
        .filter(input::followed.eq(false))
        .filter(
            input::status
                .eq(Status::Done)
                .or(input::status.eq(Status::Unanswered)),
        )
        .order((input::conversation, input::ordinal))
        .select(Input::as_select())
        .load(db)?)
}

/// Marks the input followed, once. Returns whether it was not before.
pub(crate) fn follow(
    db: &mut Db,
    conversation: ConversationId,
    request: &str,
) -> Result<bool> {
    Ok(diesel::update(
        input::table
            .find((conversation, request))
            .filter(input::followed.eq(false)),
    )
    .set(input::followed.eq(true))
    .execute(db)?
        == 1)
}
