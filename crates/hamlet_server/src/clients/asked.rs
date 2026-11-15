//! A client's requests: one system for each kind. `PreUpdate` takes
//! in a connection's event and reads a welcomed client's message into
//! its request, written as a [`FromClient`] of its kind, and `Update`
//! runs the system for that kind. Whatever awaits the disk or the
//! workspace runs on a task, whose result comes back as an input of
//! its own.

use bevy_app::{App, PreUpdate};
use bevy_ecs::error::Result;
use bevy_ecs::message::Message;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::system::{Commands, ResMut};
use hamlet_protocol::client::{
    self as protocol, ClientMessage, RequestId, ServerMessage,
};
use tokio::net::TcpStream;

use super::check::{fault, reply};
use super::connection::Event;
use super::{Clients, Connections};
use crate::commands::ChiefCommand;
use crate::core::Core;
use crate::server::{AddHandler, InputMessage, InputOrigin};
use crate::society::refusal::Refusal;

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
        .add_handler::<FromClient<protocol::New>, _>(new)
        .add_handler::<FromClient<protocol::Resume>, _>(resume)
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
        .add_handler::<FromClient<protocol::MoveLodges>, _>(
            move_lodges,
        )
        .add_handler::<FromClient<protocol::Login>, _>(login)
        .add_handler::<FromClient<protocol::Check>, _>(check);
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

/// Takes in what a connection did: a client that is late or gone is
/// dealt with, and a welcomed client's message is written as a
/// request of its kind.
fn take_in_client_event(
    mut events: InputMessage<ClientEvent>,
    mut core: Core,
    mut origin: ResMut<InputOrigin>,
    mut commands: Commands,
) -> Result {
    let Some(ClientEvent(event)) = events.take() else {
        return Ok(());
    };
    let (client, text) = match event {
        Event::Late(client) => {
            core.late(client);
            return Ok(());
        }
        Event::Gone(client) => {
            // Its hooks and observers take what the client held,
            // and its uploads go with it.
            if let Some(gone) = core.connections.entity(client) {
                commands.entity(gone).despawn();
            }
            return Ok(());
        }
        Event::Message(client, text) => (client, text),
    };
    let Some(message) = core.checked(client, &text)? else {
        return Ok(());
    };
    *origin = InputOrigin::Client {
        client,
        request: message.id().cloned(),
    };
    crate::fault::panic_at("request");
    write(&mut commands, client, message);
    Ok(())
}

/// A connection is an entity until it goes.
fn connect(
    mut connected: InputMessage<ClientConnected>,
    mut clients: ResMut<Clients>,
    mut commands: Commands,
) {
    if let Some(ClientConnected(stream)) = connected.take() {
        commands.spawn(clients.accept(stream));
    }
}

/// Writes `message` from `client` as a [`FromClient`] of its kind,
/// for its system to read.
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
        ClientMessage::New(m) => to(commands, client, m),
        ClientMessage::Resume(m) => to(commands, client, m),
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
        ClientMessage::MoveLodges(m) => to(commands, client, m),
        ClientMessage::Login(m) => to(commands, client, m),
        ClientMessage::Check(m) => to(commands, client, m),
    }
}

/// A sign-in's start, answer or cancel.
fn login(
    mut asked: InputMessage<FromClient<protocol::Login>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: login,
    }) = asked.take()
    else {
        return Ok(());
    };
    core.login(client, &login)?;
    Ok(())
}

/// A try of the chosen model or of the connection.
fn check(
    mut asked: InputMessage<FromClient<protocol::Check>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: check,
    }) = asked.take()
    else {
        return Ok(());
    };
    core.check(client, &check)?;
    Ok(())
}

/// A read of the model catalogue.
fn read_models(
    mut asked: InputMessage<FromClient<ReadModels>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: ReadModels { id, refresh },
    }) = asked.take()
    else {
        return Ok(());
    };
    core.read_models(client, &id, refresh)?;
    Ok(())
}

/// A chief's line to an agent.
fn say(
    mut asked: InputMessage<FromClient<protocol::Say>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: say,
    }) = asked.take()
    else {
        return Ok(());
    };
    core.say(client, say)?;
    Ok(())
}

fn read(
    mut asked: InputMessage<FromClient<protocol::Read>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: read,
    }) = asked.take()
    else {
        return Ok(());
    };
    core.read(client, read)?;
    Ok(())
}

fn move_lodges(
    mut asked: InputMessage<FromClient<protocol::MoveLodges>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: asked,
    }) = asked.take()
    else {
        return Ok(());
    };
    core.move_lodges(client, &asked.id)?;
    Ok(())
}

fn hello(
    mut asked: InputMessage<FromClient<protocol::Hello>>,
    mut connections: Connections,
) -> Result {
    let Some(FromClient { client, .. }) = asked.take() else {
        return Ok(());
    };
    connections.send(
        client,
        fault(None, "This connection said hello already."),
    );
    Ok(())
}

/// A chat watched from now on, with its last page.
fn open(
    mut asked: InputMessage<FromClient<protocol::Open>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: open,
    }) = asked.take()
    else {
        return Ok(());
    };
    let conversation = match core.watch_target(&open.chat)? {
        Ok(conversation) => conversation,
        Err(refusal) => {
            core.answer(client, reply(&open.id, Err(refusal)))?;
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
}

fn close(
    mut asked: InputMessage<FromClient<protocol::Close>>,
    mut connections: Connections,
) -> Result {
    let Some(FromClient {
        client,
        request: close,
    }) = asked.take()
    else {
        return Ok(());
    };
    connections.unwatch(client, &close.chat);
    Ok(())
}

fn new(
    mut asked: InputMessage<FromClient<protocol::New>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: new,
    }) = asked.take()
    else {
        return Ok(());
    };
    let Some(agent) = core.reached(client, &new.id, new.to.0)? else {
        return Ok(());
    };
    let words = core.new_session(agent)?;
    core.answer(client, reply(&new.id, Ok::<_, Refusal>(words)))?;
    Ok(())
}

fn resume(
    mut asked: InputMessage<FromClient<protocol::Resume>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: resume,
    }) = asked.take()
    else {
        return Ok(());
    };
    let Some(agent) =
        core.reached(client, &resume.id, resume.to.0)?
    else {
        return Ok(());
    };
    let words = core.resume_session(agent, resume.number)?;
    core.answer(client, reply(&resume.id, words))?;
    Ok(())
}

fn retry(
    mut asked: InputMessage<FromClient<protocol::Retry>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: retry,
    }) = asked.take()
    else {
        return Ok(());
    };
    let Some(agent) = core.reached(client, &retry.id, retry.to.0)?
    else {
        return Ok(());
    };
    let words = core.retry_turn(agent)?;
    core.answer(client, reply(&retry.id, words))?;
    Ok(())
}

fn stop(
    mut asked: InputMessage<FromClient<protocol::Stop>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: stop,
    }) = asked.take()
    else {
        return Ok(());
    };
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
    core.answer(client, reply(&stop.id, Ok::<_, Refusal>(words)))?;
    Ok(())
}

fn edit(
    mut asked: InputMessage<FromClient<protocol::Edit>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: edit,
    }) = asked.take()
    else {
        return Ok(());
    };
    core.edit(client, edit)?;
    Ok(())
}

/// A chief's answer to what waits.
fn answer(
    mut asked: InputMessage<FromClient<protocol::Answer>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: answer,
    }) = asked.take()
    else {
        return Ok(());
    };
    let words = core.answer_wait(
        &answer.wait,
        &answer.choice,
        answer.text.as_deref(),
        answer.picks.as_deref(),
    )?;
    core.answer(client, reply(&answer.id, words))?;
    Ok(())
}

fn put_image(
    mut asked: InputMessage<FromClient<protocol::PutImage>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: put,
    }) = asked.take()
    else {
        return Ok(());
    };
    let answer = core.put_image(client, put);
    core.answer(client, answer)?;
    Ok(())
}

fn put_image_part(
    mut asked: InputMessage<FromClient<protocol::PutImagePart>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: part,
    }) = asked.take()
    else {
        return Ok(());
    };
    let answer = core.put_image_part(client, part)?;
    core.answer(client, answer)?;
    Ok(())
}

fn put_file(
    mut asked: InputMessage<FromClient<protocol::PutFile>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: put,
    }) = asked.take()
    else {
        return Ok(());
    };
    let answer = core.put_file(client, put)?;
    core.answer(client, answer)?;
    Ok(())
}

fn put_file_part(
    mut asked: InputMessage<FromClient<protocol::PutFilePart>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: part,
    }) = asked.take()
    else {
        return Ok(());
    };
    let answer = core.put_file_part(client, part)?;
    core.answer(client, answer)?;
    Ok(())
}

/// The chief's chat taken back to before one of its lines, which is
/// given back to be said again.
fn rewind(
    mut asked: InputMessage<FromClient<protocol::Rewind>>,
    mut core: Core,
) -> Result {
    let Some(FromClient {
        client,
        request: rewind,
    }) = asked.take()
    else {
        return Ok(());
    };
    if rewind.chat.from.is_some() || rewind.chat.past.is_some() {
        core.answer(
            client,
            reply(&rewind.id, Err(Refusal::RewindNotCurrent)),
        )?;
        return Ok(());
    }
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
        Err(refusal) => reply(&rewind.id, Err(refusal)),
    };
    core.answer(client, answer)?;
    Ok(())
}
