//! A client's requests, one system for each kind.

use bevy_app::{App, PreUpdate};
use bevy_ecs::error::Result;
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::system::{Commands, ResMut};
use hamlet_protocol::client::{
    self as protocol, ClientMessage, RequestId, ServerMessage,
};
use tokio::net::TcpStream;

use super::check::{fault, reply};
use super::commands::ChiefCommand;
use super::connection::Event;
use super::{Clients, Connections};
use crate::app::input::{AddHandler, InputOrigin};
use crate::core::Core;
use crate::society::rejection::Rejection;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<ClientEvent>()
        .add_message::<ClientConnected>()
        .add_systems(
            PreUpdate,
            (
                take_in_client_event
                    .run_if(on_message::<ClientEvent>),
                connect.run_if(on_message::<ClientConnected>),
            ),
        )
        .add_handler::<FromClient<protocol::Hello>, _>(hello)
        .add_handler::<FromClient<protocol::Open>, _>(open)
        .add_handler::<FromClient<protocol::Close>, _>(close)
        .add_handler::<FromClient<protocol::Compact>, _>(compact)
        .add_handler::<FromClient<protocol::Withdraw>, _>(withdraw)
        .add_handler::<FromClient<protocol::Resume>, _>(resume)
        .add_handler::<FromClient<protocol::ImportProject>, _>(
            import_project,
        )
        .add_handler::<FromClient<protocol::Retry>, _>(retry)
        .add_handler::<FromClient<protocol::Stop>, _>(stop)
        .add_handler::<FromClient<protocol::Edit>, _>(edit)
        .add_handler::<FromClient<protocol::Answer>, _>(answer)
        .add_handler::<FromClient<protocol::PutImage>, _>(put_image)
        .add_handler::<FromClient<protocol::PutImagePart>, _>(
            put_image_part,
        )
        .add_handler::<FromClient<protocol::PutFile>, _>(put_file)
        .add_handler::<FromClient<protocol::PutFilePart>, _>(
            put_file_part,
        )
        .add_handler::<FromClient<protocol::Rewind>, _>(rewind)
        .add_handler::<FromClient<protocol::Say>, _>(say)
        .add_handler::<FromClient<ReadModels>, _>(read_models)
        .add_handler::<FromClient<protocol::Read>, _>(read)
        .add_handler::<FromClient<protocol::MoveWorktrees>, _>(
            move_worktrees,
        )
        .add_handler::<FromClient<protocol::Login>, _>(login)
        .add_handler::<FromClient<protocol::Check>, _>(check)
        .add_handler::<FromClient<protocol::Mcp>, _>(mcp);
}

/// An event of a client's connection.
#[derive(Message)]
pub(crate) struct ClientEvent(pub Event);

/// A client connected.
#[derive(Message)]
pub(crate) struct ClientConnected(pub TcpStream);

/// A request of the kind `M` from the client `client`.
#[derive(Message)]
pub(crate) struct FromClient<M: Send + Sync + 'static> {
    pub client: u64,
    pub request: M,
}

/// A read of the model catalogue, fetched again when `refresh`.
pub(crate) struct ReadModels {
    id: RequestId,
    refresh: Option<bool>,
}

/// Takes in what each connection did.
fn take_in_client_event(
    mut events: ResMut<Messages<ClientEvent>>,
    mut core: Core,
    mut origin: ResMut<InputOrigin>,
    mut commands: Commands,
) -> Result {
    events.drain().try_for_each(|ClientEvent(event)| {
        let (client, text) = match event {
            Event::Late(client) => {
                core.late(client);
                return Ok(());
            }
            Event::Gone(client) => {
                // Its hooks and observers take what the client held,
                // and its uploads go with it.
                core.terminals.gone(client);
                if let Some(gone) = core.connections.entity(client) {
                    commands.entity(gone).despawn();
                }
                return Ok(());
            }
            // Its place among the connection's waiting messages frees
            // once its request is written.
            Event::Message(client, text, _place) => (client, text),
        };
        let Some(message) = core.checked(client, &text)? else {
            return Ok(());
        };
        *origin = InputOrigin::Client {
            client,
            request: message.id().cloned(),
        };
        crate::app::fault::panic_at("request");
        write(&mut commands, client, message);
        Ok(())
    })
}

/// A connection is an entity until it goes.
fn connect(
    mut connected: ResMut<Messages<ClientConnected>>,
    mut clients: ResMut<Clients>,
    mut commands: Commands,
) {
    for ClientConnected(stream) in connected.drain() {
        commands.spawn(clients.accept(stream));
    }
}

/// Writes `message` from `client` as a [`FromClient`] of its kind.
fn write(
    commands: &mut Commands,
    client: u64,
    message: ClientMessage,
) {
    fn to<M: Send + Sync + 'static>(
        commands: &mut Commands,
        client: u64,
        request: M,
    ) {
        commands.write_message(FromClient { client, request });
    }
    match message {
        ClientMessage::Hello(m) => to(commands, client, m),
        ClientMessage::Say(say)
            if say.text.trim().starts_with('/') =>
        {
            to(commands, client, ChiefCommand(say));
        }
        ClientMessage::Open(m) => to(commands, client, m),
        ClientMessage::Close(m) => to(commands, client, m),
        ClientMessage::Compact(m) => to(commands, client, m),
        ClientMessage::Withdraw(m) => to(commands, client, m),
        ClientMessage::Resume(m) => to(commands, client, m),
        ClientMessage::ImportProject(m) => to(commands, client, m),
        ClientMessage::Retry(m) => to(commands, client, m),
        ClientMessage::Stop(m) => to(commands, client, m),
        ClientMessage::Edit(m) => to(commands, client, m),
        ClientMessage::Answer(m) => to(commands, client, m),
        ClientMessage::PutImage(m) => to(commands, client, m),
        ClientMessage::PutImagePart(m) => to(commands, client, m),
        ClientMessage::PutFile(m) => to(commands, client, m),
        ClientMessage::PutFilePart(m) => to(commands, client, m),
        ClientMessage::Rewind(m) => to(commands, client, m),
        ClientMessage::Say(m) => to(commands, client, m),
        ClientMessage::Read(protocol::Read::Models {
            id,
            refresh,
        }) => {
            to(commands, client, ReadModels { id, refresh });
        }
        ClientMessage::Read(m) => to(commands, client, m),
        ClientMessage::MoveWorktrees(m) => to(commands, client, m),
        ClientMessage::Terminal(m) => to(commands, client, m),
        ClientMessage::Login(m) => to(commands, client, m),
        ClientMessage::Check(m) => to(commands, client, m),
        ClientMessage::Mcp(m) => to(commands, client, m),
    }
}

/// A sign-in's start, answer or cancel.
fn login(
    mut received: ResMut<Messages<FromClient<protocol::Login>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            core.login(client, &request)?;
            Ok(())
        })
}

/// A try of the chosen model or of the connection.
fn check(
    mut received: ResMut<Messages<FromClient<protocol::Check>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            core.check(client, &request)?;
            Ok(())
        })
}

/// A chief's change to the list of MCP servers.
fn mcp(
    mut received: ResMut<Messages<FromClient<protocol::Mcp>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            core.mcp(client, request)?;
            Ok(())
        })
}

/// A read of the model catalogue.
fn read_models(
    mut received: ResMut<Messages<FromClient<ReadModels>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: ReadModels { id, refresh },
         }| {
            core.read_models(client, &id, refresh)?;
            Ok(())
        },
    )
}

/// A chief's line to an agent.
fn say(
    mut received: ResMut<Messages<FromClient<protocol::Say>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            core.say(client, request)?;
            Ok(())
        })
}

fn read(
    mut received: ResMut<Messages<FromClient<protocol::Read>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            core.read(client, request)?;
            Ok(())
        })
}

fn move_worktrees(
    mut received: ResMut<
        Messages<FromClient<protocol::MoveWorktrees>>,
    >,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            core.move_worktrees(client, &request.id)?;
            Ok(())
        })
}

fn hello(
    mut received: ResMut<Messages<FromClient<protocol::Hello>>>,
    mut connections: Connections,
) {
    for FromClient { client, .. } in received.drain() {
        connections.send(
            client,
            fault(None, "This connection said hello already."),
        );
    }
}

/// A chat watched from now on, with its last page.
fn open(
    mut received: ResMut<Messages<FromClient<protocol::Open>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: open,
         }| {
            let conversation = match core.watch_target(&open.chat)? {
                Ok(conversation) => conversation,
                Err(rejection) => {
                    core.answer(
                        client,
                        reply(&open.id, Err(rejection)),
                    )?;
                    return Ok(());
                }
            };
            let transcript = core.transcript(
                conversation,
                &open.chat,
                open.before.as_deref(),
                Some(&open.id),
            )?;
            core.connections.watch(client, open.chat, conversation);
            core.answer(client, transcript)?;
            Ok(())
        },
    )
}

fn close(
    mut received: ResMut<Messages<FromClient<protocol::Close>>>,
    mut connections: Connections,
) {
    for FromClient { client, request } in received.drain() {
        connections.unwatch(client, &request.chat);
    }
}

/// An agent's conversation summarized now, as when its context fills.
fn compact(
    mut received: ResMut<Messages<FromClient<protocol::Compact>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: compact,
         }| {
            let Some(agent) =
                core.reached(client, &compact.id, compact.to.0)?
            else {
                return Ok(());
            };
            let words = core.compact_chat(agent)?;
            core.answer(
                client,
                reply(&compact.id, Ok::<_, Rejection>(words)),
            )?;
            Ok(())
        },
    )
}

/// The chief's own queued line taken back before its turn.
fn withdraw(
    mut received: ResMut<Messages<FromClient<protocol::Withdraw>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: withdraw,
         }| {
            let Some(agent) = core.reached(
                client,
                &withdraw.id,
                withdraw.chat.with.0,
            )?
            else {
                return Ok(());
            };
            let words = core.withdraw_line(agent, &withdraw.item)?;
            core.answer(client, reply(&withdraw.id, words))?;
            Ok(())
        },
    )
}

/// The hold a stop put on an agent, or on every agent, ended.
fn resume(
    mut received: ResMut<Messages<FromClient<protocol::Resume>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: resume,
         }| {
            let targets = match resume.agent {
                Some(agent) => {
                    let Some(agent) =
                        core.reached(client, &resume.id, agent.0)?
                    else {
                        return Ok(());
                    };
                    vec![agent]
                }
                None => core.reachable()?,
            };
            let words = core.resume_agents(&targets)?;
            core.answer(
                client,
                reply(&resume.id, Ok::<_, Rejection>(words)),
            )?;
            Ok(())
        },
    )
}

fn import_project(
    mut received: ResMut<
        Messages<FromClient<protocol::ImportProject>>,
    >,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: project,
         }| {
            let words = core.import_project(&project)?;
            core.answer(client, reply(&project.id, words))?;
            Ok(())
        },
    )
}

fn retry(
    mut received: ResMut<Messages<FromClient<protocol::Retry>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: retry,
         }| {
            let Some(agent) =
                core.reached(client, &retry.id, retry.to.0)?
            else {
                return Ok(());
            };
            let words = core.retry_turn(agent)?;
            core.answer(client, reply(&retry.id, words))?;
            Ok(())
        },
    )
}

fn stop(
    mut received: ResMut<Messages<FromClient<protocol::Stop>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: stop,
         }| {
            let targets = match stop.agent {
                Some(agent) => {
                    let Some(agent) =
                        core.reached(client, &stop.id, agent.0)?
                    else {
                        return Ok(());
                    };
                    vec![agent]
                }
                None => core.reachable()?,
            };
            let words = core.stop_turns(&targets)?;
            core.answer(
                client,
                reply(&stop.id, Ok::<_, Rejection>(words)),
            )?;
            Ok(())
        },
    )
}

fn edit(
    mut received: ResMut<Messages<FromClient<protocol::Edit>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            core.edit(client, request)?;
            Ok(())
        })
}

/// A chief's answer to what waits.
fn answer(
    mut received: ResMut<Messages<FromClient<protocol::Answer>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: answer,
         }| {
            let words = core.answer_wait(
                &answer.wait,
                &answer.choice,
                answer.text.as_deref(),
                answer.picks.as_deref(),
            )?;
            core.answer(client, reply(&answer.id, words))?;
            Ok(())
        },
    )
}

fn put_image(
    mut received: ResMut<Messages<FromClient<protocol::PutImage>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            let answer = core.put_image(client, request);
            core.answer(client, answer)?;
            Ok(())
        })
}

fn put_image_part(
    mut received: ResMut<
        Messages<FromClient<protocol::PutImagePart>>,
    >,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            let answer = core.put_image_part(client, request)?;
            core.answer(client, answer)?;
            Ok(())
        })
}

fn put_file(
    mut received: ResMut<Messages<FromClient<protocol::PutFile>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            let answer = core.put_file(client, request)?;
            core.answer(client, answer)?;
            Ok(())
        })
}

fn put_file_part(
    mut received: ResMut<Messages<FromClient<protocol::PutFilePart>>>,
    mut core: Core,
) -> Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            let answer = core.put_file_part(client, request)?;
            core.answer(client, answer)?;
            Ok(())
        })
}

/// The chief's chat taken back to before one of its lines.
fn rewind(
    mut received: ResMut<Messages<FromClient<protocol::Rewind>>>,
    mut core: Core,
) -> Result {
    received.drain().try_for_each(
        |FromClient {
             client,
             request: rewind,
         }| {
            let Some(agent) =
                core.reached(client, &rewind.id, rewind.chat.with.0)?
            else {
                return Ok(());
            };
            let done = core.rewind_chat(agent, &rewind.item)?;
            let answer = match done {
                Ok((text, images, files)) => {
                    core.keep_images(&images);
                    core.keep_files(&files);
                    ServerMessage::Reply(protocol::Reply {
                        file: None,
                        files,
                        image: None,
                        images,
                        kind: protocol::ReplyKind::Rewound,
                        text,
                        to: rewind.id.clone(),
                        upload: None,
                    })
                }
                Err(rejection) => reply(&rewind.id, Err(rejection)),
            };
            core.answer(client, answer)?;
            Ok(())
        },
    )
}
