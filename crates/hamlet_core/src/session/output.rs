//! A reply of a session's turn as it lands on the app's thread: in
//! the entities, in the transcript and with the channels.

use std::time::Instant;

use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use hamlet_rules::civic::Civic;

use super::mail::{Letter, mirror};
use super::{
    Reply, SessionKey, SessionOutput, Status, TalksTo, Transcript,
    Usage, reopen, turns,
};
use crate::agent::Event as AgentEvent;
use crate::agents::{
    self, Activity, Busy, Citizen, Left, Mayor, Saying,
};

/// Tells every other session a chief has open what happened.
pub(super) fn tell_chiefs(
    world: &mut World,
    except: Entity,
    text: &str,
) {
    let others = world
        .query::<(Entity, &SessionKey)>()
        .iter(world)
        .filter(|(e, key)| {
            *e != except && !key.is_mail() && !key.is_asked()
        })
        .map(|(e, _)| e)
        .collect::<Vec<_>>();
    for session in others {
        output(world, session, Reply::Notice(text.to_string()));
    }
}

/// Brings the entities in step with what a turn just wrote: the
/// record when the hamlet changed, the accounts when something was
/// spent.
fn note_record(world: &mut World, session: Entity, reply: &Reply) {
    let Reply::Event(outer) = reply else {
        return;
    };
    let mut event = outer;
    while let AgentEvent::Agent { event: inner, .. } = event {
        event = inner.as_ref();
    }
    match event {
        AgentEvent::Civic(Civic::Ballot { voter, .. }) => {
            agents::sync(world);
            // A ballot is shown where the vote was called, and the
            // chief reads them all when it closes.
            if let Some(agent) = agents::named(world, voter) {
                world.trigger(agents::Voted { agent });
            }
        }
        AgentEvent::Consulted { by } => {
            let agent = agents::named(world, by);
            world.trigger(agents::Consulted { agent });
        }
        AgentEvent::Civic(civic) => {
            agents::sync(world);
            tell_chiefs(world, session, &civic.notice());
        }
        AgentEvent::AgentRemoved { name: gone }
        | AgentEvent::AgentsMerged { gone, .. } => {
            reopen(world, gone);
            agents::sync(world);
        }
        AgentEvent::Message { .. }
        | AgentEvent::RoadBuilt { .. }
        | AgentEvent::AgentCreated { .. } => agents::sync(world),
        AgentEvent::ModelCall(_) | AgentEvent::ToolDone { .. } => {
            agents::account(world)
        }
        _ => {}
    }
}

/// Adds an event of `agent`'s turn to what it is saying.
fn speak(world: &mut World, agent: Entity, event: &AgentEvent) {
    let Ok(mut agent) = world.get_entity_mut(agent) else {
        return;
    };
    let mut saying =
        agent.get::<Saying>().cloned().unwrap_or_default();
    let was = saying.clone();
    saying.note(event);
    if saying != was || !agent.contains::<Saying>() {
        agent.insert(saying);
    }
}

/// Sets what `agent` is doing, and whether it is busy.
fn set_work(
    world: &mut World,
    agent: Entity,
    busy: bool,
    activity: Activity,
) {
    let Ok(mut agent) = world.get_entity_mut(agent) else {
        return;
    };
    if agent.get::<Activity>() != Some(&activity) {
        agent.insert(activity);
    }
    match (busy, agent.contains::<Busy>()) {
        (true, false) => {
            agent.insert(Busy);
        }
        (false, true) => {
            agent.remove::<Busy>();
        }
        _ => {}
    }
}

/// Keeps what each agent is doing in step with the turn of
/// `session`. `working` says more turns are queued behind it.
pub(super) fn note_work(
    world: &mut World,
    session: Entity,
    working: bool,
    reply: &Reply,
) {
    let Some(agent) = world.get::<TalksTo>(session).map(|t| t.0)
    else {
        return;
    };
    let was =
        world.get::<Activity>(agent).copied().unwrap_or_default();
    // A session with an agent: what happens in it is that agent's.
    if world.get::<Mayor>(agent).is_none() {
        match reply {
            Reply::Event(event) => {
                let mut next = was;
                next.note(event);
                set_work(world, agent, true, next);
                speak(world, agent, event);
            }
            Reply::Done(_) | Reply::Error(_) if working => {
                set_work(world, agent, true, was)
            }
            Reply::Done(_) | Reply::Error(_) => {
                set_work(world, agent, false, Activity::default())
            }
            Reply::Notice(_) => {}
        }
        return;
    }
    match reply {
        Reply::Event(AgentEvent::Agent { .. }) | Reply::Notice(_) => {
        }
        Reply::Event(event) => {
            let mut next = was;
            next.note(event);
            set_work(world, agent, next.step > 0, next);
            speak(world, agent, event);
        }
        Reply::Done(_) | Reply::Error(_) => {
            set_work(world, agent, false, Activity::default())
        }
    }
    // Only an agent's own event and the end of a step say anything
    // of the others.
    if !matches!(
        reply,
        Reply::Event(
            AgentEvent::Agent { .. } | AgentEvent::ToolDone { .. }
        ) | Reply::Done(_)
            | Reply::Error(_)
    ) {
        return;
    }
    let others = world
        .query_filtered::<(Entity, &Name, &Activity, Has<Busy>), (
            With<Citizen>,
            Without<Mayor>,
            Without<Left>,
        )>()
        .iter(world)
        .map(|(entity, name, activity, busy)| {
            (entity, name.as_str().to_string(), *activity, busy)
        })
        .collect::<Vec<_>>();
    match reply {
        Reply::Event(AgentEvent::Agent { name, event }) => {
            // Everyone along the chain of messages is busy; the
            // step belongs to the one at its end.
            let mut chain = vec![name.as_str()];
            let mut inner = &**event;
            while let AgentEvent::Agent { name, event } = inner {
                chain.push(name);
                inner = event;
            }
            let last = chain[chain.len() - 1];
            for (entity, name, mut activity, _) in others {
                if name == last {
                    activity.note(inner);
                    speak(world, entity, inner);
                }
                let busy = chain.contains(&name.as_str());
                set_work(world, entity, busy, activity);
            }
        }
        Reply::Event(AgentEvent::ToolDone { .. })
        | Reply::Done(_)
        | Reply::Error(_)
            if others.iter().any(|(.., busy)| *busy) =>
        {
            for (entity, ..) in others {
                set_work(world, entity, false, Activity::default());
            }
        }
        _ => {}
    }
}

pub(super) fn output(
    world: &mut World,
    session: Entity,
    reply: Reply,
) {
    let key = world.get::<SessionKey>(session).cloned();
    // True when more turns are queued behind this one.
    let working =
        world.get::<Status>(session).is_some_and(|s| s.pending > 1);
    note_record(world, session, &reply);
    note_work(world, session, working, &reply);
    turns::note(world, session, &reply);
    if let Reply::Event(AgentEvent::Agent { name, event }) = &reply {
        mirror(world, session, name, event);
    }
    if let (
        Some(from),
        Reply::Event(AgentEvent::Tell { to, text, task }),
    ) = (&key, &reply)
    {
        world.trigger(Letter {
            from: from.clone(),
            to: to.clone(),
            text: text.clone(),
            task: task.clone(),
        });
    }
    let activity = match &reply {
        Reply::Event(AgentEvent::Tool { name, .. }) => {
            Some(format!("tool {name}"))
        }
        Reply::Event(AgentEvent::Agent { name, .. }) => {
            Some(format!("agent {name}"))
        }
        Reply::Done(_) => Some("replied".to_string()),
        Reply::Error(_) => Some("error".to_string()),
        _ => None,
    };
    let Ok(mut entity) = world.get_entity_mut(session) else {
        return;
    };
    if let Some(mut t) = entity.get_mut::<Transcript>() {
        t.apply(&reply);
    }
    if let (Some(a), Some(mut s)) =
        (activity, entity.get_mut::<Status>())
    {
        s.last_activity = Some((Instant::now(), a));
    }
    let call = match &reply {
        Reply::Event(AgentEvent::ModelCall(call)) => Some(call),
        // An agent's calls are spent by the session that asked it.
        Reply::Event(AgentEvent::Agent { event, .. }) => {
            match &**event {
                AgentEvent::ModelCall(call) => Some(call),
                _ => None,
            }
        }
        _ => None,
    };
    if let (Some(call), Some(mut u)) =
        (call, entity.get_mut::<Usage>())
    {
        u.record(call);
    }
    let Some(key) = entity.get::<SessionKey>().cloned() else {
        return;
    };
    world.trigger(SessionOutput {
        session,
        key,
        reply,
    });
}
