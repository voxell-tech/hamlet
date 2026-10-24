//! Agreements between current mayors and recoverable house moves.

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Deserialize;

use super::{
    Agreed, End, Move, Pending, Realm, Removal, Road, read, resolve,
    write,
};
use crate::Outcome;
use crate::harness::Harnesses;
use crate::limits::Limits;
use crate::protocol::Refusal;
use crate::record::{
    self, Actor, AgentRef, Audience, Change, RecordOperation,
    RecordResult,
};
use crate::service::Service;
use crate::society::letters::Message;
use crate::society::{self, Caller, Notice, SocietyOutcome};

/// The arguments of `ask_removal`.
#[derive(Deserialize)]
pub(crate) struct AskRemoval {
    hamlet: String,
    reason: String,
}

/// The arguments of `agree_road`.
#[derive(Deserialize)]
pub(crate) struct AgreeRoad {
    a: String,
    b: String,
    reason: String,
}

/// The arguments of `agree_move`.
#[derive(Deserialize)]
pub(crate) struct AgreeMove {
    agent: String,
    to: String,
    #[serde(default)]
    keep_roads: Vec<String>,
    reason: String,
}

/// The mayor's `ask_removal`: asks the chief to remove a hamlet.
pub(crate) async fn ask_removal(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    given: AskRemoval,
) -> Result<Option<SocietyOutcome>> {
    let limits = service.limits_of("home")?;
    deal(
        service,
        harnesses,
        caller,
        &given.reason,
        |db, realm, from, reason| {
            removal(db, realm, from, reason, &given.hamlet, &limits)
        },
    )
    .await
}

/// The mayor's `agree_road`: agrees to a road between hamlets.
pub(crate) async fn agree_road(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    given: AgreeRoad,
) -> Result<Option<SocietyOutcome>> {
    let limits = service.limits_of("home")?;
    deal(
        service,
        harnesses,
        caller,
        &given.reason,
        |db, realm, from, reason| {
            road(
                db,
                realm,
                from,
                reason,
                [&given.a, &given.b],
                &limits,
            )
        },
    )
    .await
}

/// The mayor's `agree_move`: agrees that an agent moves with its
/// house.
pub(crate) async fn agree_move(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    given: AgreeMove,
) -> Result<Option<SocietyOutcome>> {
    let task = caller.task.clone();
    deal(
        service,
        harnesses,
        caller,
        &given.reason,
        |db, realm, from, reason| {
            moving(db, realm, from, reason, &given, task)
        },
    )
    .await
}

/// Carries out a mayor's dealing with other hamlets once, under its
/// call: `make` changes the realm for the mayor `from`, who gave a
/// reason. Then a pending house move is made, the other mayor is
/// asked and a hamlet's mayor told, as the outcome says.
async fn deal(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    reason: &str,
    make: impl FnOnce(
        &Connection,
        &mut Realm,
        &AgentRef,
        &str,
    ) -> Result<SocietyOutcome>,
) -> Result<Option<SocietyOutcome>> {
    let from = &caller.place.agent;
    let outcome = society::once(
        service.home.store(),
        &crate::OperationId {
            runtime: crate::RuntimeInstance::new("realm-agreements")?,
            key: serde_json::to_string(&(
                caller.id.runtime.as_str(),
                &caller.id.key,
            ))?,
        },
        &caller.request,
        |db| {
            let mut realm = read(db)?;
            let own = record::read(db, &from.hamlet)?;
            if !realm.is_living(&from.hamlet)
                || realm.live().count() < 2
                || own
                    .as_ref()
                    .is_none_or(|r| r.require_mayor(from).is_some())
            {
                return Ok(SocietyOutcome::failed(
                    "Only a mayor of a living hamlet can do that.",
                ));
            }
            if let Some(held) = own.as_ref().and_then(|r| {
                r.may_decide(
                    crate::clock::now(),
                    record::Action::Mayor,
                )
            }) {
                return Ok(SocietyOutcome::failed(
                    held.text(Audience::Agent),
                ));
            }
            let reason = reason.trim();
            if reason.is_empty() {
                return Ok(SocietyOutcome::failed(
                    "say why in `reason`; the chief is shown it",
                ));
            }
            make(db, &mut realm, from, reason)
        },
    )?;
    if let Some(pending) = read(service.db())?.pending {
        crate::chats::stop_agent(service, harnesses, &pending.to)
            .await?;
        for binding in crate::chats::records::conversations_of(
            service.db(),
            &pending.to,
        )? {
            harnesses.cancel(service.home.store(), binding).await?;
        }
    }
    finish(service)?;
    if let Some(ask) = outcome.ask.clone()
        && society::letters::ask(service, harnesses, caller, ask)
            .await?
            .is_none()
    {
        return Ok(None);
    }
    if let Some(notice) = &outcome.notice
        && let Some(r) =
            service.home.store().record(&notice.hamlet)?
    {
        let binding = crate::chats::current(
            service,
            harnesses,
            &AgentRef {
                hamlet: notice.hamlet.clone(),
                number: r.mayor,
            },
        )
        .await?;
        crate::chats::write(
            service,
            harnesses,
            binding,
            &format!(
                "notice:{}:{}",
                caller.id.runtime.as_str(),
                caller.id.key
            ),
            &notice.text,
            false,
        )
        .await?;
    }
    society::reconfigure(service, harnesses).await?;
    Ok(Some(outcome))
}

/// Records the mayor `from`'s ask that a hamlet be removed.
fn removal(
    db: &Connection,
    realm: &mut Realm,
    from: &AgentRef,
    reason: &str,
    id: &str,
    limits: &Limits,
) -> Result<SocietyOutcome> {
    if !realm.is_living(id) {
        let refusal = Refusal::NoHamlet(id.into());
        return Ok(SocietyOutcome::failed(refusal.to_string()));
    }
    realm.removals.push(Removal {
        hamlet: id.into(),
        by: from.hamlet.clone(),
        reason: reason.into(),
        at: crate::clock::now(),
    });
    let asked = realm
        .removals
        .iter()
        .filter(|r| r.hamlet == id && realm.is_living(&r.by))
        .map(|r| r.by.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let ready = asked.contains(id)
        || asked.len() >= limits.hamlets.removal_mayors as usize;
    let text = format!(
        "The mayor of `{}` asks for the hamlet `{id}` to be \
         removed. {}",
        from.hamlet,
        if ready {
            format!(
                "Enough have asked: /remove-hamlet {id} confirms it."
            )
        } else {
            "Not enough mayors have asked yet.".into()
        }
    );
    write(db, realm)?;
    Ok(SocietyOutcome {
        notice: Some(Notice {
            hamlet: id.into(),
            text: text.clone(),
        }),
        ..SocietyOutcome::answered(text)
    })
}

/// Records the mayor `from`'s agreement to a road between hamlets.
fn road(
    db: &Connection,
    realm: &mut Realm,
    from: &AgentRef,
    reason: &str,
    ends_named: [&str; 2],
    limits: &Limits,
) -> Result<SocietyOutcome> {
    if !limits.hamlets.roads_between {
        return Ok(SocietyOutcome::failed(
            "The chief does not allow roads between hamlets.",
        ));
    }
    let mut ends = Vec::new();
    for address in ends_named {
        match resolve(db, &from.hamlet, address)? {
            Ok(resolved) => ends.push(resolved.agent),
            Err(error) => {
                return Ok(SocietyOutcome::failed(
                    error.text(address),
                ));
            }
        }
    }
    let (a, b) = (&ends[0], &ends[1]);
    for end in [a, b] {
        if record::read(db, &end.hamlet)?
            .is_some_and(|r| r.mayor == end.number)
        {
            return Ok(SocietyOutcome::failed(
                "The mayors already have an office road; agree to a \
                 road between other agents.",
            ));
        }
    }
    if a.hamlet == b.hamlet
        || (a.hamlet != from.hamlet && b.hamlet != from.hamlet)
    {
        return Ok(SocietyOutcome::failed(
            "Agree to a road between your hamlet and another hamlet.",
        ));
    }
    let pair = |r: &Road| {
        (r.a == End::Agent(a.clone()) && r.b == End::Agent(b.clone()))
            || (r.a == End::Agent(b.clone())
                && r.b == End::Agent(a.clone()))
    };
    if let Some(road) = realm.roads.iter_mut().find(|r| pair(r)) {
        if !road.agreed.contains(&from.hamlet) {
            road.agreed.push(from.hamlet.clone());
        }
    } else {
        realm.roads.push(Road {
            a: End::Agent(a.clone()),
            b: End::Agent(b.clone()),
            agreed: vec![from.hamlet.clone()],
            reason: reason.into(),
            messages: 0,
            last_from: None,
            last_used: 0,
        });
    }
    write(db, realm)?;
    let mut result = SocietyOutcome::answered(
        "The road agreement is recorded; it works once both mayors \
         agree.",
    );
    if realm
        .roads
        .iter()
        .find(|r| pair(r))
        .is_some_and(|r| r.agreed.len() < 2)
    {
        let other = if a.hamlet == from.hamlet {
            &b.hamlet
        } else {
            &a.hamlet
        };
        result.ask = Some(Message {
            name: format!("mayor@{other}"),
            message: format!(
                "Please consider agreeing to a road between {}#{} \
                 and {}#{}. {reason}",
                a.hamlet, a.number, b.hamlet, b.number
            ),
        });
    }
    Ok(result)
}

/// Records the mayor `from`'s agreement to an agent's move, and once
/// both mayors agree, moves it in the record.
fn moving(
    db: &Connection,
    realm: &mut Realm,
    from: &AgentRef,
    reason: &str,
    given: &AgreeMove,
    task: Option<String>,
) -> Result<SocietyOutcome> {
    let address = &given.agent;
    let agent = match resolve(db, &from.hamlet, address)? {
        Ok(r) => r.agent,
        Err(error) => {
            return Ok(SocietyOutcome::failed(error.text(address)));
        }
    };
    let to = given.to.as_str();
    if !realm.is_living(to)
        || agent.hamlet == to
        || (from.hamlet != agent.hamlet && from.hamlet != to)
    {
        return Ok(SocietyOutcome::failed(
            "A move needs the agent's mayor and the receiving mayor.",
        ));
    }
    if realm.pending.is_some() {
        return Ok(SocietyOutcome::failed(
            "A house move is still pending; finish it before \
             another move.",
        ));
    }
    let old = record::read(db, &agent.hamlet)?
        .context("the moving agent's hamlet has no record")?;
    let target = record::read(db, to)?
        .context("the target hamlet has no record")?;
    let other = if from.hamlet == old.id { &target } else { &old };
    if let Some(held) =
        other.may_decide(crate::clock::now(), record::Action::Move)
    {
        return Ok(SocietyOutcome::failed(
            held.text(Audience::Agent),
        ));
    }
    if agent.number == old.mayor {
        return Ok(SocietyOutcome::failed(
            "A mayor cannot move to another hamlet.",
        ));
    }
    let keep = given
        .keep_roads
        .iter()
        .filter_map(|name| old.named(name))
        .map(|a| a.number)
        .filter(|number| {
            old.roads_of().iter().any(|road| {
                let end = |end| match end {
                    record::End::Office => old.mayor,
                    record::End::Agent(number) => number,
                };
                (end(road.a) == agent.number
                    && end(road.b) == *number)
                    || (end(road.b) == agent.number
                        && end(road.a) == *number)
            })
        })
        .collect::<Vec<_>>();
    let index = match realm
        .moves
        .iter()
        .position(|m| m.agent == agent && m.to == to)
    {
        Some(index) => index,
        None => {
            realm.moves.push(Move {
                agent: agent.clone(),
                to: to.into(),
                agreed: Vec::new(),
            });
            realm.moves.len() - 1
        }
    };
    let moving = &mut realm.moves[index];
    moving.agreed.retain(|a| a.hamlet != from.hamlet);
    moving.agreed.push(Agreed {
        hamlet: from.hamlet.clone(),
        keep,
        reason: reason.into(),
        at: crate::clock::now(),
    });
    if moving.agreed.len() < 2 {
        write(db, realm)?;
        let other =
            if from.hamlet == to { &agent.hamlet } else { to };
        return Ok(SocietyOutcome {
            ask: Some(Message {
                name: format!("mayor@{other}"),
                message: format!(
                    "Please consider agreeing to move {address} to \
                     {to}. {reason}"
                ),
            }),
            ..SocietyOutcome::answered(
                "The move is proposed; the other mayor must agree.",
            )
        });
    }
    let keep = moving.agreed[0]
        .keep
        .iter()
        .filter(|n| moving.agreed[1].keep.contains(n))
        .copied()
        .collect::<Vec<_>>();
    let result = crate::operation::change(
        db,
        &RecordOperation {
            hamlet: agent.hamlet.clone(),
            actor: Actor::Mayors {
                leaving: AgentRef {
                    hamlet: agent.hamlet.clone(),
                    number: old.mayor,
                },
                joining: AgentRef {
                    hamlet: to.into(),
                    number: target.mayor,
                },
            },
            at: crate::clock::now(),
            reason: reason.into(),
            task,
            change: Change::MoveMember {
                number: agent.number,
                to: to.into(),
            },
        },
    )?;
    let moved = match result {
        Outcome::Done(RecordResult::Moved(moved)) => moved,
        Outcome::Refused(refusal) => {
            return Ok(SocietyOutcome::failed(
                refusal.text(Audience::Agent),
            ));
        }
        _ => anyhow::bail!("unexpected move result"),
    };
    society::leaving::lapse(db, &agent.hamlet, agent.number)?;
    for table in [
        "harness_binding",
        "harness_agent",
        "chat",
        "chat_session",
        "chat_conversation",
        "chat_instruction",
        "society_peer",
    ] {
        db.execute(
            &format!(
                "UPDATE {table} SET hamlet=?3, number=?4 WHERE \
                 hamlet=?1 AND number=?2"
            ),
            params![
                agent.hamlet,
                i64::try_from(agent.number)?,
                moved.hamlet,
                i64::try_from(moved.number)?
            ],
        )?;
    }
    db.execute(
        "UPDATE society_peer SET from_hamlet=?3, from_number=?4 \
         WHERE from_hamlet=?1 AND from_number=?2",
        params![
            agent.hamlet,
            i64::try_from(agent.number)?,
            moved.hamlet,
            i64::try_from(moved.number)?
        ],
    )?;
    db.execute(
        "UPDATE society_binding SET payer=?1 WHERE binding IN \
         (SELECT binding FROM society_peer WHERE from_hamlet=?1 AND \
         from_number=?2)",
        params![moved.hamlet, i64::try_from(moved.number)?],
    )?;
    realm.roads.retain(|r| {
        r.a != End::Agent(agent.clone())
            && r.b != End::Agent(agent.clone())
    });
    for number in keep {
        if number != agent.number
            && old.agents.iter().any(|a| a.number == number)
        {
            realm.roads.push(Road {
                a: End::Agent(AgentRef {
                    hamlet: agent.hamlet.clone(),
                    number,
                }),
                b: End::Agent(moved.clone()),
                agreed: vec![
                    agent.hamlet.clone(),
                    moved.hamlet.clone(),
                ],
                reason: reason.into(),
                messages: 0,
                last_from: None,
                last_used: 0,
            });
        }
    }
    realm.moves.retain(|m| m.agent != agent);
    realm.pending = Some(Pending {
        from: agent,
        to: moved,
    });
    write(db, realm)?;
    Ok(SocietyOutcome::answered(
        "The agent moved with its house and agreed roads.",
    ))
}

pub(crate) fn finish(service: &mut Service) -> Result<()> {
    let mut realm = read(service.db())?;
    if let Some(pending) = &realm.pending {
        service.houses.move_house(&pending.from, &pending.to)?;
        realm.pending = None;
        write(service.db(), &realm)?;
    }
    Ok(())
}
