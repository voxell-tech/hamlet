//! Inputs: what a conversation is given, kept before it is sent and
//! taken in once, in order (S23).

use anyhow::Result;
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::sql_types::Text;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ConversationId, TaskId};
use hamlet_store::schema::{conversation, input};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use super::marks::Mark;
use crate::realm::change::Change;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
    IntoStaticStr,
    EnumString,
    Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[diesel(sql_type = Text)]
pub(crate) enum InputKind {
    Chief,
    Retry,
    Reflection,
    Notice,
    Notification,
    Request,
    Response,
    /// Why a request or a worker has no answer, written into the
    /// context without a turn.
    ErrorResponse,
    Errand,
    Worker,
    Dream,
    /// A summary of the conversation's earlier part, asked now.
    Compaction,
    /// What a hire merged into this one said and did, added to the
    /// context.
    Merged,
}
hamlet_store::named_text!(InputKind);

/// The kinds of input no turn answers.
const NOT_TURNS: [InputKind; 4] = [
    InputKind::Notice,
    InputKind::Merged,
    InputKind::ErrorResponse,
    InputKind::Compaction,
];

impl InputKind {
    /// Whether its turn runs ahead of the others queued: a chief's
    /// own.
    fn is_first(self) -> bool {
        matches!(self, Self::Chief | Self::Retry)
    }
}

/// Where an input is on its way to its runtime.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
    IntoStaticStr,
    EnumString,
    Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[diesel(sql_type = Text)]
pub(crate) enum State {
    Kept,
    Admitted,
    /// Never to be sent: its conversation was stopped first.
    Dropped,
}
hamlet_store::named_text!(State);

/// What its runtime last said of an admitted input.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
    IntoStaticStr,
    EnumString,
    Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[diesel(sql_type = Text)]
pub(crate) enum Status {
    Queued,
    Placed,
    Done,
    Unanswered,
}
hamlet_store::named_text!(Status);

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
        matches!(
            self.kind,
            InputKind::Notice
                | InputKind::Merged
                | InputKind::ErrorResponse
        )
    }

    /// Whether a turn answers it.
    pub fn is_turn(&self) -> bool {
        !self.is_write() && self.kind != InputKind::Compaction
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

/// The inputs a runtime is sent again.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resend {
    Nothing,
    /// Those it admitted whose end was not heard, so it answers them
    /// again.
    Unended,
}

/// What a chief's stop and the line after it do to the order of a
/// conversation's turns.
struct Hold {
    /// No queued turn starts.
    held: bool,
    /// The input that runs before the rest queued.
    first: Option<String>,
}

fn hold_of(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<Hold> {
    let (held, first) = conversation::table
        .find(conversation)
        .select((conversation::held, conversation::runs_first))
        .first::<(bool, Option<String>)>(db)?;
    Ok(Hold { held, first })
}

/// What `conversation` is owed, in order: its kept writes and
/// compactions, and one turn at a time. With no turn open and no
/// hold, that is the next kept input a turn answers, a chief's first;
/// with an open turn and `resend` [`Resend::Unended`], it is the one
/// admitted whose end was not heard, so its runtime answers it again.
/// `sent` says which kept inputs are on their way, one of which holds
/// the next back as an open turn does.
pub(crate) fn owed(
    db: &mut Db,
    conversation: ConversationId,
    resend: Resend,
    sent: impl Fn(&str) -> bool,
) -> Result<Vec<Input>> {
    let hold = hold_of(db, conversation)?;
    let rows = input::table
        .filter(input::conversation.eq(conversation))
        .filter(input::state.ne(State::Dropped))
        .order(input::ordinal)
        .select(Input::as_select())
        .load::<Input>(db)?;
    let open = rows
        .iter()
        .filter(|row| {
            row.is_turn()
                && row.state == State::Admitted
                && !row.is_ended()
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut owed = rows
        .iter()
        .filter(|row| row.state == State::Kept && !row.is_turn())
        .cloned()
        .collect::<Vec<_>>();
    let waiting = rows
        .iter()
        .filter(|row| row.state == State::Kept && row.is_turn())
        .collect::<Vec<_>>();
    if !open.is_empty() {
        if resend == Resend::Unended {
            owed.extend(open);
        }
    } else if let Some(on_way) =
        waiting.iter().find(|row| sent(&row.request))
    {
        owed.push((*on_way).clone());
    } else if !hold.held
        && let Some(next) =
            in_turn_order(waiting, hold.first.as_deref()).first()
    {
        owed.push((*next).clone());
    }
    owed.sort_by_key(|row| row.ordinal);
    Ok(owed)
}

/// `waiting` in the order their turns run: `first`, then a chief's,
/// then the rest as they came.
fn in_turn_order<'a>(
    mut waiting: Vec<&'a Input>,
    first: Option<&str>,
) -> Vec<&'a Input> {
    waiting.sort_by_key(|row| {
        (
            Some(row.request.as_str()) != first,
            !row.kind.is_first(),
            row.ordinal,
        )
    });
    waiting
}

/// The inputs of `conversation` that wait for a turn to take them
/// up, in the order their turns run.
pub(crate) fn queued(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<Vec<Input>> {
    let first = hold_of(db, conversation)?.first;
    let rows = input::table
        .filter(input::conversation.eq(conversation))
        .filter(input::state.ne(State::Dropped))
        .filter(
            input::status
                .is_null()
                .or(input::status.eq(Status::Queued)),
        )
        .order(input::ordinal)
        .select(Input::as_select())
        .load::<Input>(db)?;
    Ok(in_turn_order(
        rows.iter().filter(|row| row.is_turn()).collect(),
        first.as_deref(),
    )
    .into_iter()
    .cloned()
    .collect())
}

/// Takes the kept input `request` out of its queue, never to be
/// sent.
pub(crate) fn withdraw(
    db: &mut Db,
    conversation: ConversationId,
    request: &str,
) -> Result<()> {
    diesel::update(
        input::table
            .find((conversation, request))
            .filter(input::state.eq(State::Kept)),
    )
    .set(input::state.eq(State::Dropped))
    .execute(db)?;
    Ok(())
}

/// Whether `conversation` runs a turn now: an input sent to its
/// runtime with no end heard.
pub(crate) fn is_running(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<bool> {
    Ok(input::table
        .filter(input::conversation.eq(conversation))
        .filter(input::state.eq(State::Admitted))
        .filter(input::kind.ne_all(NOT_TURNS))
        .filter(
            input::status.is_null().or(input::status
                .ne(Status::Done)
                .and(input::status.ne(Status::Unanswered))),
        )
        .count()
        .get_result::<i64>(db)?
        > 0)
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
        .filter(input::kind.ne_all(NOT_TURNS))
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
