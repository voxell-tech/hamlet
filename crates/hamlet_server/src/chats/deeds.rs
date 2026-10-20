//! What a chief does to a chat (C24-C27, H34): a new session, going
//! back to one, carrying a turn on, reflecting, taking a line back
//! and stopping.

use anyhow::Result;
use hamlet_protocol::v0::{
    Address, ReplyKind, Rewind, ServerMessage,
};
use serde_json::json;

use super::follow::{self, Mode};
use super::records::{self, Place};
use crate::gate::{self, TrustKey};
use crate::harness::{BindingId, Harnesses, Input};
use crate::oversight;
use crate::protocol::{Refusal, reply};
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
) -> Result<std::result::Result<Done, Refusal>> {
    let agent = match chat_agent(service, address)? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(Err(refusal)),
    };
    Ok(self::deed(service, harnesses, &agent, deed)
        .await?
        .map_err(Refusal::Invalid))
}

/// The living agent of a chat a chief changes.
fn chat_agent(
    service: &mut Service,
    address: &Address,
) -> Result<std::result::Result<AgentRef, Refusal>> {
    Ok(service
        .agent(address, "change chat")?
        .map(|agent| AgentRef {
            hamlet: address.hamlet.clone(),
            number: agent.number,
        }))
}

/// Does `deed` on the chief's chat with `agent`; a refusal in words.
pub(super) async fn deed(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
    deed: Deed,
) -> Result<std::result::Result<Done, String>> {
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
                Err(words) => Ok(Err(words)),
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
    let db = service.home.store().connection();
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
        service.home.store().connection(),
        agent,
        &chief,
        number,
        binding,
        super::now(),
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
        records::set_retiring(
            service.home.store().connection(),
            previous,
            true,
        )?;
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
) -> Result<std::result::Result<(BindingId, String), String>> {
    let chief = service.chief.clone();
    let db = service.home.store().connection();
    let Some(session) = records::session(db, agent, &chief, number)?
    else {
        return Ok(Err(format!(
            "There is no past session `{number}` here."
        )));
    };
    let previous = super::current(service, harnesses, agent).await?;
    let db = service.home.store().connection();
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
fn busy(service: &mut Service, binding: BindingId) -> Result<bool> {
    let live = service
        .chats
        .conversations
        .get(&binding)
        .is_some_and(|c| c.running || c.pending > 0);
    Ok(live
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
) -> Result<std::result::Result<Done, String>> {
    let binding = super::current(service, harnesses, agent).await?;
    if busy(service, binding)? {
        return Ok(Err("A turn is running here. Stop it, then \
                       retry."
            .into()));
    }
    let items = service.home.store().items(binding)?;
    let last = items.iter().rev().find(|item| {
        use crate::harness::link::ItemKind;
        matches!(
            item.kind,
            ItemKind::User
                | ItemKind::Assistant
                | ItemKind::ToolResult
                | ItemKind::Summary
        )
    });
    let answered = last.is_none_or(|last| {
        last.kind == crate::harness::link::ItemKind::Assistant
            && matches!(last.stop.as_deref(), Some("stop" | "length"))
    });
    if answered {
        return Ok(Err("the last message was answered, so there \
                       is nothing to carry on"
            .into()));
    }
    let request = format!("retry:{}", uuid::Uuid::new_v4());
    let mark = super::mark(
        "hamlet.retry",
        json!({"task": super::new_task(), "at": super::now()}),
    );
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
    let id = rewind.id.as_str();
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
    let line = entry.mark.as_ref().filter(|mark| {
        mark.kind == "hamlet.chief"
            && mark.data.get("chief").and_then(|c| c.as_str())
                == Some(chief.as_str())
    });
    let Some(line) = line else {
        return refused(
            "That is not a chief's message in this conversation.",
        );
    };
    let text = line
        .data
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_owned();
    let since = line
        .data
        .get("at")
        .and_then(|t| t.as_i64())
        .unwrap_or(entry.at);
    let kept = &items[..target];
    let mut turns = 0;
    for item in kept {
        match item.mark.as_ref().map(|mark| mark.kind.as_str()) {
            Some("hamlet.reflection") => turns = 0,
            Some("hamlet.chief" | "hamlet.retry") => turns += 1,
            _ => {}
        }
    }
    let db = service.home.store().connection();
    let reflecting = follow::reserved(db, binding)?;
    if busy(service, binding)? || reflecting {
        return refused(
            "A turn is running here. Stop it, then rewind.",
        );
    }
    let db = service.home.store().connection();
    let Some(conversation) = records::kept(db, binding)? else {
        return refused(
            "The current conversation changed. Try again.",
        );
    };
    let count = conversation.unreflected.min(turns);
    let session = conversation.place.session;
    let woken = oversight::lapse(
        db,
        &agent,
        &chief,
        "rewound",
        since,
        Some(session),
        super::now(),
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
                service.home.store().connection(),
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
        service.home.store().connection(),
        &agent,
        &chief,
        session,
        fork,
        super::now(),
    )?;
    Ok(ServerMessage::Reply(hamlet_protocol::v0::Reply {
        kind: ReplyKind::Rewound,
        text,
        to: id.into(),
        upload: None,
    }))
}

/// Stops one agent, or every agent of the hamlets this chief leads,
/// before answering.
pub(super) async fn stop(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: Option<&Address>,
) -> Result<std::result::Result<String, Refusal>> {
    let Some(address) = agent else {
        for record in hamlets(service.home.store().connection())? {
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
    for binding in records::conversations_of(
        service.home.store().connection(),
        agent,
    )? {
        if busy(service, binding)? {
            stop_conversation(service, harnesses, agent, binding)
                .await?;
        }
    }
    Ok(())
}

/// Stops the turn of the conversation `binding` of `agent`, drops its
/// queued inputs and ends its waits.
pub(super) async fn stop_conversation(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
    binding: BindingId,
) -> Result<()> {
    let chief = service.chief.clone();
    let turn = service
        .chats
        .conversations
        .get(&binding)
        .and_then(|c| c.turn.clone());
    let dropped = records::queued_lines(
        service.home.store().connection(),
        binding,
        turn.as_deref(),
    )?;
    super::calls::stop_tools(service, binding);
    harnesses.cancel(service.home.store(), binding).await?;
    let db = service.home.store().connection();
    let place = records::kept(db, binding)?.map(|kept| kept.place);
    let mut woken = oversight::lapse(
        db,
        agent,
        place
            .as_ref()
            .and_then(|place| place.chief.as_deref())
            .unwrap_or(&chief),
        "stopped",
        0,
        None,
        super::now(),
    )?;
    for wait in oversight::open(db)?
        .iter()
        .filter(|wait| wait.agent.as_ref() == Some(agent))
        .filter(|wait| wait.kind != "fault")
    {
        if let Some((_, calls)) = oversight::settle(
            db,
            &wait.id,
            "stopped",
            "",
            Some(&chief),
            super::now(),
        )? {
            woken.extend(calls);
        }
    }
    for (runtime, key) in woken {
        harnesses.wake(&runtime, &key).await;
    }
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
