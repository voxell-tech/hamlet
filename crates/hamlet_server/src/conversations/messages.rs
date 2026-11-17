//! Notifications and requests between agents along roads (H30, H31):
//! each kept once per tool call as an input to the receiver's one
//! conversation, and a request's final answer carried back to its
//! sender's as a response.

use anyhow::Result;
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::sql_types::Text;
use hamlet_protocol::client::ImageId;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ConversationId, TaskId};
use hamlet_store::schema::{input, message, tool_call};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use super::inputs::{self, InputKind, Keep};
use super::marks::{From, Mark};
use super::{chats, items};
use crate::conversations::{self, Kind};
use crate::realm::change::Change;
use crate::realm::settings::EffectiveSettings;
use crate::society::addresses;

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
pub(crate) enum MessageKind {
    Notification,
    Request,
}
hamlet_store::named_text!(MessageKind);

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = message)]
pub(crate) struct Message {
    pub id: i64,
    pub kind: MessageKind,
    pub runtime: String,
    pub key: String,
    pub sender: AgentId,
    pub receiver: AgentId,
    pub from_conversation: ConversationId,
    pub to_conversation: ConversationId,
    pub address: String,
    pub task: TaskId,
    pub text: String,
    pub at: i64,
    pub input: Option<String>,
    pub response: Option<String>,
    pub failure: Option<String>,
    pub carried: bool,
}

/// A message to keep.
pub(crate) struct Send<'a> {
    pub kind: MessageKind,
    /// The call that sends it, by its runtime and key.
    pub runtime: &'a str,
    pub key: &'a str,
    pub sender: AgentId,
    pub receiver: AgentId,
    /// The sender's conversation that sends it.
    pub from: ConversationId,
    /// The receiver as the sender named it.
    pub address: &'a str,
    pub task: TaskId,
    pub text: &'a str,
    /// The images sent with it, kept as images.
    pub images: &'a [ImageId],
}

/// Keeps `send` once per call: its row and its input to the receiver.
pub(crate) fn send(
    cx: &mut Change<'_>,
    env: &EffectiveSettings,
    send: Send<'_>,
) -> Result<Message> {
    if let Some(found) = find(cx.db, send.runtime, send.key)? {
        return Ok(found);
    }
    let to = chats::current(cx, env, send.receiver)?;
    let id: i64 = diesel::insert_into(message::table)
        .values((
            message::kind.eq(send.kind),
            message::runtime.eq(send.runtime),
            message::key.eq(send.key),
            message::sender.eq(send.sender),
            message::receiver.eq(send.receiver),
            message::from_conversation.eq(send.from),
            message::to_conversation.eq(to),
            message::address.eq(send.address),
            message::task.eq(send.task),
            message::text.eq(send.text),
            message::at.eq(cx.at),
        ))
        .returning(message::id)
        .get_result(cx.db)?;
    let request = format!("{}:{id}", send.kind);
    let from = From {
        from: send.sender,
        author: addresses::name_of(cx.db, send.sender)?,
        text: send.text.to_owned(),
        at: cx.at,
        images: send.images.iter().map(ImageId::to_string).collect(),
    };
    let (input_kind, mark) = match send.kind {
        MessageKind::Notification => {
            (InputKind::Notification, Mark::Notification(from))
        }
        MessageKind::Request => {
            (InputKind::Request, Mark::Request(from))
        }
    };
    inputs::keep(
        cx,
        Keep {
            conversation: to,
            request: request.clone(),
            kind: input_kind,
            task: Some(send.task),
            content: mark.heading().unwrap_or_default(),
            mark,
            steer: false,
            boundary: false,
        },
    )?;
    diesel::update(message::table.find(id))
        .set(message::input.eq(&request))
        .execute(cx.db)?;
    crate::society::hires::take_purpose(
        cx,
        send.receiver,
        send.text,
    )?;
    cx.touched.agents.insert(send.receiver);
    cx.touched.agents.insert(send.sender);
    Ok(message::table
        .find(id)
        .select(Message::as_select())
        .first(cx.db)?)
}

/// The message the call `key` of `runtime` sent.
pub(crate) fn find(
    db: &mut Db,
    runtime: &str,
    key: &str,
) -> Result<Option<Message>> {
    Ok(message::table
        .filter(message::runtime.eq(runtime))
        .filter(message::key.eq(key))
        .select(Message::as_select())
        .first(db)
        .optional()?)
}

/// The agents whose response `sender` waits for, by the requests it
/// sent that have none yet.
pub(crate) fn awaited(
    db: &mut Db,
    sender: AgentId,
) -> Result<Vec<AgentId>> {
    Ok(message::table
        .filter(message::sender.eq(sender))
        .filter(message::kind.eq(MessageKind::Request))
        .filter(message::response.is_null())
        .filter(message::failure.is_null())
        .order(message::at)
        .select(message::receiver)
        .load(db)?)
}

/// Keeps the response to the request whose input `request` of
/// `conversation` ended, once, and gives it to its sender.
pub(crate) fn responded(
    cx: &mut Change<'_>,
    conversation: ConversationId,
    request: &str,
    failed: Option<&str>,
) -> Result<Option<(String, String)>> {
    let found = message::table
        .filter(message::to_conversation.eq(conversation))
        .filter(message::input.eq(request))
        .filter(message::kind.eq(MessageKind::Request))
        .select(Message::as_select())
        .first(cx.db)
        .optional()?;
    let Some(found) = found else {
        return Ok(None);
    };
    if found.response.is_some() || found.failure.is_some() {
        return Ok(None);
    }
    let response = items::answer_of(cx.db, conversation, request)?;
    let failure = match (&response, failed) {
        (Some(_), None) => None,
        (_, Some(words)) => Some(words.to_owned()),
        (None, None) => Some("it gave no answer".to_owned()),
    };
    let carried = match failure {
        Some(_) => Carried::Error,
        None => Carried::Answer,
    };
    diesel::update(message::table.find(found.id))
        .set((
            message::response.eq(&response),
            message::failure.eq(&failure),
        ))
        .execute(cx.db)?;
    cx.touched.agents.insert(found.sender);
    let words = match (&response, &failure) {
        (Some(response), None) => response.clone(),
        (_, Some(failure)) => {
            format!("It could not answer: {failure}")
        }
        _ => String::new(),
    };
    carry(
        cx,
        Carry {
            runtime: &found.runtime,
            key: &found.key,
            from: found.receiver,
            task: found.task,
            request: &format!("response:{}", found.id),
            words: &words,
            carried,
            waits: false,
            author: None,
        },
    )
}

/// What a response carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Carried {
    /// An answer, which starts a turn of the caller's.
    Answer,
    /// Why there is no answer, given to the caller's conversation
    /// without starting a turn.
    Error,
}

/// Words for the caller of a tool call, from the agent it reached.
pub(crate) struct Carry<'a> {
    pub runtime: &'a str,
    pub key: &'a str,
    pub from: AgentId,
    pub task: TaskId,
    /// The id of the response, when it is kept as one.
    pub request: &'a str,
    pub words: &'a str,
    pub carried: Carried,
    /// Whether the call waits for the words.
    pub waits: bool,
    /// Who a response is shown as from, when not `from`'s name.
    pub author: Option<&'a str>,
}

/// Gives the words of `carry` to its call: the call to wake while it
/// waits in a turn that goes on, else a response kept for the
/// caller's conversation as it is now, an error one written in
/// without a turn.
pub(crate) fn carry(
    cx: &mut Change<'_>,
    carry: Carry<'_>,
) -> Result<Option<(String, String)>> {
    let Carry {
        runtime,
        key,
        from,
        task,
        request,
        words,
        carried,
        waits,
        author,
    } = carry;
    let call = tool_call::table
        .find((runtime, key))
        .select((tool_call::conversation, tool_call::input))
        .first::<(ConversationId, Option<String>)>(cx.db)
        .optional()?;
    let Some((sender, submission)) = call else {
        return Ok(None);
    };
    let waiting = waits
        && match &submission {
            Some(submission) => input::table
                .filter(input::conversation.eq(sender))
                .filter(input::submission.eq(submission))
                .select(input::status)
                .first::<Option<inputs::Status>>(cx.db)
                .optional()?
                .flatten()
                .is_none_or(|status| !status.is_ended()),
            None => false,
        };
    if waiting {
        return Ok(Some((runtime.to_owned(), key.to_owned())));
    }
    let author = match author {
        Some(author) => author.to_owned(),
        None => addresses::name_of(cx.db, from)?,
    };
    // A rewind since the call points the caller at a fork.
    let caller = conversations::get(cx.db, sender)?;
    let sender = match caller.kind {
        Kind::Chat => {
            chats::of(cx.db, caller.agent)?.unwrap_or(sender)
        }
        _ => sender,
    };
    let from = From {
        from,
        author,
        text: words.to_owned(),
        at: cx.at,
        images: Vec::new(),
    };
    let (kind, mark) = match carried {
        Carried::Answer => {
            (InputKind::Response, Mark::Response(from))
        }
        Carried::Error => {
            (InputKind::ErrorResponse, Mark::ErrorResponse(from))
        }
    };
    inputs::keep(
        cx,
        Keep {
            conversation: sender,
            request: request.to_owned(),
            kind,
            task: Some(task),
            content: mark.heading().unwrap_or_default(),
            mark,
            steer: false,
            boundary: false,
        },
    )?;
    cx.touched.agents.insert(caller.agent);
    diesel::update(
        message::table
            .filter(message::runtime.eq(runtime))
            .filter(message::key.eq(key)),
    )
    .set(message::carried.eq(true))
    .execute(cx.db)?;
    Ok(None)
}
