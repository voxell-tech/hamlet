//! Letters and questions between agents along roads (H30, H31): each
//! receiver's conversation with each sender, the message kept once
//! per tool call, and a question's answer carried back to its asker.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ConversationId, TaskId};
use hamlet_store::schema::{input, message, tool_call};

use super::conversations::{self, Begin, Kind};
use super::inputs::{self, InputKind, Keep};
use super::marks::{From, Mark};
use super::{Env, items, models, runtimes};
use crate::change::Change;
use crate::society::{addresses, agents};
use crate::text::text_enum;

text_enum! {
    pub(crate) enum MessageKind {
        Letter = "letter",
        Question = "question",
    }
}

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
    pub answer: Option<String>,
    pub failure: Option<String>,
    pub carried: bool,
    pub replied: bool,
    pub refused: bool,
}

/// The conversation `agent` keeps with `peer` for messages of `kind`,
/// begun with the first.
pub(crate) fn with(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    kind: Kind,
    peer: AgentId,
) -> Result<ConversationId> {
    if let Some(id) =
        conversations::with_peer(cx.db, agent, kind, peer)?
    {
        return Ok(id);
    }
    let harness = agents::get(cx.db, agent)?.harness;
    let runtime = runtimes::of_harness(cx.db, &harness)?;
    let choice =
        models::choose(cx.db, &env.settings, &env.overrides, agent)?;
    let mut opening = Begin::new(agent, kind);
    opening.peer = Some(peer);
    conversations::begin(cx, opening, &runtime, &choice.model, None)
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
}

/// Keeps `send` once per call: its row and its input to the receiver.
pub(crate) fn send(
    cx: &mut Change<'_>,
    env: &Env,
    send: Send<'_>,
) -> Result<Message> {
    if let Some(found) = find(cx.db, send.runtime, send.key)? {
        return Ok(found);
    }
    let kind = match send.kind {
        MessageKind::Letter => Kind::Letters,
        MessageKind::Question => Kind::Questions,
    };
    let to = with(cx, env, send.receiver, kind, send.sender)?;
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
        at: cx.at,
    };
    let (input_kind, mark) = match send.kind {
        MessageKind::Letter => {
            (InputKind::Letter, Mark::Letter(from))
        }
        MessageKind::Question => {
            (InputKind::Question, Mark::Question(from))
        }
    };
    inputs::keep(
        cx,
        Keep {
            conversation: to,
            request: request.clone(),
            kind: input_kind,
            task: Some(send.task),
            content: send.text.to_owned(),
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

/// Whether `asker` asking `receiver` would close a loop of questions
/// that wait on each other.
pub(crate) fn would_loop(
    db: &mut Db,
    asker: AgentId,
    receiver: AgentId,
) -> Result<bool> {
    let open = message::table
        .filter(message::kind.eq(MessageKind::Question))
        .filter(message::answer.is_null())
        .filter(message::failure.is_null())
        .select((message::sender, message::receiver))
        .load::<(AgentId, AgentId)>(db)?;
    let mut seen = vec![receiver];
    let mut next = vec![receiver];
    while let Some(at) = next.pop() {
        if at == asker {
            return Ok(true);
        }
        for (_, waited) in
            open.iter().filter(|(waiting, _)| *waiting == at)
        {
            if !seen.contains(waited) {
                seen.push(*waited);
                next.push(*waited);
            }
        }
    }
    Ok(false)
}

/// Keeps the answer to the question whose input `request` of
/// `conversation` ended, once. Gives the asker's call to wake while
/// it still waits; an asker whose turn ended gets the answer as a
/// reply.
pub(crate) fn answered(
    cx: &mut Change<'_>,
    conversation: ConversationId,
    request: &str,
    failed: Option<&str>,
) -> Result<Option<(String, String)>> {
    let found = message::table
        .filter(message::to_conversation.eq(conversation))
        .filter(message::input.eq(request))
        .filter(message::kind.eq(MessageKind::Question))
        .select(Message::as_select())
        .first(cx.db)
        .optional()?;
    let Some(found) = found else {
        return Ok(None);
    };
    if found.answer.is_some() || found.failure.is_some() {
        return Ok(None);
    }
    let answer = items::answer_of(cx.db, conversation, request)?;
    let failure = match (&answer, failed) {
        (Some(_), None) => None,
        (_, Some(words)) => Some(words.to_owned()),
        (None, None) => Some("it gave no answer".to_owned()),
    };
    diesel::update(message::table.find(found.id))
        .set((
            message::answer.eq(&answer),
            message::failure.eq(&failure),
        ))
        .execute(cx.db)?;
    cx.touched.agents.insert(found.sender);
    let words = match (&answer, &failure) {
        (Some(answer), None) => answer.clone(),
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
            request: &format!("reply:{}", found.id),
            words: &words,
            waits: true,
            author: None,
        },
    )
}

/// Words for the caller of a tool call, from the agent it reached.
pub(crate) struct Carry<'a> {
    pub runtime: &'a str,
    pub key: &'a str,
    pub from: AgentId,
    pub task: TaskId,
    /// The id of the reply, when it is kept as one.
    pub request: &'a str,
    pub words: &'a str,
    /// Whether the call waits for the words.
    pub waits: bool,
    /// Who a reply is shown as from, when not `from`'s name.
    pub author: Option<&'a str>,
}

/// Gives the words of `carry` to its call: the call to wake while it
/// waits in a turn that goes on, else a reply kept for the caller's
/// conversation.
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
        waits,
        author,
    } = carry;
    let call = tool_call::table
        .find((runtime, key))
        .select((tool_call::conversation, tool_call::input))
        .first::<(ConversationId, Option<String>)>(cx.db)
        .optional()?;
    let Some((asker, submission)) = call else {
        return Ok(None);
    };
    let waiting = waits
        && match &submission {
            Some(submission) => input::table
                .filter(input::conversation.eq(asker))
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
    inputs::keep(
        cx,
        Keep {
            conversation: asker,
            request: request.to_owned(),
            kind: InputKind::Reply,
            task: Some(task),
            content: format!("{author} answers: {words}"),
            mark: Mark::Reply(From {
                from,
                author,
                at: cx.at,
            }),
            steer: false,
            boundary: false,
        },
    )?;
    diesel::update(
        message::table
            .filter(message::runtime.eq(runtime))
            .filter(message::key.eq(key)),
    )
    .set(message::carried.eq(true))
    .execute(cx.db)?;
    Ok(None)
}
