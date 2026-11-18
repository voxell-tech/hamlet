//! Whether a final answer stands (C28).

use anyhow::Result;
use diesel::prelude::*;
use hamlet_protocol::client::Choice;
use hamlet_protocol::runtime::{self, Block, Yield};
use hamlet_store::schema::answer_fault;

use super::inputs;
use super::marks::Mark;
use crate::conversations::{Conversation, Kind};
use crate::realm::change::Change;
use crate::society::oversight::{self, Raise, WaitKind};

/// The words of an answer with no text and no call.
const EMPTY: &str =
    "the model answered with nothing. Send the message again";
/// The words of an answer cut off for room, even summarised.
const NO_ROOM: &str = "the model has no room left to answer, even \
                       with the conversation summarised. Choose a \
                       model that holds more";
const NO_SUMMARY: &str = "the conversation is longer than the model \
                          can hold, and the model could not \
                          summarise it. Compact it now, or choose a \
                          model that holds more";
/// The fewest output tokens an answer cut off for its own length has.
const ROOMY: u64 = 1024;

/// What the server says of a final answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Ruled {
    /// Summarise the conversation and answer again.
    pub compact: bool,
    /// The answer fails with these words.
    pub words: Option<String>,
    pub notice: Option<runtime::Mark>,
}

/// Rules on the final answer `yielded` gives in `conversation`.
pub(crate) fn rule(
    cx: &mut Change<'_>,
    conversation: &Conversation,
    yielded: &Yield,
) -> Result<Ruled> {
    let answer = &yielded.answer;
    let stands =
        answer.blocks.iter().flatten().any(|block| match block {
            Block::Call { .. } => true,
            Block::Text { text } => !text.trim().is_empty(),
            Block::Thinking { .. } => false,
        });
    if stands {
        return Ok(Ruled::default());
    }
    let input = match &yielded.input {
        Some(submission) => {
            inputs::by_submission(cx.db, conversation.id, submission)?
        }
        None => None,
    };
    let reflection = matches!(
        input.as_ref().map(inputs::Input::mark).transpose()?,
        Some(Mark::Reflection { .. })
    );
    let tasked =
        input.as_ref().and_then(|input| input.task).is_some();
    let cutoff = answer.stop.as_deref() == Some("length")
        && answer
            .usage
            .as_ref()
            .is_some_and(|usage| usage.output < ROOMY)
        && !reflection
        && tasked;
    let history = yielded.history == Some(true);
    let first = yielded.retried != Some(true);
    if cutoff && history && first && yielded.summary_failure.is_none()
    {
        return Ok(Ruled {
            compact: true,
            ..Ruled::default()
        });
    }
    let words = if let Some(error) = &yielded.summary_failure {
        match error.strip_prefix("Summarization failed: ") {
            Some(detail) => format!("provider error: {detail}"),
            None => NO_SUMMARY.into(),
        }
    } else if cutoff && !history && first {
        "the message is longer than the model can hold".into()
    } else if cutoff {
        NO_ROOM.into()
    } else {
        EMPTY.into()
    };
    if let Some(submission) = &yielded.input {
        let kept = diesel::insert_into(answer_fault::table)
            .values((
                answer_fault::conversation.eq(conversation.id),
                answer_fault::input.eq(submission),
                answer_fault::words.eq(&words),
                answer_fault::reported
                    .eq(conversation.kind == Kind::Chat),
            ))
            .on_conflict_do_nothing()
            .execute(cx.db)?;
        if kept == 1 && conversation.kind == Kind::Chat {
            oversight::raise(
                cx,
                Raise {
                    kind: WaitKind::Fault,
                    danger: None,
                    agent: conversation.agent,
                    scholar: None,
                    conversation: Some(conversation.id),
                    task: input.as_ref().and_then(|input| input.task),
                    about: words.clone(),
                    change: None,
                    answers: vec![Choice::Dismiss],
                    questions: None,
                    setup: None,
                },
            )?;
        }
    }
    Ok(Ruled {
        compact: false,
        notice: Some(
            Mark::Notice {
                text: words.clone(),
                at: cx.at,
            }
            .wire(),
        ),
        words: Some(words),
    })
}
