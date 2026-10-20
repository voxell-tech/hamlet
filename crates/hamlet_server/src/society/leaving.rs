//! Departure, restoration and the chief's account of the record.

use anyhow::Result;
use rusqlite::{Connection, params};
use serde_json::Value;

use super::{Caller, Deed, outcome};
use crate::harness::Harnesses;
use crate::protocol::Refusal;
use crate::record::{
    self, Actor, AgentRef, Audience, Change, Decision, DecisionKind,
    Departure, NameFallback, RecordOperation, RecordResult,
};
use crate::service::Service;
use crate::{Outcome, society};

pub(super) fn guards(db: &Connection, hamlet: &str) -> Result<()> {
    let Some(mut r) = record::read(db, hamlet)? else {
        return Ok(());
    };
    r.protected = db
        .prepare(
            "SELECT number, chief, text, done FROM chat_instruction \
             WHERE hamlet = ?1",
        )?
        .query_map([hamlet], |row| {
            Ok(record::Protection {
                agent: row.get::<_, i64>(0)? as u64,
                chief: row.get(1)?,
                text: row.get(2)?,
                done: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    record::write(db, &r)
}

fn removed(r: &record::Hamlet, archive: &str) -> Value {
    let entry = r
        .archive
        .iter()
        .find(|a| a.id == archive)
        .expect("recorded departure");
    let mut result = outcome(
        &format!(
            "Removed `{}`. Its house is archived as {} and its \
             roads are gone. A chief can restore it.",
            entry.agent.name, entry.id
        ),
        false,
    );
    result["number"] = entry.agent.number.into();
    result
}

pub(super) async fn remove(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    args: &Value,
) -> Result<Value> {
    let agent = &caller.place.agent;
    let value = society::once(
        service.home.store(),
        &caller.id,
        &caller.request,
        |db| {
            guards(db, &agent.hamlet)?;
            let change = RecordOperation {
                hamlet: agent.hamlet.clone(),
                actor: society::actor(db, agent)?,
                at: society::now(),
                reason: args["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                task: caller.task.clone(),
                change: Change::Depart {
                    name: args["name"]
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                    how: Departure::Removed,
                },
            };
            Ok(match crate::operation::change(db, &change)? {
                Outcome::Done(RecordResult::Departed { archive }) => {
                    removed(
                        &record::read(db, &agent.hamlet)?
                            .expect("departed record"),
                        &archive,
                    )
                }
                Outcome::Refused(refusal) => {
                    outcome(&refusal.text(Audience::Agent), true)
                }
                Outcome::Done(_) => {
                    anyhow::bail!("unexpected departure result")
                }
            })
        },
    )?;
    if let Some(number) = value["number"].as_u64() {
        crate::chats::stop_agent(
            service,
            harnesses,
            &AgentRef {
                hamlet: agent.hamlet.clone(),
                number,
            },
        )
        .await?;
        lapse(
            service.home.store().connection(),
            &agent.hamlet,
            number,
        )?;
    }
    Ok(value)
}

pub(crate) fn lapse(
    db: &Connection,
    hamlet: &str,
    number: u64,
) -> Result<()> {
    let agent = AgentRef {
        hamlet: hamlet.into(),
        number,
    };
    for permission in ["network", "unconfined"] {
        crate::gate::revoke(
            db,
            &agent,
            permission,
            "departure",
            society::now(),
        )?;
    }
    db.execute(
        "DELETE FROM gate_trust WHERE hamlet = ?1 AND number = ?2",
        params![hamlet, i64::try_from(number)?],
    )?;
    Ok(())
}

pub(super) async fn deed(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    deed: &str,
    rest: &str,
) -> Result<Option<Deed>> {
    if !["remove", "restore", "undo", "erase", "record"]
        .contains(&deed)
    {
        return Ok(None);
    }
    let mut r = match service.access(hamlet, deed)? {
        Ok(r) => r,
        Err(refusal) => return Ok(Some(Err(refusal))),
    };
    if deed == "record" {
        return Ok(Some(Ok(history(&r))));
    }
    let mut words = rest.split_whitespace();
    let which = words.next().unwrap_or_default();
    let reason = words.collect::<Vec<_>>().join(" ");
    let mut op = RecordOperation {
        hamlet: hamlet.into(),
        actor: Actor::Chief(service.chief.clone()),
        at: society::now(),
        reason: if reason.is_empty() {
            "a chief asked".into()
        } else {
            reason
        },
        task: None,
        change: Change::Depart {
            name: which.into(),
            how: Departure::Removed,
        },
    };
    let entry = r
        .archive
        .iter()
        .rev()
        .find(|a| {
            a.back.is_none()
                && (deed == "undo"
                    || a.id == which
                    || a.agent.name == which)
        })
        .cloned();
    if deed != "remove" {
        let Some(entry) = entry else {
            let text = match deed {
                "undo" => {
                    return Ok(Some(Ok("Nothing to undo. Only a \
                                       removal or a merge can \
                                       be undone (the archive \
                                       keeps the house); a \
                                       permission is taken back \
                                       with /revoke. A command \
                                       that ran, a file that \
                                       was changed and what was \
                                       spent cannot be undone."
                        .into())));
                }
                "erase" => format!(
                    "nothing is archived under `{which}`. Only an \
                     agent that was removed or merged can be \
                     deleted for good; /remove it first"
                ),
                _ => format!(
                    "nothing is archived under `{which}`; /record \
                     lists what is"
                ),
            };
            return Ok(Some(Err(Refusal::Invalid(text))));
        };
        if deed == "erase" {
            if entry.merge.as_ref().is_some_and(|m| !m.applied) {
                return Ok(Some(Err(Refusal::Invalid(
                    "The merge has not finished; finish it before \
                     erasing its house."
                        .into(),
                ))));
            }
            crate::chats::stop_agent(
                service,
                harnesses,
                &AgentRef {
                    hamlet: hamlet.into(),
                    number: entry.agent.number,
                },
            )
            .await?;
            r.archive.retain(|a| a.id != entry.id);
            r.decisions.push(Decision {
                at: op.at,
                by: format!("chief:{}", service.chief),
                reason: String::new(),
                kind: DecisionKind::Erased {
                    agent: entry.agent.number,
                    name: entry.agent.name.clone(),
                    archive: entry.id,
                },
            });
            record::write(service.home.store().connection(), &r)?;
            crate::fault::point("after_agent_erasure");
            service.houses.erase(hamlet, entry.agent.number)?;
            return Ok(Some(Ok(format!(
                "Deleted `{}` for good. It cannot be restored.",
                entry.agent.name
            ))));
        }
        let rename = rest.split_whitespace().collect::<Vec<_>>();
        let name = if rename.get(1) == Some(&"as") {
            rename.get(2).copied().unwrap_or_default().into()
        } else {
            entry.agent.name
        };
        op.change = Change::Restore {
            archive: entry.id,
            name,
        };
    }
    let tx = service.home.store().transaction()?;
    let result = crate::operation::change(&tx, &op)?;
    tx.commit()?;
    let result = match result {
        Outcome::Done(RecordResult::Departed { archive }) => {
            let r = service
                .home
                .store()
                .record(hamlet)?
                .expect("departed record");
            let value = removed(&r, &archive);
            let number =
                value["number"].as_u64().expect("departure number");
            crate::chats::stop_agent(
                service,
                harnesses,
                &AgentRef {
                    hamlet: hamlet.into(),
                    number,
                },
            )
            .await?;
            lapse(service.home.store().connection(), hamlet, number)?;
            Ok(value["text"].as_str().unwrap_or_default().into())
        }
        Outcome::Done(RecordResult::Restored { text, .. }) => {
            if service.houses.workspace.is_none() {
                super::merging::finish(service).await?;
            }
            Ok(text)
        }
        Outcome::Refused(refusal) => {
            Err(Refusal::Invalid(refusal.text(Audience::Chief)))
        }
        Outcome::Done(_) => {
            anyhow::bail!("unexpected lifecycle result")
        }
    };
    Ok(Some(result))
}

pub(crate) async fn recover(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let live = crate::realm::live(service.home.store().connection())?;
    for r in
        crate::service::records(service.home.store().connection())?
    {
        let mut retired = r
            .archive
            .iter()
            .filter(|entry| entry.back.is_none())
            .map(|entry| entry.agent.number)
            .collect::<Vec<_>>();
        if !live.contains(&r.id) {
            retired.extend(r.agents.iter().map(|agent| agent.number));
        }
        for decision in &r.decisions {
            if let DecisionKind::Erased { agent, .. } = decision.kind
            {
                retired.push(agent);
                service.houses.erase(&r.id, agent)?;
            }
        }
        for number in retired {
            let agent = AgentRef {
                hamlet: r.id.clone(),
                number,
            };
            crate::chats::stop_agent(service, harnesses, &agent)
                .await?;
            for binding in crate::chats::records::conversations_of(
                service.home.store().connection(),
                &agent,
            )? {
                harnesses
                    .cancel(service.home.store(), binding)
                    .await?;
            }
            lapse(service.home.store().connection(), &r.id, number)?;
        }
    }
    Ok(())
}

fn history(r: &record::Hamlet) -> String {
    let name = |n| r.name_of(n, NameFallback::Recorded);
    let end = |e| match e {
        record::End::Office => "mayor".into(),
        record::End::Agent(n) => name(n),
    };
    let mut lines = r
        .decisions
        .iter()
        .map(|d| {
            let by = if d.by == "mayor" {
                "the mayor".into()
            } else if let Some(chief) = d.by.strip_prefix("chief:") {
                format!("the chief {chief}")
            } else {
                d.by.clone()
            };
            let text = match &d.kind {
                DecisionKind::Created { name, purpose, .. } => {
                    format!("the mayor created `{name}` ({purpose})")
                }
                DecisionKind::Removed { name, .. } => {
                    format!("{by} removed `{name}`")
                }
                DecisionKind::Restored { name, .. } => {
                    format!("{by} restored `{name}`")
                }
                DecisionKind::Erased { name, .. } => {
                    format!("{by} deleted `{name}` for good")
                }
                DecisionKind::RoadBuilt { a, b } => format!(
                    "{by} built a road between `{}` and `{}`",
                    end(*a),
                    end(*b)
                ),
                DecisionKind::RoadRemoved { a, b } => format!(
                    "{by} took up the road between `{}` and `{}`",
                    end(*a),
                    end(*b)
                ),
                DecisionKind::Merged { keep, absorb, .. } => format!(
                    "{by} merged `{}` into `{}`",
                    name(*absorb),
                    name(*keep)
                ),
                DecisionKind::MayorChanged { from, to } => format!(
                    "`{}` became mayor in place of `{}`",
                    name(*to),
                    name(*from)
                ),
                DecisionKind::Model { agent, model } => {
                    format!("{by} set `{}` to {model}", name(*agent))
                }
                DecisionKind::SkillGiven { to, skill, from } => {
                    format!(
                        "{by} gave `{}` the skill `{skill}`{}",
                        name(*to),
                        from.as_ref()
                            .map(|from| format!(" of `{from}`"))
                            .unwrap_or_default()
                    )
                }
                DecisionKind::MovedIn { name, hamlet, .. } => {
                    format!("{by} moved `{name}` from `{hamlet}`")
                }
                DecisionKind::MovedOut { name, hamlet, .. } => {
                    format!("{by} moved `{name}` to `{hamlet}`")
                }
                DecisionKind::RoadRefused { from, to } => format!(
                    "The mayor refused the road from `{}` to `{to}`",
                    name(*from)
                ),
            };
            format!("{text}: {}", d.reason)
        })
        .collect::<Vec<_>>();
    for entry in r.archive.iter().filter(|a| a.back.is_none()) {
        lines.push(format!(
            "{}: {} /restore {}",
            entry.id,
            r.farewell(entry),
            entry.id
        ));
    }
    if lines.is_empty() {
        "The record is empty.".into()
    } else {
        lines.join("\n")
    }
}
