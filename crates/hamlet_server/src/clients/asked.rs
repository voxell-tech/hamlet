//! What a client asks: one system for each kind of request. The
//! runner takes a welcomed client's message apart and runs the
//! request's system once, with the client it came from ([`ask`]).
//! What awaits the disk or the workspace runs on a task, whose result
//! comes back as an input of its own.

use anyhow::{Result, anyhow};
use bevy_ecs::system::{In, IntoSystem, ResMut};
use bevy_ecs::world::World;
use hamlet_protocol::client::{
    self as protocol, ClientMessage, ServerMessage,
};

use super::check::{fault, reply};
use super::{Clients, Watch};
use crate::core::Core;
use crate::society::refusal::Refusal;

/// A request of the client with an id, as a system's input.
type Asked<M> = In<(u64, M)>;

/// Runs the system for `message` from `client`.
pub(crate) fn ask(
    world: &mut World,
    client: u64,
    message: ClientMessage,
) -> Result<()> {
    crate::fault::panic_at("request");
    match message {
        ClientMessage::Hello(_) => once(world, hello, (client, ())),
        ClientMessage::Say(say)
            if say.text.trim().starts_with('/') =>
        {
            once(world, crate::commands::said, (client, say))
        }
        ClientMessage::Open(m) => once(world, open, (client, m)),
        ClientMessage::Close(m) => once(world, close, (client, m)),
        ClientMessage::New(m) => once(world, new, (client, m)),
        ClientMessage::Resume(m) => once(world, resume, (client, m)),
        ClientMessage::Retry(m) => once(world, retry, (client, m)),
        ClientMessage::Stop(m) => once(world, stop, (client, m)),
        ClientMessage::Edit(m) => once(world, edit, (client, m)),
        ClientMessage::Answer(m) => once(world, answer, (client, m)),
        ClientMessage::PutImage(m) => {
            once(world, put_image, (client, m))
        }
        ClientMessage::PutImagePart(m) => {
            once(world, put_image_part, (client, m))
        }
        ClientMessage::PutFile(m) => {
            once(world, put_file, (client, m))
        }
        ClientMessage::PutFilePart(m) => {
            once(world, put_file_part, (client, m))
        }
        ClientMessage::Rewind(m) => once(world, rewind, (client, m)),
        ClientMessage::Say(m) => once(world, say, (client, m)),
        ClientMessage::Read(protocol::Read::Models {
            id,
            refresh,
        }) => once(world, read_models, (client, (id, refresh))),
        ClientMessage::Read(m) => once(world, read, (client, m)),
        ClientMessage::MoveLodges(m) => {
            once(world, move_lodges, (client, m))
        }
        ClientMessage::Login(m) => once(world, login, (client, m)),
        ClientMessage::Check(m) => once(world, check, (client, m)),
    }
}

/// A sign-in's start, answer or cancel.
fn login(
    In((client, login)): Asked<protocol::Login>,
    mut core: Core,
) -> Result<()> {
    core.login(client, &login)
}

/// A try of the chosen model or of the connection.
fn check(
    In((client, check)): Asked<protocol::Check>,
    mut core: Core,
) -> Result<()> {
    core.check(client, &check)
}

/// A read of the model catalogue.
fn read_models(
    In((client, (id, refresh))): Asked<(
        protocol::RequestId,
        Option<bool>,
    )>,
    mut core: Core,
) -> Result<()> {
    core.read_models(client, &id, refresh)
}

/// A chief's line to an agent.
fn say(
    In((client, say)): Asked<protocol::Say>,
    mut core: Core,
) -> Result<()> {
    core.say(client, say)
}

fn read(
    In((client, read)): Asked<protocol::Read>,
    mut core: Core,
) -> Result<()> {
    core.read(client, read)
}

fn move_lodges(
    In((client, asked)): Asked<protocol::MoveLodges>,
    mut core: Core,
) -> Result<()> {
    core.move_lodges(client, &asked.id)
}

/// Runs `system` once with `input`, as a cached one-shot system.
fn world_run<I, O, M>(
    world: &mut World,
    system: impl IntoSystem<In<I>, Result<O>, M> + 'static,
    input: I,
) -> Result<O>
where
    I: Send + 'static,
    O: 'static,
{
    world
        .run_system_cached_with(system, input)
        .map_err(|error| anyhow!("{error}"))?
}

fn once<I, M>(
    world: &mut World,
    system: impl IntoSystem<In<I>, Result<()>, M> + 'static,
    input: I,
) -> Result<()>
where
    I: Send + 'static,
{
    world_run(world, system, input)
}

fn hello(
    In((client, ())): Asked<()>,
    mut clients: ResMut<Clients>,
) -> Result<()> {
    clients.send(
        client,
        fault(None, "This connection said hello already."),
    );
    Ok(())
}

/// A chat watched from now on, with its last page.
fn open(
    In((client, open)): Asked<protocol::Open>,
    mut core: Core,
) -> Result<()> {
    let conversation = match core.watch_target(&open.chat)? {
        Ok(conversation) => conversation,
        Err(refusal) => {
            return core
                .answer(client, reply(&open.id, Err(refusal)));
        }
    };
    let transcript = core.transcript(
        conversation,
        &open.chat,
        open.before.as_deref(),
        Some(&open.id),
    )?;
    core.clients
        .watches
        .retain(|w| !(w.client == client && w.chat == open.chat));
    core.clients.watches.push(Watch {
        client,
        chat: open.chat,
        conversation,
    });
    core.answer(client, transcript)
}

fn close(
    In((client, close)): Asked<protocol::Close>,
    mut clients: ResMut<Clients>,
) -> Result<()> {
    clients
        .watches
        .retain(|w| !(w.client == client && w.chat == close.chat));
    Ok(())
}

fn new(
    In((client, new)): Asked<protocol::New>,
    mut core: Core,
) -> Result<()> {
    let Some(agent) = core.reached(client, &new.id, new.to.0)? else {
        return Ok(());
    };
    let words = core.new_session(agent)?;
    core.answer(client, reply(&new.id, Ok::<_, Refusal>(words)))
}

fn resume(
    In((client, resume)): Asked<protocol::Resume>,
    mut core: Core,
) -> Result<()> {
    let Some(agent) =
        core.reached(client, &resume.id, resume.to.0)?
    else {
        return Ok(());
    };
    let words = core.resume_session(agent, resume.number)?;
    core.answer(client, reply(&resume.id, words))
}

fn retry(
    In((client, retry)): Asked<protocol::Retry>,
    mut core: Core,
) -> Result<()> {
    let Some(agent) = core.reached(client, &retry.id, retry.to.0)?
    else {
        return Ok(());
    };
    let words = core.retry_turn(agent)?;
    core.answer(client, reply(&retry.id, words))
}

fn stop(
    In((client, stop)): Asked<protocol::Stop>,
    mut core: Core,
) -> Result<()> {
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
    core.answer(client, reply(&stop.id, Ok::<_, Refusal>(words)))
}

fn edit(
    In((client, edit)): Asked<protocol::Edit>,
    mut core: Core,
) -> Result<()> {
    core.edit(client, edit)
}

/// A chief's answer to what waits.
fn answer(
    In((client, answer)): Asked<protocol::Answer>,
    mut core: Core,
) -> Result<()> {
    let words = core.answer_wait(
        &answer.wait,
        &answer.choice,
        answer.text.as_deref(),
        answer.picks.as_deref(),
    )?;
    core.answer(client, reply(&answer.id, words))
}

fn put_image(
    In((client, put)): Asked<protocol::PutImage>,
    mut core: Core,
) -> Result<()> {
    let answer = core.put_image(client, put);
    core.answer(client, answer)
}

fn put_image_part(
    In((client, part)): Asked<protocol::PutImagePart>,
    mut core: Core,
) -> Result<()> {
    let answer = core.put_image_part(client, part)?;
    core.answer(client, answer)
}

fn put_file(
    In((client, put)): Asked<protocol::PutFile>,
    mut core: Core,
) -> Result<()> {
    let answer = core.put_file(client, put)?;
    core.answer(client, answer)
}

fn put_file_part(
    In((client, part)): Asked<protocol::PutFilePart>,
    mut core: Core,
) -> Result<()> {
    let answer = core.put_file_part(client, part)?;
    core.answer(client, answer)
}

/// The chief's chat taken back to before one of its lines, which is
/// given back to be said again.
fn rewind(
    In((client, rewind)): Asked<protocol::Rewind>,
    mut core: Core,
) -> Result<()> {
    if rewind.chat.from.is_some() || rewind.chat.past.is_some() {
        return core.answer(
            client,
            reply(&rewind.id, Err(Refusal::RewindNotCurrent)),
        );
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
    core.answer(client, answer)
}
