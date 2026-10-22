//! A line on its way to a session: a chief's command is carried
//! out, anything else is queued for the session's worker.

use std::path::PathBuf;

use anyhow::Result;
use bevy_ecs::prelude::*;
use hamlet_rules::chief;
use hamlet_rules::commands::{
    self, Command, Handler, InSession, OnHamlet, OnRealm, Ruling,
};

use crate::agents;
use crate::core::Settings;
use crate::session::mail::Mail;
use crate::session::output::{output, tell_chiefs};
use crate::session::worker::{Brief, Job, Jobs, Turn, start_worker};
use crate::session::{
    Entry, Instructed, JobFinished, Reply, Seat, SessionKey, Status,
    Transcript, TurnDropped, Typed, UserMessage, ensure, hamlet_of,
    place, realm, reopen, reseat,
};

/// Shows `reply` in `session` as all there is to say to a line.
fn answer(world: &mut World, session: Entity, reply: Reply) {
    output(world, session, reply);
    world.trigger(JobFinished { session });
}

fn failed(error: anyhow::Error) -> Reply {
    Reply::Error(format!("{error:#}"))
}

/// Carries out a chief's command about the hamlet the session is
/// with.
fn on_hamlet(
    world: &mut World,
    msg: &UserMessage,
    command: OnHamlet,
    rest: &str,
) {
    let done = hamlet_of(world, &msg.key)
        .carry_out(&msg.user, command, rest);
    let session = ensure(world, &msg.key);
    let reply = match done {
        Ok(decree) => {
            for name in &decree.touched {
                reopen(world, name);
            }
            agents::sync(world);
            Reply::Notice(decree.lines.join("\n"))
        }
        Err(e) => failed(e),
    };
    answer(world, session, reply);
}

/// Carries out a chief's command about the hamlets of the realm.
fn on_realm(
    world: &mut World,
    msg: &UserMessage,
    command: OnRealm,
    rest: &str,
) {
    let hamlet = hamlet_of(world, &msg.key);
    let done = chief::carry_out(
        &realm(world),
        hamlet.name(),
        &msg.user,
        command,
        rest,
    );
    let session = ensure(world, &msg.key);
    let reply = match done {
        Ok(text) => {
            agents::sync(world);
            tell_chiefs(world, session, &text);
            Reply::Notice(text)
        }
        Err(e) => failed(e),
    };
    answer(world, session, reply);
}

/// Puts a line to whoever keeps what waits for a chief. True when
/// it was dealt with there.
fn heard(
    world: &mut World,
    msg: &UserMessage,
    ruling: Option<(Ruling, String)>,
) -> bool {
    let mut typed = Typed {
        key: msg.key.clone(),
        user: msg.user.clone(),
        text: msg.text.clone(),
        ruling,
        reply: None,
    };
    world.trigger_ref(&mut typed);
    world.flush();
    let Some(reply) = typed.reply else {
        return false;
    };
    let session = ensure(world, &msg.key);
    if !reply.is_empty() {
        output(world, session, Reply::Notice(reply));
    }
    world.trigger(JobFinished { session });
    true
}

/// Carries out the command `msg` is, or queues it for the worker of
/// its session. `mail` comes with a message between agents.
pub(super) fn deliver(
    world: &mut World,
    msg: UserMessage,
    mail: Option<Mail>,
) {
    // Only a human gives commands.
    let human = !msg.key.is_mail() && mail.is_none();
    let slash = msg.text.trim().starts_with('/');
    let typed = human.then(|| commands::read(&msg.text)).flatten();
    let mut in_session = None;
    let dealt = match typed.map(|typed| (typed.command.by, typed)) {
        Some((Handler::Hamlet(command), typed)) => {
            on_hamlet(world, &msg, command, typed.rest);
            true
        }
        Some((Handler::Realm(command), typed)) => {
            on_realm(world, &msg, command, typed.rest);
            true
        }
        Some((Handler::Oversight(ruling), typed)) => {
            let ruling = (ruling, typed.rest.to_string());
            heard(world, &msg, Some(ruling))
        }
        // The channel that shows the chat changes who it is with.
        Some((Handler::Switch(_), _)) => false,
        Some((Handler::Session(command), typed)) => {
            let rest = typed.rest.to_string();
            in_session = Some((command, typed.command, rest));
            false
        }
        // Words to an agent may answer what it waits for.
        None => human && !slash && heard(world, &msg, None),
    };
    if dealt {
        return;
    }
    let session = ensure(world, &msg.key);
    let placed = place(world, &msg.key).map(|(seat, ..)| seat);
    // The office changed hands, or the agent moved: what was running
    // was someone else, or somewhere else.
    let had = world.get::<Seat>(session).cloned();
    if placed.as_ref().ok() != had.as_ref() {
        let _ = reseat(world, session, &msg.key);
        if let (Some(had), Ok(now)) = (&had, &placed)
            && had.name != now.name
        {
            let note = format!(
                "`{}` holds this office now, not `{}`. This is your \
                 conversation with `{}`.",
                now.name, had.name, now.name
            );
            output(world, session, Reply::Notice(note));
        }
    }
    if let Err(e) = staffed(world, session, &msg.key) {
        return answer(world, session, failed(e));
    }
    let job = match &in_session {
        Some((InSession::Budget, _, rest)) if rest.is_empty() => {
            agents::account(world);
            let model = &world.resource::<Settings>().config.model;
            let lines =
                hamlet_of(world, &msg.key).report(model).lines();
            return answer(world, session, Reply::Notice(lines));
        }
        Some((InSession::New, _, rest)) if rest.is_empty() => {
            Ok(Job::Archive { resume: None })
        }
        Some((InSession::Reflect, _, rest)) if rest.is_empty() => {
            Ok(Job::Reflect)
        }
        Some((InSession::Retry, _, rest)) if rest.is_empty() => {
            Ok(Job::Retry)
        }
        Some((InSession::Vote, command, rest)) => {
            asked_vote(world, &msg, command, rest)
        }
        Some((InSession::Resume, command, rest)) => {
            past(world, &msg.key, command, rest)
                .map(|log| Job::Archive { resume: Some(log) })
        }
        // With words after it, one of these is words to the agent.
        Some((
            InSession::Budget
            | InSession::New
            | InSession::Reflect
            | InSession::Retry,
            ..,
        ))
        | None => Ok(Job::Turn(turn(
            world,
            session,
            msg,
            mail,
            human && !slash,
        ))),
    };
    match job {
        Ok(job) => queue(world, session, job),
        Err(e) => {
            answer(world, session, Reply::Notice(format!("{e:#}")))
        }
    }
}

/// Gives `session` a worker where it has none that reads its queue.
pub(super) fn staffed(
    world: &mut World,
    session: Entity,
    key: &SessionKey,
) -> Result<()> {
    // A worker that ended, as one does after `/new`, reads no more
    // of its queue.
    if world
        .get::<Jobs>(session)
        .is_none_or(|jobs| jobs.queue.is_closed())
    {
        let jobs = start_worker(world, session, key)?;
        world.entity_mut(session).insert(jobs);
    }
    Ok(())
}

/// The turn that answers `msg`, shown in the transcript of
/// `session`. `direct` says a chief gave it as an instruction.
fn turn(
    world: &mut World,
    session: Entity,
    msg: UserMessage,
    mail: Option<Mail>,
    direct: bool,
) -> Turn {
    let mut owed = None;
    if let (Some(name), true) = (msg.key.agent(), direct) {
        // The mayor is told; a failure to note it must not stop the
        // instruction.
        owed = hamlet_of(world, &msg.key)
            .instructed(name, &msg.user, &msg.text)
            .ok();
    }
    let mut instruction = None;
    if direct {
        let mut instructed = Instructed {
            session,
            agent: msg.key.address().to_string(),
            user: msg.user.clone(),
            text: msg.text.clone(),
            instruction: None,
        };
        world.trigger_ref(&mut instructed);
        instruction = instructed.instruction;
    }
    if let Some(mut said) = world.get_mut::<Transcript>(session) {
        said.push(Entry::User {
            user: msg.user.clone(),
            text: msg.text.clone(),
        });
    }
    Turn {
        user: msg.user,
        text: msg.text,
        mail,
        instruction,
        owed,
    }
}

/// Hands `job` to the worker of `session`.
pub(super) fn queue(world: &mut World, session: Entity, job: Job) {
    let brief = Brief::read(world);
    let Some(mut jobs) = world.get_mut::<Jobs>(session) else {
        return;
    };
    jobs.last += 1;
    let sent = jobs.queue.send((jobs.last, brief, job));
    if let Some(mut status) = world.get_mut::<Status>(session) {
        status.pending += 1;
    }
    // The worker ended after it was found running.
    let Err(unsent) = sent else {
        return;
    };
    if let Some(mut status) = world.get_mut::<Status>(session) {
        status.pending = status.pending.saturating_sub(1);
    }
    if let (
        _,
        _,
        Job::Turn(Turn {
            instruction: Some(instruction),
            ..
        }),
    ) = unsent.0
    {
        world.trigger(TurnDropped { instruction });
    }
    let said = "The session was being put away and did not take \
                that. Send it again.";
    answer(world, session, Reply::Error(said.into()));
}

/// The vote that `/vote <agent> [reason]` asks for.
fn asked_vote(
    world: &World,
    msg: &UserMessage,
    command: &Command,
    rest: &str,
) -> Result<Job> {
    let hamlet = hamlet_of(world, &msg.key);
    if !realm(world).is_chief(hamlet.name(), &msg.user) {
        anyhow::bail!(
            "only a chief of `{}` can call its vote",
            hamlet.name()
        );
    }
    let mut words = rest.split_whitespace();
    let Some(candidate) = words.next() else {
        anyhow::bail!(
            "Say who would be mayor: `{}`. The hamlet's agents then \
             vote on it.",
            command.usage()
        );
    };
    let reason = words.collect::<Vec<_>>().join(" ");
    Ok(Job::Vote {
        user: msg.user.clone(),
        candidate: candidate.to_string(),
        reason: match reason.is_empty() {
            true => format!("Asked by the chief {}.", msg.user),
            false => reason,
        },
    })
}

/// The archived log that `/resume <number>` names.
fn past(
    world: &World,
    key: &SessionKey,
    command: &Command,
    number: &str,
) -> Result<PathBuf> {
    if number.is_empty()
        || !number.chars().all(|c| c.is_ascii_digit())
    {
        anyhow::bail!(
            "Say which: `{}`, with the number History shows for a \
             past session.",
            command.usage()
        );
    }
    let (_, _, path) = place(world, key)?;
    let past = path.with_extension(format!("{number}.jsonl"));
    if !past.exists() {
        anyhow::bail!("There is no past session `{number}` here.");
    }
    Ok(past)
}
