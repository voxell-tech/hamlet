//! The agent's chat, as the client protocol shows it: a page as a
//! `Transcript`, an event as a `Turn`.

use anyhow::Result;
use hamlet_messages::agent::chat::{
    ChatEvent, ChatFile, ChatImage, ChatItem, ChatOp, ChatPage,
    ChatRef, ChatTool, LineMark, ToolChange, Usage,
};
use hamlet_protocol::client::{
    self as protocol, AgentId, AttachedFile, AttachedImage,
    RequestId, Transcript, TranscriptOperation, Turn,
};
use serde::Deserialize;

/// `page` of `chat`, answering the request `to`.
pub(crate) fn transcript(
    page: ChatPage,
    chat: protocol::ChatRef,
    to: Option<RequestId>,
) -> Result<Transcript> {
    Ok(Transcript {
        number: chat
            .from
            .is_none()
            .then_some(page.session)
            .flatten()
            .map(i64::from),
        chat,
        earlier: page.earlier,
        items: page
            .items
            .into_iter()
            .map(item)
            .collect::<Result<_>>()?,
        last: None,
        pending: u64::from(page.pending),
        to,
        usage: usage(page.usage),
        working: page.working,
    })
}

/// `event` as a turn of the watched chat `chat`, or none when the
/// event is of another chat.
pub(crate) fn turn(
    event: ChatEvent,
    chat: &protocol::ChatRef,
) -> Result<Option<Turn>> {
    if chat_ref(&event.chat) != *chat {
        return Ok(None);
    }
    Ok(Some(Turn {
        chat: chat.clone(),
        number: chat.from.is_none().then_some(chat.past).flatten(),
        operations: event
            .ops
            .into_iter()
            .map(operation)
            .collect::<Result<_>>()?,
    }))
}

/// The words of an answer that is a refusal, which carries them
/// beside its reason; none for any other answer.
pub(crate) fn refusal_words(answer: &[u8]) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct Worded {
        words: Option<String>,
    }
    Ok(ciborium::from_reader::<Worded, _>(answer)?.words)
}

fn chat_ref(chat: &ChatRef) -> protocol::ChatRef {
    protocol::ChatRef {
        from: chat.from.map(|from| AgentId(u64::from(from.0))),
        kind: chat.kind.clone(),
        past: chat.past.map(i64::from),
        with: AgentId(u64::from(chat.with.0)),
    }
}

fn operation(op: ChatOp) -> Result<TranscriptOperation> {
    Ok(match op {
        ChatOp::Add {
            item: added,
            replaces,
        } => TranscriptOperation::Add {
            item: item(*added)?,
            replaces,
        },
        ChatOp::Append { item, text, .. } => {
            TranscriptOperation::Append { item, text }
        }
        ChatOp::Status { working, pending } => {
            TranscriptOperation::Status {
                last: None,
                pending: u64::from(pending),
                working,
            }
        }
        ChatOp::Usage { usage: used } => {
            TranscriptOperation::Usage { usage: usage(used) }
        }
        ChatOp::Reset => TranscriptOperation::Reset {},
    })
}

fn item(item: ChatItem) -> Result<protocol::Item> {
    Ok(protocol::Item {
        at: u64::from(item.at) * 1000,
        author: item.author,
        chief: None,
        files: item
            .files
            .into_iter()
            .map(file)
            .collect::<Result<_>>()?,
        id: item.id,
        images: item
            .images
            .into_iter()
            .map(image)
            .collect::<Result<_>>()?,
        kind: item.kind,
        rewindable: item.rewindable,
        text: item.text,
        tool: item.tool.map(tool),
    })
}

fn image(image: ChatImage) -> Result<AttachedImage> {
    Ok(AttachedImage {
        at: image.at.map(u64::from),
        id: image.id.to_string().parse()?,
    })
}

fn file(file: ChatFile) -> Result<AttachedFile> {
    Ok(AttachedFile {
        at: file.at.map(u64::from),
        id: file.id.to_string().parse()?,
        name: file.name,
        size: u64::from(file.size),
    })
}

fn tool(tool: ChatTool) -> protocol::ItemTool {
    protocol::ItemTool {
        change: tool.change.map(change),
        // The agent writes a negative exit code as its 32 bits.
        exit: tool.exit.map(|exit| i64::from(exit as i32)),
        failure: tool.failure,
        ms: tool.ms.map(u64::from),
        name: tool.name,
        summary: tool.summary,
    }
}

fn change(change: ToolChange) -> protocol::Change {
    protocol::Change {
        added: u64::from(change.added),
        count: change.count,
        created: change.created,
        lines: change
            .lines
            .into_iter()
            .map(|line| protocol::ChangeLines {
                mark: match line.mark {
                    LineMark::Same => {
                        protocol::ChangeLinesMark::Space
                    }
                    LineMark::Added => {
                        protocol::ChangeLinesMark::Plus
                    }
                    LineMark::Removed => {
                        protocol::ChangeLinesMark::Minus
                    }
                },
                text: line.text,
            })
            .collect(),
        path: change.path,
        removed: u64::from(change.removed),
        total: u64::from(change.total),
    }
}

fn usage(usage: Usage) -> protocol::UsageView {
    protocol::UsageView {
        cached: usage.cached.0,
        calls: usage.calls.0,
        input: usage.input.0,
        last_input: usage.last_input.0,
        micros: usage.micros.0,
        ms: usage.ms.0,
        output: usage.output.0,
        reasoning: usage.reasoning.0,
        summary: usage.summary,
        unreported: usage.unreported.0,
    }
}
