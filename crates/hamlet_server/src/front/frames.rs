//! The actors' answers as the client protocol shows them: a page of
//! a chat as a `Transcript`, an event as a `Turn`, and each answer to
//! a client's request as its frame.

use anyhow::{Result, bail};
use hamlet_messages::agent::chat::{
    Began, ChatEvent, ChatFile, ChatImage, ChatItem, ChatOp,
    ChatPage, ChatRef, ChatTool, Checked, History, LineMark,
    ModelList, Prompt, ResumeAnswer, RewindAnswer, ToolChange, Usage,
};
use hamlet_messages::realm::documents::CharterRead;
use hamlet_protocol::client::{
    self as protocol, AgentId, AttachedFile, AttachedImage, Listed,
    ListedPriced, ReadResult, RequestId, ServerMessage, Transcript,
    TranscriptOperation, Turn,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::view::Own;
use super::words;
use crate::clients::check::reply;
use crate::society::refusal::Refusal;

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

/// The answer to request `id` that `answer` gives: the refusal it
/// carries, in its own words, or the frame `done` makes of it.
fn worded<T: DeserializeOwned>(
    id: &RequestId,
    answer: &[u8],
    done: impl FnOnce(T) -> Result<ServerMessage>,
) -> Result<ServerMessage> {
    match refusal_words(answer)? {
        Some(words) => Ok(reply(id, Err(Refusal::Usage { words }))),
        None => done(ciborium::from_reader(answer)?),
    }
}

fn notice(id: &RequestId, words: String) -> ServerMessage {
    reply(id, Ok::<_, Refusal>(words))
}

pub(crate) fn began(
    id: &RequestId,
    answer: &[u8],
    _: &Own,
) -> Result<ServerMessage> {
    worded(id, answer, |Began { number }| {
        Ok(notice(id, words::began(number)))
    })
}

pub(crate) fn back(
    id: &RequestId,
    answer: &[u8],
    _: &Own,
) -> Result<ServerMessage> {
    worded(id, answer, |back| match back {
        ResumeAnswer::Back { number } => {
            Ok(notice(id, words::back(number)))
        }
        _ => bail!("a resume that is no refusal is back"),
    })
}

pub(crate) fn carrying_on(
    id: &RequestId,
    answer: &[u8],
    _: &Own,
) -> Result<ServerMessage> {
    worded(id, answer, |serde::de::IgnoredAny| {
        Ok(notice(id, words::CARRYING_ON.to_owned()))
    })
}

pub(crate) fn rewound(
    id: &RequestId,
    answer: &[u8],
    _: &Own,
) -> Result<ServerMessage> {
    worded(id, answer, |rewound| match rewound {
        RewindAnswer::Rewound {
            text,
            images,
            files,
        } => Ok(ServerMessage::Reply(protocol::Reply {
            file: None,
            files: files
                .into_iter()
                .map(file)
                .collect::<Result<_>>()?,
            image: None,
            images: images
                .into_iter()
                .map(image)
                .collect::<Result<_>>()?,
            kind: protocol::ReplyKind::Rewound,
            text,
            to: id.clone(),
            upload: None,
        })),
        _ => bail!("a rewind that is no refusal is rewound"),
    })
}

/// A check of a model: its hello is the reply's words, and its
/// failure or refusal the error's.
pub(crate) fn checked(
    id: &RequestId,
    answer: &[u8],
    _: &Own,
) -> Result<ServerMessage> {
    Ok(match ciborium::from_reader(answer)? {
        Checked::Answered { words } => notice(id, words),
        Checked::Failed { words } | Checked::Refused { words } => {
            reply(id, Err(Refusal::Usage { words }))
        }
    })
}

/// The agent's prompt, which it gives whole, as one section.
pub(crate) fn prompt(
    id: &RequestId,
    answer: &[u8],
    _: &Own,
) -> Result<ServerMessage> {
    worded(id, answer, |Prompt { text }| {
        Ok(ServerMessage::Read(ReadResult::Prompt {
            prompt: vec![protocol::PromptSection {
                name: "prompt".to_owned(),
                text,
            }],
            to: id.clone(),
        }))
    })
}

/// The sessions of the chief's chat, each begun by this server's
/// chief.
pub(crate) fn history(
    id: &RequestId,
    answer: &[u8],
    own: &Own,
) -> Result<ServerMessage> {
    worded(id, answer, |History { sessions }| {
        let history = sessions
            .into_iter()
            .map(|session| protocol::Past {
                began: i64::from(session.began) * 1000,
                by: own.chief.clone(),
                current: session.current,
                messages: i64::from(session.messages),
                modified: i64::from(session.modified) * 1000,
                number: i64::from(session.number),
                title: session.title,
                tokens: session.tokens.0,
            })
            .collect();
        Ok(ServerMessage::Read(ReadResult::History {
            history,
            to: id.clone(),
        }))
    })
}

/// The models the Hand's listing holds, with what it does not know
/// of them left unknown.
pub(crate) fn models(
    id: &RequestId,
    answer: &[u8],
    _: &Own,
) -> Result<ServerMessage> {
    worded(id, answer, |ModelList { models }| {
        let models = models
            .into_iter()
            .map(|model| Listed {
                context: Some(i64::from(model.context)),
                default_effort: None,
                efforts: Vec::new(),
                fast: false,
                images: None,
                name: model.name,
                note: String::new(),
                priced: if model.price.is_some() {
                    ListedPriced::Listed
                } else {
                    ListedPriced::Unknown
                },
                price: model.price.map(|price| protocol::Price {
                    input: price.input,
                    output: price.output,
                }),
                title: model.title,
            })
            .collect();
        Ok(ServerMessage::Read(ReadResult::Models {
            models,
            to: id.clone(),
        }))
    })
}

pub(crate) fn charter(
    id: &RequestId,
    answer: &[u8],
    _: &Own,
) -> Result<ServerMessage> {
    worded(id, answer, |CharterRead { own, text }| {
        Ok(ServerMessage::Read(ReadResult::Charter {
            charter: protocol::Charter { own, text },
            to: id.clone(),
        }))
    })
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

#[cfg(test)]
mod tests {
    use hamlet_messages::agent::chat::{
        Count, ListedModel, ResumeRefusal, Session,
    };
    use hamlet_messages::agent::copies::Price;
    use hamlet_protocol::client::{Confinement, ReplyKind};
    use serde::Serialize;

    use super::*;

    fn own() -> Own {
        Own {
            chief: "Ada".to_owned(),
            confinement: Confinement::Sandbox {
                name: "bubblewrap".to_owned(),
            },
            realm: "/realm".to_owned(),
            epoch: 0,
        }
    }

    fn bytes(answer: &impl Serialize) -> Vec<u8> {
        let mut bytes = Vec::new();
        ciborium::into_writer(answer, &mut bytes).unwrap();
        bytes
    }

    fn words(message: ServerMessage) -> (ReplyKind, String) {
        let ServerMessage::Reply(reply) = message else {
            panic!("a reply: {message:?}");
        };
        (reply.kind, reply.text)
    }

    #[test]
    fn a_refusal_is_answered_in_its_own_words() {
        let refused = ResumeAnswer::Refused {
            why: ResumeRefusal::NoSuchSession { number: 7 },
        };
        let answer =
            back(&RequestId::new(), &bytes(&refused), &own())
                .unwrap();
        assert_eq!(
            words(answer),
            (
                ReplyKind::Error,
                "there is no past session 7 here".to_owned()
            )
        );
        let answer = back(
            &RequestId::new(),
            &bytes(&ResumeAnswer::Back { number: 3 }),
            &own(),
        )
        .unwrap();
        assert_eq!(words(answer).1, "Back in session 3.");
    }

    #[test]
    fn the_history_is_in_milliseconds_and_begun_by_the_chief() {
        let history = History {
            sessions: vec![Session {
                number: 2,
                began: 1_700_000_000,
                modified: 1_700_000_060,
                current: true,
                messages: 4,
                tokens: Count(5_000_000_000),
                title: "Plans".to_owned(),
            }],
        };
        let answer = super::history(
            &RequestId::new(),
            &bytes(&history),
            &own(),
        )
        .unwrap();
        let ServerMessage::Read(ReadResult::History {
            history, ..
        }) = answer
        else {
            panic!("a read of the history");
        };
        let past = &history[0];
        assert_eq!(past.began, 1_700_000_000_000);
        assert_eq!(past.modified, 1_700_000_060_000);
        assert_eq!(past.by, "Ada");
        assert_eq!(past.tokens, 5_000_000_000);
    }

    #[test]
    fn a_model_with_no_price_is_priced_unknown() {
        let list = ModelList {
            models: vec![
                ListedModel {
                    name: "faux::a".to_owned(),
                    title: "A".to_owned(),
                    context: 32_000,
                    price: Some(Price {
                        input: 1.0,
                        output: 2.0,
                        cache_read: 0.1,
                        cache_write: 1.25,
                    }),
                },
                ListedModel {
                    name: "faux::b".to_owned(),
                    title: "B".to_owned(),
                    context: 8_000,
                    price: None,
                },
            ],
        };
        let answer =
            models(&RequestId::new(), &bytes(&list), &own()).unwrap();
        let ServerMessage::Read(ReadResult::Models {
            models, ..
        }) = answer
        else {
            panic!("a read of the models");
        };
        assert_eq!(models[0].priced, ListedPriced::Listed);
        assert_eq!(
            models[0].price,
            Some(protocol::Price {
                input: 1.0,
                output: 2.0
            })
        );
        assert_eq!(models[1].priced, ListedPriced::Unknown);
        assert_eq!(models[1].context, Some(8_000));
    }

    #[test]
    fn a_failed_check_is_an_error_with_its_words() {
        let failed = Checked::Failed {
            words: "The key was refused.".to_owned(),
        };
        let answer =
            checked(&RequestId::new(), &bytes(&failed), &own())
                .unwrap();
        assert_eq!(
            words(answer),
            (ReplyKind::Error, "The key was refused.".to_owned())
        );
    }
}
