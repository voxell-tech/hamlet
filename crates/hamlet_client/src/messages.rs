//! The chief's requests and the server's answers.

use std::collections::HashMap;

use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use hamlet_protocol::v0;

use crate::chats::Chat;
use crate::outbox::{Outbox, RequestId};
use crate::realm::address_of;
use crate::waits::{Choice, Waiting};

/// Observers routing the chief's deeds into protocol requests.
pub fn plugin(app: &mut App) {
    app.init_resource::<Outbox>()
        .init_resource::<Rewinds>()
        .add_observer(
            |message: On<UserMessage>, mut outbox: ResMut<Outbox>| {
                let message = message.event();
                outbox.send(|id| {
                    v0::ClientMessage::Say(v0::Say {
                        id,
                        to: message.to.clone(),
                        text: message.text.clone(),
                    })
                });
            },
        )
        .add_observer(
            |answer: On<Answer>,
             waits: Query<&Waiting>,
             mut outbox: ResMut<Outbox>| {
                let answer = answer.event();
                if let Ok(wait) = waits.get(answer.wait) {
                    outbox.send(|id| {
                        v0::ClientMessage::Answer(v0::Answer {
                            id,
                            wait: wait.id.clone(),
                            choice: answer.choice.clone(),
                            text: answer.text.clone(),
                        })
                    });
                }
            },
        )
        .add_observer(|veto: On<Veto>, mut world: DeferredWorld| {
            let agent = match veto.event().agent {
                Some(entity) => {
                    let Some(address) = address_of(&world, entity)
                    else {
                        return;
                    };
                    Some(address)
                }
                None => None,
            };
            world.resource_mut::<Outbox>().send(|id| {
                v0::ClientMessage::Stop(v0::Stop { id, agent })
            });
        })
        .add_observer(
            |rewind: On<Rewind>,
             chats: Query<&Chat>,
             mut outbox: ResMut<Outbox>,
             mut rewinds: ResMut<Rewinds>| {
                let rewind = rewind.event();
                if let Ok(chat) = chats.get(rewind.chat) {
                    let id = outbox.send(|id| {
                        v0::ClientMessage::Rewind(v0::Rewind {
                            id,
                            chat: chat.0.clone(),
                            item: rewind.item.clone(),
                        })
                    });
                    rewinds.0.insert(id, rewind.chat);
                }
            },
        );
}

/// A chief's typed line addressed to an agent.
#[derive(Event, Clone, Debug)]
pub struct UserMessage {
    pub to: v0::Address,
    pub text: String,
}

/// A chief's answer to a received need.
#[derive(Event, Clone, Debug)]
pub struct Answer {
    pub wait: Entity,
    pub choice: Choice,
    pub text: Option<String>,
}

/// A stop request for one agent, or every agent when absent.
#[derive(Event, Clone, Copy, Debug)]
pub struct Veto {
    pub agent: Option<Entity>,
}

/// A request to take a chat back before an item.
#[derive(Event, Clone, Debug)]
pub struct Rewind {
    pub chat: Entity,
    pub item: String,
}

/// An operation received for a watched chat.
#[derive(Event, Clone, Debug)]
pub struct SessionOutput {
    pub chat: Entity,
    pub op: v0::Op,
}

/// The end of a watched chat's running work.
#[derive(Event, Clone, Copy, Debug)]
pub struct JobFinished {
    pub chat: Entity,
}

/// The removed chief words returned to a chat's input.
#[derive(Event, Clone, Debug)]
pub struct Rewound {
    pub chat: Entity,
    pub text: String,
}

/// A server answer paired with its request id.
#[derive(Event, Clone, Debug)]
pub struct Response {
    pub to: RequestId,
    pub message: v0::ServerMessage,
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

pub(crate) fn receive(
    world: &mut World,
    message: &v0::ServerMessage,
) {
    let to = match message {
        v0::ServerMessage::Reply(reply) => Some(&reply.to),
        v0::ServerMessage::Read(read) => Some(match read {
            v0::ReadResult::Prompt { to, .. }
            | v0::ReadResult::History { to, .. }
            | v0::ReadResult::Models { to, .. }
            | v0::ReadResult::Papers { to, .. }
            | v0::ReadResult::Charter { to, .. } => to,
        }),
        v0::ServerMessage::Login(login) => login.to.as_ref(),
        v0::ServerMessage::Transcript(transcript) => {
            transcript.to.as_ref()
        }
        v0::ServerMessage::Fault(fault) => {
            if fault.to.is_none() {
                world.trigger(ServerFault {
                    text: fault.text.clone(),
                });
            }
            fault.to.as_ref()
        }
        _ => None,
    };
    let Some(to) = to else {
        return;
    };
    if matches!(
        message,
        v0::ServerMessage::Reply(_) | v0::ServerMessage::Fault(_)
    ) {
        let chat = world.resource_mut::<Rewinds>().0.remove(to);
        if let (Some(chat), v0::ServerMessage::Reply(reply)) =
            (chat, message)
            && reply.kind == v0::ReplyKind::Rewound
        {
            world.trigger(Rewound {
                chat,
                text: reply.text.clone(),
            });
        }
    }
    world.trigger(Response {
        to: to.clone(),
        message: message.clone(),
    });
}
