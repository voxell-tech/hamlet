//! The chief's requests and the server's answers.

use std::collections::HashMap;

use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use hamlet_protocol::client as protocol;

use crate::chats::Chat;
use crate::outbox::{Outbox, RequestId};
use crate::realm::id_of;
use crate::waits::{Choice, Refused, Waiting};

/// Observers routing the chief's deeds into protocol requests.
pub fn plugin(app: &mut App) {
    app.init_resource::<Outbox>()
        .init_resource::<Rewinds>()
        .init_resource::<Answers>()
        .add_observer(
            |message: On<ChiefMessage>,
             mut outbox: ResMut<Outbox>| {
                let message = message.event();
                outbox.push(protocol::ClientMessage::Say(
                    protocol::Say {
                        files: message.files.clone(),
                        id: message.request.clone(),
                        images: message.images.clone(),
                        to: message.to,
                        text: message.text.clone(),
                    },
                ));
            },
        )
        .add_observer(
            |answer: On<Answer>, mut world: DeferredWorld| {
                let answer = answer.event();
                send_answer(
                    &mut world,
                    answer.wait,
                    answer.request.clone(),
                    answer.choice.clone(),
                    answer.text.clone(),
                    None,
                );
            },
        )
        .add_observer(
            |answer: On<Answered>, mut world: DeferredWorld| {
                let answer = answer.event();
                send_answer(
                    &mut world,
                    answer.entity,
                    answer.request.clone(),
                    Choice::Reply,
                    None,
                    Some(answer.picks.clone()),
                );
            },
        )
        .add_observer(|veto: On<Veto>, mut world: DeferredWorld| {
            let veto = veto.event();
            let agent = match veto.agent {
                Some(entity) => {
                    let Some(id) = id_of(&world, entity) else {
                        return;
                    };
                    Some(id)
                }
                None => None,
            };
            world.resource_mut::<Outbox>().push(
                protocol::ClientMessage::Stop(protocol::Stop {
                    id: veto.request.clone(),
                    agent,
                }),
            );
        })
        .add_observer(
            |resume: On<Resume>, mut world: DeferredWorld| {
                let resume = resume.event();
                let Some(agent) = id_of(&world, resume.agent) else {
                    return;
                };
                world.resource_mut::<Outbox>().push(
                    protocol::ClientMessage::Resume(
                        protocol::Resume {
                            id: resume.request.clone(),
                            agent: Some(agent),
                        },
                    ),
                );
            },
        )
        .add_observer(
            |rewind: On<Rewind>,
             chats: Query<&Chat>,
             mut outbox: ResMut<Outbox>,
             mut rewinds: ResMut<Rewinds>| {
                let rewind = rewind.event();
                if let Ok(chat) = chats.get(rewind.chat) {
                    outbox.push(protocol::ClientMessage::Rewind(
                        protocol::Rewind {
                            id: rewind.request.clone(),
                            chat: chat.0.clone(),
                            item: rewind.item.clone(),
                        },
                    ));
                    rewinds
                        .0
                        .insert(rewind.request.clone(), rewind.chat);
                }
            },
        )
        .add_observer(
            |compact: On<Compact>, mut outbox: ResMut<Outbox>| {
                let compact = compact.event();
                outbox.push(protocol::ClientMessage::Compact(
                    protocol::Compact {
                        id: compact.request.clone(),
                        to: compact.to,
                    },
                ));
            },
        )
        .add_observer(
            |withdraw: On<Withdraw>,
             chats: Query<&Chat>,
             mut outbox: ResMut<Outbox>| {
                let withdraw = withdraw.event();
                if let Ok(chat) = chats.get(withdraw.chat) {
                    outbox.push(protocol::ClientMessage::Withdraw(
                        protocol::Withdraw {
                            id: withdraw.request.clone(),
                            chat: chat.0.clone(),
                            item: withdraw.item.clone(),
                        },
                    ));
                }
            },
        );
}

/// A chief's typed line addressed to an agent.
#[derive(Event, Clone, Debug)]
pub struct ChiefMessage {
    /// The id of the `say` request, which its reply is to.
    pub request: RequestId,
    pub to: protocol::AgentId,
    pub text: String,
    /// The images the line carries, already uploaded (C33).
    pub images: Vec<protocol::AttachedImage>,
    /// The files the line carries, already uploaded (C33).
    pub files: Vec<protocol::AttachedFile>,
}

impl ChiefMessage {
    /// A line of words alone.
    pub fn words(to: protocol::AgentId, text: String) -> Self {
        ChiefMessage {
            request: RequestId::new(),
            to,
            text,
            images: Vec::new(),
            files: Vec::new(),
        }
    }
}

/// A chief's answer to a received need.
#[derive(EntityEvent, Clone, Debug)]
pub struct Answer {
    #[event_target]
    pub wait: Entity,
    pub request: RequestId,
    pub choice: Choice,
    pub text: Option<String>,
}

/// A chief's answer to each question of a received need.
#[derive(EntityEvent, Clone, Debug)]
pub struct Answered {
    pub entity: Entity,
    pub request: RequestId,
    pub picks: Vec<protocol::Pick>,
}

/// A stop request for one agent, or every agent when absent.
#[derive(Event, Clone, Debug)]
pub struct Veto {
    pub request: RequestId,
    pub agent: Option<Entity>,
}

/// A request that a stopped agent take what waits for it again.
#[derive(Event, Clone, Debug)]
pub struct Resume {
    pub request: RequestId,
    pub agent: Entity,
}

/// A request to take a chat back before an item.
#[derive(EntityEvent, Clone, Debug)]
pub struct Rewind {
    #[event_target]
    pub chat: Entity,
    pub request: RequestId,
    pub item: String,
}

/// A request to summarize the earlier part of an agent's
/// conversation now.
#[derive(Event, Clone, Debug)]
pub struct Compact {
    pub request: RequestId,
    pub to: protocol::AgentId,
}

/// A request to take back the chief's queued line `item` of a chat.
#[derive(EntityEvent, Clone, Debug)]
pub struct Withdraw {
    #[event_target]
    pub chat: Entity,
    pub request: RequestId,
    pub item: String,
}

/// An operation received for a watched chat.
#[derive(EntityEvent, Clone, Debug)]
pub struct ChatOutput {
    #[event_target]
    pub chat: Entity,
    pub operation: protocol::TranscriptOperation,
}

/// The end of a watched chat's running work.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct JobFinished {
    #[event_target]
    pub chat: Entity,
}

/// The removed chief words and images returned to a chat's input.
#[derive(EntityEvent, Clone, Debug)]
pub struct Rewound {
    #[event_target]
    pub chat: Entity,
    pub text: String,
    pub images: Vec<protocol::AttachedImage>,
    pub files: Vec<protocol::AttachedFile>,
}

/// A server answer paired with its request id.
#[derive(Event, Clone, Debug)]
pub struct Response {
    pub to: RequestId,
    pub message: protocol::ServerMessage,
}

/// An unaddressed server failure for the chief.
#[derive(Event, Clone, Debug)]
pub struct ServerFault {
    pub text: String,
}

/// A connection restored after its loss.
#[derive(Event, Clone, Copy, Debug)]
pub struct Reconnected;

#[derive(Resource, Default)]
struct Rewinds(HashMap<RequestId, Entity>);

/// The need each answer in flight answers.
#[derive(Resource, Default)]
struct Answers(HashMap<RequestId, Entity>);

/// Sends the chief's answer to `need`, for the server to check.
fn send_answer(
    world: &mut DeferredWorld,
    need: Entity,
    request: RequestId,
    choice: Choice,
    text: Option<String>,
    picks: Option<Vec<protocol::Pick>>,
) {
    let Some(wait) = world.get::<Waiting>(need).map(|w| w.id.clone())
    else {
        return;
    };
    world
        .resource_mut::<Answers>()
        .0
        .insert(request.clone(), need);
    world.resource_mut::<Outbox>().push(
        protocol::ClientMessage::Answer(protocol::Answer {
            id: request,
            wait,
            choice,
            text,
            picks,
        }),
    );
}

pub(crate) fn receive(
    world: &mut World,
    message: &protocol::ServerMessage,
) {
    if let protocol::ServerMessage::Fault(fault) = message
        && fault.to.is_none()
    {
        world.trigger(ServerFault {
            text: fault.text.clone(),
        });
    }
    let Some(to) = message.to() else {
        return;
    };
    if matches!(
        message,
        protocol::ServerMessage::Reply(_)
            | protocol::ServerMessage::Fault(_)
    ) {
        let chat = world.resource_mut::<Rewinds>().0.remove(to);
        if let (Some(chat), protocol::ServerMessage::Reply(reply)) =
            (chat, message)
            && reply.kind == protocol::ReplyKind::Rewound
        {
            world.trigger(Rewound {
                chat,
                text: reply.text.clone(),
                images: reply.images.clone(),
                files: reply.files.clone(),
            });
        }
        let refusal = match message {
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                Some(reply.text.clone())
            }
            protocol::ServerMessage::Fault(fault) => {
                Some(fault.text.clone())
            }
            _ => None,
        };
        let need = world.resource_mut::<Answers>().0.remove(to);
        if let Some(need) = need
            && let Ok(mut need) = world.get_entity_mut(need)
        {
            match refusal {
                Some(text) => need.insert(Refused(text)),
                None => need.remove::<Refused>(),
            };
        }
    }
    world.trigger(Response {
        to: to.to_owned(),
        message: message.clone(),
    });
}
