//! Agreements between current mayors and recoverable house moves.

use anyhow::Result;
use rusqlite::params;
use serde_json::{Value, json};

use super::{
    Agreed, End, Move, Pending, Removal, Road, read, resolve, write,
};
use crate::Outcome;
use crate::harness::Harnesses;
use crate::record::{
    self, Actor, AgentRef, Audience, Change, RecordOperation,
    RecordResult,
};
use crate::service::Service;
use crate::society::{self, Caller, outcome};

pub(crate) async fn operation(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    tool: &str,
    args: &Value,
) -> Result<Option<Value>> {
    let from = &caller.place.agent;
    let limits = service.limits_of("home")?;
    let value = society::once(
        service.home.store(),
        &crate::OperationId {
            runtime: crate::RuntimeInstance::new("realm-agreements")?,
            key: serde_json::to_string(&(
                caller.id.runtime.as_str(),
                &caller.id.key,
            ))?,
        },
        tool,
        |db| {
            let mut realm = read(db)?;
            let own = record::read(db, &from.hamlet)?;
            if !realm.lives(&from.hamlet)
                || realm.live().count() < 2
                || own.as_ref().is_none_or(|r| r.mayor != from.number)
            {
                return Ok(outcome(
                    "Only a mayor of a living hamlet can do that.",
                    true,
                ));
            }
            if let Some(held) = own.as_ref().and_then(|r| {
                r.may_decide(society::now(), record::Action::Mayor)
            }) {
                return Ok(outcome(
                    &held.text(Audience::Agent),
                    true,
                ));
            }
            let reason =
                args["reason"].as_str().unwrap_or_default().trim();
            if reason.is_empty() {
                return Ok(outcome(
                    "say why in `reason`; the chief is shown it",
                    true,
                ));
            }
            if tool == "ask_removal" {
                let id = args["hamlet"].as_str().unwrap_or_default();
                if !realm.lives(id) {
                    return Ok(outcome(
                        &format!("no hamlet named `{id}`"),
                        true,
                    ));
                }
                realm.removals.push(Removal {
                    hamlet: id.into(),
                    by: from.hamlet.clone(),
                    reason: reason.into(),
                    at: society::now(),
                });
                let asked = realm
                    .removals
                    .iter()
                    .filter(|r| r.hamlet == id && realm.lives(&r.by))
                    .map(|r| r.by.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let ready = asked.contains(id)
                    || asked.len()
                        >= limits["hamlets"]["removalMayors"]
                            .as_u64()
                            .unwrap_or(3)
                            as usize;
                let text = format!(
                    "The mayor of `{}` asks for the hamlet `{id}` \
                     to be removed. {}",
                    from.hamlet,
                    if ready {
                        format!(
                            "Enough have asked: /remove-hamlet {id} \
                             confirms it."
                        )
                    } else {
                        "Not enough mayors have asked yet.".into()
                    }
                );
                write(db, &realm)?;
                let mut result = outcome(&text, false);
                result["notice"] = json!({"hamlet":id,"text":text});
                return Ok(result);
            }
            if tool == "agree_road" {
                if limits["hamlets"]["roadsBetween"] == false {
                    return Ok(outcome(
                        "The chief does not allow roads between \
                         hamlets.",
                        true,
                    ));
                }
                let mut ends = Vec::new();
                for field in ["a", "b"] {
                    let address =
                        args[field].as_str().unwrap_or_default();
                    match resolve(db, &from.hamlet, address)? {
                        Ok(resolved) => ends.push(resolved.agent),
                        Err(error) => {
                            return Ok(outcome(
                                &error.text(address),
                                true,
                            ));
                        }
                    }
                }
                let (a, b) = (&ends[0], &ends[1]);
                for end in [a, b] {
                    if record::read(db, &end.hamlet)?
                        .is_some_and(|r| r.mayor == end.number)
                    {
                        return Ok(outcome(
                            "The mayors already have an office \
                             road; agree to a road between other \
                             agents.",
                            true,
                        ));
                    }
                }
                if a.hamlet == b.hamlet
                    || (a.hamlet != from.hamlet
                        && b.hamlet != from.hamlet)
                {
                    return Ok(outcome(
                        "Agree to a road between your hamlet and \
                         another hamlet.",
                        true,
                    ));
                }
                let pair = |r: &Road| {
                    (r.a == End::Agent(a.clone())
                        && r.b == End::Agent(b.clone()))
                        || (r.a == End::Agent(b.clone())
                            && r.b == End::Agent(a.clone()))
                };
                if let Some(road) =
                    realm.roads.iter_mut().find(|r| pair(r))
                {
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
                write(db, &realm)?;
                let mut result = outcome(
                    "The road agreement is recorded; it works once \
                     both mayors agree.",
                    false,
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
                    result["ask"] = json!({"name":format!("mayor@{other}"),"message":format!("Please consider agreeing to a road between {}#{} and {}#{}. {reason}",a.hamlet,a.number,b.hamlet,b.number)});
                }
                return Ok(result);
            }
            let address = args["agent"].as_str().unwrap_or_default();
            let agent = match resolve(db, &from.hamlet, address)? {
                Ok(r) => r.agent,
                Err(error) => {
                    return Ok(outcome(&error.text(address), true));
                }
            };
            let to = args["to"].as_str().unwrap_or_default();
            if !realm.lives(to)
                || agent.hamlet == to
                || (from.hamlet != agent.hamlet && from.hamlet != to)
            {
                return Ok(outcome(
                    "A move needs the agent's mayor and the \
                     receiving mayor.",
                    true,
                ));
            }
            if realm.pending.is_some() {
                return Ok(outcome(
                    "A house move is still pending; finish it \
                     before another move.",
                    true,
                ));
            }
            let old = record::read(db, &agent.hamlet)?
                .expect("resolved agent");
            let target =
                record::read(db, to)?.expect("living target");
            let other =
                if from.hamlet == old.id { &target } else { &old };
            if let Some(held) =
                other.may_decide(society::now(), record::Action::Move)
            {
                return Ok(outcome(
                    &held.text(Audience::Agent),
                    true,
                ));
            }
            if agent.number == old.mayor {
                return Ok(outcome(
                    "A mayor cannot move to another hamlet.",
                    true,
                ));
            }
            let keep = args["keep_roads"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
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
                at: society::now(),
            });
            if moving.agreed.len() < 2 {
                write(db, &realm)?;
                let mut result = outcome(
                    "The move is proposed; the other mayor must \
                     agree.",
                    false,
                );
                let other = if from.hamlet == to {
                    &agent.hamlet
                } else {
                    to
                };
                result["ask"] = json!({"name":format!("mayor@{other}"),"message":format!("Please consider agreeing to move {address} to {to}. {reason}")});
                return Ok(result);
            }
            let keep = moving.agreed[0]
                .keep
                .iter()
                .filter(|n| moving.agreed[1].keep.contains(n))
                .copied()
                .collect::<Vec<_>>();
            let result = record::change(
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
                    at: society::now(),
                    reason: reason.into(),
                    task: caller.task.clone(),
                    change: Change::MoveMember {
                        number: agent.number,
                        to: to.into(),
                    },
                },
            )?;
            let moved = match result {
                Outcome::Done(RecordResult::Moved(moved)) => moved,
                Outcome::Refused(refusal) => {
                    return Ok(outcome(
                        &refusal.text(Audience::Agent),
                        true,
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
                        "UPDATE {table} SET hamlet=?3, number=?4 \
                         WHERE hamlet=?1 AND number=?2"
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
                "UPDATE society_peer SET from_hamlet=?3, \
                 from_number=?4 WHERE from_hamlet=?1 AND \
                 from_number=?2",
                params![
                    agent.hamlet,
                    i64::try_from(agent.number)?,
                    moved.hamlet,
                    i64::try_from(moved.number)?
                ],
            )?;
            db.execute(
                "UPDATE society_binding SET payer=?1 WHERE binding \
                 IN (SELECT binding FROM society_peer WHERE \
                 from_hamlet=?1 AND from_number=?2)",
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
            write(db, &realm)?;
            Ok(outcome(
                "The agent moved with its house and agreed roads.",
                false,
            ))
        },
    )?;
    if let Some(pending) =
        read(service.home.store().connection())?.pending
    {
        crate::chats::stop_agent(service, harnesses, &pending.to)
            .await?;
        for binding in crate::chats::records::conversations_of(
            service.home.store().connection(),
            &pending.to,
        )? {
            harnesses.cancel(service.home.store(), binding).await?;
        }
    }
    finish(service)?;
    if !value["ask"].is_null()
        && society::letters::ask(
            service,
            harnesses,
            caller,
            &value["ask"],
        )
        .await?
        .is_none()
    {
        return Ok(None);
    }
    if let Some(hamlet) = value["notice"]["hamlet"].as_str()
        && let Some(r) = service.home.store().record(hamlet)?
    {
        let binding = crate::chats::current(
            service,
            harnesses,
            &AgentRef {
                hamlet: hamlet.into(),
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
            value["notice"]["text"].as_str().unwrap_or_default(),
            false,
        )
        .await?;
    }
    society::reconfigure(service, harnesses).await?;
    Ok(Some(value))
}

pub(crate) fn finish(service: &mut Service) -> Result<()> {
    let mut realm = read(service.home.store().connection())?;
    if let Some(pending) = &realm.pending {
        service.houses.move_house(&pending.from, &pending.to)?;
        realm.pending = None;
        write(service.home.store().connection(), &realm)?;
    }
    Ok(())
}
