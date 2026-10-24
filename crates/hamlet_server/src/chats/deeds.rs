//! What a chief does to a chat (C24-C27, H34): a new session, going
//! back to one, carrying a turn on, reflecting, taking a line back
//! and stopping.

use anyhow::Result;
use hamlet_protocol::client::{
    Address, ReplyKind, Rewind, ServerMessage,
};

use super::follow::{self, Mode};
use super::records::{self, Place};
use crate::gate::{self, TrustKey};
use crate::harness::{BindingId, Harnesses, Input, InputMark};
use crate::oversight;
use crate::protocol::{Refusable, Refusal, reply};
use crate::record::AgentRef;
use crate::service::{Service, records as hamlets};

const STARTED: &str = "Started a new session.";
const RESUMED: &str = "Went back to an earlier session.";
/// The words a retry asks the model with (C26).
const CARRY_ON: &str = "Carry on from where you stopped.";

/// A deed on a chief's chat.
#[derive(Clone, Copy, Debug)]
pub(super) enum Deed {
    New,
    Resume(i64),
    Retry,
    Reflect,
}

/// A deed done, and the input it submitted.
pub(super) struct Done {
    pub text: String,
    pub submitted: Option<(BindingId, String)>,
    /// The notice it wrote, which its reply waits to see kept.
    pub written: Option<(BindingId, String)>,
}

/// A deed asked over the protocol, of the agent `address` names.
pub(super) async fn protocol(
    service: &mut Service,
    harnesses: &mut Harnesses,
    address: &Address,
    deed: Deed,
) -> Result<Refusable<Done>> {
    let agent = match chat_agent(service, address)? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(Err(refusal)),
    };
    self::deed(service, harnesses, &agent, deed).await
}

/// The living agent of a chat a chief changes.
fn chat_agent(
    service: &mut Service,
    address: &Address,
) -> Result<Refusable<AgentRef>> {
    Ok(service
        .agent(address, "change chat")?
        .map(|agent| AgentRef {
            hamlet: address.hamlet.clone(),
            number: agent.number,
        }))
}

/// Does `deed` on the chief's chat with `agent`.
pub(super) async fn deed(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
    deed: Deed,
) -> Result<Refusable<Done>> {
    let done = |text: &str, written| {
        Ok(Ok(Done {
            text: text.into(),
            submitted: None,
            written: Some(written),
        }))
    };
    match deed {
        Deed::New => {
            let written = start(service, harnesses, agent).await?;
            done(STARTED, written)
        }
        Deed::Resume(number) => {
            match resume(service, harnesses, agent, number).await? {
                Ok(written) => done(RESUMED, written),
                Err(refusal) => Ok(Err(refusal)),
            }
        }
        Deed::Retry => carry_on(service, harnesses, agent).await,
        Deed::Reflect => {
            let binding =
                super::current(service, harnesses, agent).await?;
            let submitted = follow::request_reflection(
                service,
                harnesses,
                binding,
                Mode::Explicit,
            )
            .await?;
            Ok(Ok(Done {
                text: String::new(),
                submitted: submitted
                    .map(|request| (binding, request)),
                written: None,
            }))
        }
    }
}

/// A fresh session, pointed at at once while its predecessor
/// finishes (C24); gives the notice written in it.
async fn start(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
) -> Result<(BindingId, String)> {
    let previous = super::current(service, harnesses, agent).await?;
    let chief = service.chief.clone();
    let db = service.db();
    let number = records::sessions(db, agent, &chief)?
        .iter()
        .map(|session| session.number)
        .max()
        .unwrap_or(0)
        + 1;
    let binding = super::begin(
        service,
        harnesses,
        &Place {
            agent: agent.clone(),
            chief: Some(chief.clone()),
            session: number,
        },
        None,
        0,
    )
    .await?;
    records::point(
        service.db(),
        agent,
        &chief,
        number,
        binding,
        crate::clock::now(),
    )?;
    let written =
        super::write_notice(service, harnesses, binding, STARTED)
            .await?;
    retire(service, harnesses, previous).await?;
    Ok((binding, written))
}

/// Puts the session of `previous` away, with its owed reflection.
async fn retire(
    service: &mut Service,
    harnesses: &mut Harnesses,
    previous: BindingId,
) -> Result<()> {
    if follow::reflect_every(service)? != 0 {
        records::set_retiring(service.db(), previous, true)?;
        follow::request_reflection(
            service,
            harnesses,
            previous,
            Mode::Owed,
        )
        .await?;
    }
    Ok(())
}

/// Goes back to the session `number` of the chief's chat (C24);
/// gives the notice written in it.
async fn resume(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
    number: i64,
) -> Result<Refusable<(BindingId, String)>> {
    let chief = service.chief.clone();
    let db = service.db();
    let Some(session) = records::session(db, agent, &chief, number)?
    else {
        return Ok(Err(Refusal::NoSession(number)));
    };
    let previous = super::current(service, harnesses, agent).await?;
    let db = service.db();
    records::select(db, agent, &chief, number)?;
    records::set_retiring(db, session.binding, false)?;
    gate::reset_trust(
        db,
        &TrustKey {
            agent,
            chief: &chief,
            session: number,
        },
    )?;
    let written = super::write_notice(
        service,
        harnesses,
        session.binding,
        RESUMED,
    )
    .await?;
    if previous != session.binding {
        retire(service, harnesses, previous).await?;
    }
    Ok(Ok((session.binding, written)))
}

/// Whether the conversation of `binding` runs a turn or has inputs
/// waiting.
fn is_busy(
    service: &mut Service,
    binding: BindingId,
) -> Result<bool> {
    Ok(service.chats.is_busy(binding)
        || service
            .home
            .store()
            .owed(binding, false)?
            .iter()
            .any(|owed| !owed.write))
}

/// Carries a failed or stopped turn on, as a new H12 task (C26).
async fn carry_on(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
) -> Result<Refusable<Done>> {
    let binding = super::current(service, harnesses, agent).await?;
    if is_busy(service, binding)? {
        return Ok(Err(Refusal::TurnRunning("retry")));
    }
    let items = service.home.store().items(binding)?;
    let last = items.iter().rev().find(|item| {
        use hamlet_protocol::runtime::ItemKind;
        matches!(
            item.kind,
            ItemKind::User
                | ItemKind::Assistant
                | ItemKind::ToolResult
                | ItemKind::Summary
        )
    });
    let answered = last.is_none_or(|last| {
        last.kind == hamlet_protocol::runtime::ItemKind::Assistant
            && matches!(last.stop.as_deref(), Some("stop" | "length"))
    });
    if answered {
        return Ok(Err(Refusal::Answered));
    }
    let request = format!("retry:{}", uuid::Uuid::new_v4());
    let mark = InputMark::Retry {
        task: super::new_task(),
        at: crate::clock::now(),
    };
    harnesses
        .submit(
            service.home.store(),
            binding,
            &Input {
                request: &request,
                content: CARRY_ON,
                steer: false,
                mark: Some(&mark),
                write: false,
                boundary: false,
            },
        )
        .await?;
    Ok(Ok(Done {
        text: "Carrying on from where it stopped.".into(),
        submitted: Some((binding, request)),
        written: None,
    }))
}

/// Takes back a chief's line by forking before it. What its turns
/// did and spent is not undone (C27).
pub(super) async fn rewind(
    service: &mut Service,
    harnesses: &mut Harnesses,
    rewind: &Rewind,
) -> Result<ServerMessage> {
    let id = &rewind.id;
    let chat = &rewind.chat;
    if let Err(refusal) =
        service.access(&chat.with.hamlet, "change chat")?
    {
        return Ok(reply(id, Err(refusal)));
    }
    if chat.from.is_some() {
        return Ok(reply(
            id,
            Err(Refusal::Invalid(
                "Conversations between agents are read-only.".into(),
            )),
        ));
    }
    if chat.past.is_some() {
        return Ok(reply(
            id,
            Err(Refusal::Invalid(
                "Open the current conversation to rewind it.".into(),
            )),
        ));
    }
    let agent = match chat_agent(service, &chat.with)? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(reply(id, Err(refusal))),
    };
    let refused = |words: &str| {
        Ok(reply(id, Err(Refusal::Invalid(words.into()))))
    };
    let binding = super::current(service, harnesses, &agent).await?;
    let chief = service.chief.clone();
    let items = service.home.store().items(binding)?;
    let Some(target) = items
        .iter()
        .position(|item| format!("{}:0", item.entry) == rewind.item)
    else {
        return refused(
            "That is not a chief's message in this conversation.",
        );
    };
    let entry = &items[target];
    let Some(InputMark::Chief {
        chief: by,
        text,
        at,
        ..
    }) = InputMark::of_item(entry.mark.as_ref())?
    else {
        return refused(
            "That is not a chief's message in this conversation.",
        );
    };
    if by != chief {
        return refused(
            "That is not a chief's message in this conversation.",
        );
    }
    let since = at;
    let kept = &items[..target];
    let mut turns = 0;
    for mark in kept.iter().filter_map(|item| item.mark.as_ref()) {
        match InputMark::of(mark)? {
            Some(InputMark::Reflection { .. }) => turns = 0,
            Some(mark) if mark.is_chief_turn() => turns += 1,
            _ => {}
        }
    }
    let db = service.db();
    let reflecting = follow::reserved(db, binding)?;
    if is_busy(service, binding)? || reflecting {
        return Ok(reply(id, Err(Refusal::TurnRunning("rewind"))));
    }
    let db = service.db();
    let Some(conversation) = records::kept(db, binding)? else {
        return refused(
            "The current conversation changed. Try again.",
        );
    };
    let count = conversation.unreflected.min(turns);
    let session = conversation.place.session;
    let woken = oversight::rewind(
        db,
        &agent,
        &chief,
        session,
        since,
        crate::clock::now(),
    )?;
    for (runtime, key) in woken {
        harnesses.wake(&runtime, &key).await;
    }
    let place = Place {
        agent: agent.clone(),
        chief: Some(chief.clone()),
        session,
    };
    let fork = match kept.last() {
        Some(previous) => {
            let fork = harnesses
                .fork(service.home.store(), binding, &previous.entry)
                .await?;
            records::keep(
                service.db(),
                fork,
                &place,
                Some(binding),
                count,
            )?;
            fork
        }
        None => {
            super::begin(
                service,
                harnesses,
                &place,
                Some(binding),
                count,
            )
            .await?
        }
    };
    records::point(
        service.db(),
        &agent,
        &chief,
        session,
        fork,
        crate::clock::now(),
    )?;
    Ok(ServerMessage::Reply(hamlet_protocol::client::Reply {
        kind: ReplyKind::Rewound,
        text,
        to: rewind.id.clone(),
        upload: None,
    }))
}

/// Stops one agent, or every agent of the hamlets this chief leads,
/// before answering.
pub(super) async fn stop(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: Option<&Address>,
) -> Result<Refusable<String>> {
    let Some(address) = agent else {
        for record in hamlets(service.db())? {
            if service.access(&record.id, "stop")?.is_err() {
                continue;
            }
            for agent in &record.agents {
                let agent = AgentRef {
                    hamlet: record.id.clone(),
                    number: agent.number,
                };
                stop_agent(service, harnesses, &agent).await?;
            }
        }
        return Ok(Ok("Stopping everything.".into()));
    };
    let agent = match chat_agent(service, address)? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(Err(refusal)),
    };
    stop_agent(service, harnesses, &agent).await?;
    let name =
        service.home.store().record(&agent.hamlet)?.map_or_else(
            || "the agent".to_owned(),
            |record| record.address_of(agent.number).to_owned(),
        );
    Ok(Ok(format!("Stopping {name}.")))
}

/// Stops the running conversations of `agent` and drops their queued
/// inputs (C25, H34).
pub(crate) async fn stop_agent(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
) -> Result<()> {
    let mut stopped = false;
    for binding in records::conversations_of(service.db(), agent)? {
        if is_busy(service, binding)? {
            stop_turn(service, harnesses, binding).await?;
            stopped = true;
        }
    }
    if stopped {
        end_waits(service, harnesses, agent).await?;
    }
    Ok(())
}

/// Stops the turn of the conversation `binding` of `agent`, drops its
/// queued inputs and ends its waits.
#[cfg(feature = "harness")]
pub(super) async fn stop_conversation(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
    binding: BindingId,
) -> Result<()> {
    stop_turn(service, harnesses, binding).await?;
    end_waits(service, harnesses, agent).await
}

/// Ends what waits for a stop of `agent` and tells the calls it held.
async fn end_waits(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
) -> Result<()> {
    let woken = oversight::stop(
        service.db(),
        agent,
        &service.chief,
        crate::clock::now(),
    )?;
    for (runtime, key) in woken {
        harnesses.wake(&runtime, &key).await;
    }
    Ok(())
}

/// Stops the turn of the conversation `binding` and drops its queued
/// inputs.
async fn stop_turn(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
) -> Result<()> {
    let chief = service.chief.clone();
    let turn = service.chats.turn(binding);
    let dropped = service
        .home
        .store()
        .queued_lines(binding, turn.as_deref())?;
    super::calls::stop_tools(service, binding);
    harnesses.cancel(service.home.store(), binding).await?;
    let text = format!(
        "Stopped: stopped by {chief}{}",
        if dropped == 0 {
            String::new()
        } else {
            format!(" ({dropped} queued dropped)")
        }
    );
    let made = service
        .home
        .store()
        .binding(binding)?
        .is_some_and(|bound| bound.conversation.is_some());
    if made {
        service.chats.stopping.insert(binding, text);
    } else {
        super::write_notice(service, harnesses, binding, &text)
            .await?;
    }
    Ok(())
}
