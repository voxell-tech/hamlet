//! The chief's requests and the server's answers.

use std::collections::HashMap;

use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use hamlet_protocol::v0;

use crate::chats::Chat;
use crate::outbox::{Outbox, RequestId};
use crate::realm::address_of;
use crate::waits::{Choice, Refused, Waiting};

/// Observers routing the chief's deeds into protocol requests.
pub fn plugin(app: &mut App) {
    app.init_resource::<Outbox>()
        .init_resource::<Rewinds>()
        .init_resource::<Answers>()
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
            |answer: On<Answer>, mut world: DeferredWorld| {
                let answer = answer.event();
                send_answer(
                    &mut world,
                    answer.wait,
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
                    Choice::Reply,
                    None,
                    Some(answer.picks.clone()),
                );
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

/// A chief's answer to each question of a received need.
#[derive(EntityEvent, Clone, Debug)]
pub struct Answered {
    pub entity: Entity,
    pub picks: Vec<v0::Pick>,
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

/// The need each answer in flight answers.
#[derive(Resource, Default)]
struct Answers(HashMap<RequestId, Entity>);

/// Sends the chief's answer to `need`, for the server to check.
fn send_answer(
    world: &mut DeferredWorld,
    need: Entity,
    choice: Choice,
    text: Option<String>,
    picks: Option<Vec<v0::Pick>>,
) {
    let Some(waiting) =
        world.get::<Waiting>(need).map(|w| w.id.clone())
    else {
        return;
    };
    let request = world.resource_mut::<Outbox>().send(|id| {
        v0::ClientMessage::Answer(v0::Answer {
            id,
            wait: waiting,
            choice,
            text,
            picks,
        })
    });
    world.resource_mut::<Answers>().0.insert(request, need);
}

pub(crate) fn receive(
    world: &mut World,
    message: &v0::ServerMessage,
) {
    if let v0::ServerMessage::Fault(fault) = message
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
        let refusal = match message {
            v0::ServerMessage::Reply(reply)
                if reply.kind == v0::ReplyKind::Error =>
            {
                Some(reply.text.clone())
            }
            v0::ServerMessage::Fault(fault) => {
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
