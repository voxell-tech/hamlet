//! Admission, departure, restoration and moves between records.

use anyhow::{Context, Result};
use rusqlite::Connection;

use super::{
    Action, Actor, Admit, Agent, AgentRef, Archived, DecisionKind,
    Departure, End, Hamlet, InheritedRoad, Merge, Reason,
    RecordOperation, RecordResult, Refusal, RoadRefusal, names, read,
    save, tidy_name,
};

impl Hamlet {
    fn room_for(&self, operation: Action) -> Option<Refusal> {
        (self
            .agents
            .iter()
            .filter(|a| a.number != self.mayor)
            .count()
            >= self.limits.chief.agent_room())
        .then(|| {
            Refusal::new(
                operation,
                &self.id,
                Reason::HamletFull {
                    most: self.limits.chief.agent_room(),
                },
            )
        })
    }

    fn ordinary(&self, name: &str) -> Result<Agent, Reason> {
        if let Some(agent) = self.named(name) {
            return if agent.number == self.mayor {
                Err(Reason::MayorUntouchable)
            } else {
                Ok(agent.clone())
            };
        }
        Err(self
            .archive
            .iter()
            .rfind(|a| a.agent.name == name && a.back.is_none())
            .map_or_else(
                || Reason::AgentMissing {
                    name: name.into(),
                    local: false,
                },
                |a| Reason::AgentDeparted {
                    farewell: self.farewell(a),
                },
            ))
    }
}

impl RecordOperation {
    pub(super) fn admit(
        &self,
        db: &Connection,
        r: &mut Hamlet,
        given: &Admit,
    ) -> Result<Result<RecordResult, Refusal>> {
        let name = tidy_name(&given.name);
        if let Some(reason) = names::available(db, &name, None)? {
            return Ok(Err(self.refuse(reason)));
        }
        if matches!(self.actor, Actor::Mayor(_))
            && given.purpose.trim().is_empty()
        {
            return Ok(Err(self.refuse(Reason::PurposeEmpty)));
        }
        if let Some(refusal) = r.room_for(Action::Admit) {
            return Ok(Err(refusal));
        }
        let number = r.next;
        r.next = r
            .next
            .checked_add(1)
            .context("agent number exhausted")?;
        let model = given.model.clone().filter(|m| {
            !m.is_empty() && r.limits.chief.models.contains(m)
        });
        let purpose = given.purpose.trim().to_string();
        r.agents.push(Agent {
            number,
            name: name.clone(),
            purpose: purpose.clone(),
            since: self.at,
            model: model.clone(),
            character: given.character.clone(),
        });
        self.decision(
            r,
            DecisionKind::Created {
                agent: number,
                name: name.clone(),
                purpose,
            },
            true,
        );
        if let Some(model) = model {
            self.decision(
                r,
                DecisionKind::Model {
                    agent: number,
                    model,
                },
                true,
            );
        }
        let mut road_refusals = Vec::new();
        for peer in &given.roads {
            if let Err(refusal) = self.lay_road(db, r, &name, peer)? {
                road_refusals.push(RoadRefusal {
                    peer: peer.clone(),
                    refusal,
                });
            }
        }
        Ok(Ok(RecordResult::Admitted {
            agent: AgentRef {
                hamlet: r.id.clone(),
                number,
            },
            road_refusals,
        }))
    }

    fn unprotected(
        &self,
        r: &Hamlet,
        agent: &Agent,
    ) -> Result<(), Refusal> {
        if matches!(self.actor, Actor::Chief(_)) {
            return Ok(());
        }
        if let Some(p) = r.protected.iter().find(|p| {
            p.agent == agent.number
                && p.done.is_none_or(|done| {
                    self.at.saturating_sub(done).max(0)
                        < r.limits.chief.protected_ms()
                })
        }) {
            return Err(self.refuse(Reason::Protected {
                name: agent.name.clone(),
                chief: p.chief.clone(),
                text: p.text.clone(),
                done: p.done,
            }));
        }
        Ok(())
    }

    pub(super) fn depart(
        &self,
        r: &mut Hamlet,
        name: &str,
        how: &Departure,
    ) -> Result<RecordResult, Refusal> {
        let kept = if let Departure::Merged {
            into, purpose, ..
        } = how
        {
            let target = r
                .agents
                .iter()
                .find(|a| a.number == *into)
                .ok_or_else(|| {
                    self.refuse(Reason::ProposedKeptMissing)
                })?;
            if target.name == name {
                return Err(self.refuse(Reason::MergeSelf));
            }
            let kept = r
                .ordinary(&target.name)
                .map_err(|reason| self.refuse(reason))?;
            let gone = r
                .ordinary(name)
                .map_err(|reason| self.refuse(reason))?;
            if purpose.as_ref().is_some_and(|p| p.trim().is_empty()) {
                return Err(self.refuse(Reason::MergedPurposeEmpty));
            }
            if self.reason.trim().is_empty() {
                return Err(self.refuse(Reason::Empty));
            }
            self.unprotected(r, &kept)?;
            self.unprotected(r, &gone)?;
            Some(kept)
        } else {
            None
        };
        let agent =
            r.ordinary(name).map_err(|reason| self.refuse(reason))?;
        if self.reason.trim().is_empty() {
            return Err(self.refuse(Reason::Empty));
        }
        self.unprotected(r, &agent)?;
        let mut stamp = i128::from(self.at);
        while r
            .archive
            .iter()
            .any(|a| a.id == format!("{name}-{stamp}"))
        {
            stamp += 1;
        }
        let mut entry = Archived {
            id: format!("{name}-{stamp}"),
            agent: agent.clone(),
            at: self.at,
            by: self.actor.by(),
            reason: self.reason.trim().into(),
            roads: r
                .roads_of()
                .into_iter()
                .filter(|r| r.has_agent(agent.number))
                .collect(),
            merge: None,
            back: None,
        };
        r.agents.retain(|a| a.number != agent.number);
        r.roads.retain(|road| !road.has_agent(agent.number));
        if let (
            Departure::Merged {
                purpose,
                notes,
                files,
                ..
            },
            Some(kept),
        ) = (how, kept)
        {
            let mut inherited = Vec::new();
            for road in &entry.roads {
                let peer = if road.a == End::Agent(agent.number) {
                    road.b
                } else {
                    road.a
                };
                if peer == End::Agent(kept.number) {
                    continue;
                }
                let existing = r.roads.iter_mut().find(|r| {
                    r.is_between(End::Agent(kept.number), peer)
                });
                let fresh = existing.is_none();
                let last_from = road.last_from.map(|end| {
                    if end == End::Agent(agent.number) {
                        End::Agent(kept.number)
                    } else {
                        end
                    }
                });
                match existing {
                    None => {
                        let mut road = road.clone();
                        road.a = End::Agent(kept.number);
                        road.b = peer;
                        road.last_from = last_from;
                        r.roads.push(road);
                    }
                    Some(existing) => {
                        existing.messages = existing
                            .messages
                            .saturating_add(road.messages);
                        if road.last_used > existing.last_used {
                            existing.last_used = road.last_used;
                            existing.last_from = last_from;
                        }
                    }
                }
                inherited.push(InheritedRoad {
                    peer,
                    messages: road.messages,
                    fresh,
                });
            }
            let purpose = purpose.as_ref().map_or_else(
                || kept.purpose.clone(),
                |p| p.trim().into(),
            );
            for living in &mut r.agents {
                if living.number == kept.number {
                    living.purpose = purpose.clone();
                }
            }
            self.decision(
                r,
                DecisionKind::Merged {
                    keep: kept.number,
                    absorb: agent.number,
                    purpose,
                    notes: *notes,
                },
                true,
            );
            entry.merge = Some(Merge {
                kept,
                files: files.clone(),
                inherited,
                applied: false,
            });
        } else {
            self.decision(
                r,
                DecisionKind::Removed {
                    agent: agent.number,
                    name: name.into(),
                    archive: entry.id.clone(),
                },
                true,
            );
        }
        let archive = entry.id.clone();
        r.archive.push(entry);
        Ok(RecordResult::Departed { archive })
    }

    pub(super) fn restore(
        &self,
        db: &Connection,
        r: &mut Hamlet,
        archive: &str,
        name: &str,
    ) -> Result<Result<RecordResult, Refusal>> {
        let Some(index) = r
            .archive
            .iter()
            .position(|a| a.id == archive && a.back.is_none())
        else {
            return Ok(Err(self.refuse(Reason::ArchiveMissing {
                archive: archive.into(),
            })));
        };
        let mut entry = r.archive[index].clone();
        if let Some(merge) = &entry.merge {
            if !merge.applied {
                return Ok(Err(self.refuse(Reason::MergeUnfinished)));
            }
            let Some(kept) = r
                .agents
                .iter()
                .find(|a| a.number == merge.kept.number)
            else {
                return Ok(Err(self.refuse(Reason::KeptDeparted {
                    kept: merge.kept.name.clone(),
                    absorbed: entry.agent.name.clone(),
                })));
            };
            if let Some(later) =
                r.archive[index + 1..].iter().find(|a| {
                    a.back.is_none()
                        && a.merge.as_ref().is_some_and(|m| {
                            m.kept.number == kept.number
                        })
                })
            {
                return Ok(Err(self.refuse(Reason::LaterMerge {
                    later: later.agent.name.clone(),
                    kept: kept.name.clone(),
                    absorbed: entry.agent.name.clone(),
                })));
            }
        }
        if let Some(reason) =
            names::available(db, name, Some(&entry))?
        {
            return Ok(Err(self.refuse(reason)));
        }
        if let Some(refusal) = r.room_for(Action::Restore) {
            return Ok(Err(refusal));
        }
        if let Some(merge) = &mut entry.merge {
            for kept in &mut r.agents {
                if kept.number == merge.kept.number {
                    kept.purpose = merge.kept.purpose.clone();
                }
            }
            for inherited in &merge.inherited {
                if let Some(index) = r.roads.iter().position(|r| {
                    r.is_between(
                        End::Agent(merge.kept.number),
                        inherited.peer,
                    )
                }) {
                    let road = &mut r.roads[index];
                    road.messages = road
                        .messages
                        .saturating_sub(inherited.messages);
                    if inherited.fresh && road.messages == 0 {
                        r.roads.remove(index);
                    }
                }
            }
            merge.applied = false;
        }
        let mut agent = entry.agent.clone();
        agent.name = name.into();
        r.agents.push(agent.clone());
        for road in &entry.roads {
            if [road.a, road.b].iter().all(|end| match end {
                End::Office => true,
                End::Agent(n) => {
                    r.agents.iter().any(|a| a.number == *n)
                }
            }) && !r
                .roads
                .iter()
                .any(|r| r.is_between(road.a, road.b))
            {
                r.roads.push(road.clone());
            }
        }
        entry.back = Some(name.into());
        self.decision(
            r,
            DecisionKind::Restored {
                agent: agent.number,
                name: name.into(),
                archive: entry.id.clone(),
            },
            false,
        );
        let peers = r
            .roads_of()
            .iter()
            .filter(|road| road.has_agent(agent.number))
            .map(|road| {
                r.end_name(if road.a == End::Agent(agent.number) {
                    road.b
                } else {
                    road.a
                })
            })
            .collect::<Vec<_>>()
            .join(", ");
        let built = r
            .roads_of()
            .iter()
            .filter(|road| {
                road.a != End::Office && road.b != End::Office
            })
            .count();
        let excess = if built > r.limits.chief.road_room() {
            format!(
                " The hamlet now has {built} roads between agents, \
                 more than the {} your limits allow. They all work; \
                 the mayor can build no more until you raise \
                 mostRoads in the limits.",
                r.limits.chief.road_room()
            )
        } else {
            String::new()
        };
        let text = if let Some(merge) = &entry.merge {
            let kept = r
                .agents
                .iter()
                .find(|a| a.number == merge.kept.number)
                .unwrap_or(&merge.kept);
            let kept = &kept.name;
            format!(
                "Split `{name}` from `{kept}`. `{name}` has its \
                 house as it was before the merge, and `{kept}` its \
                 old purpose and whatever it has learned since. \
                 `{name}`'s roads lead to: {peers}.{excess}"
            )
        } else {
            format!(
                "Restored `{name}` with its house as it was. Its \
                 roads lead to: {peers}.{excess}"
            )
        };
        r.archive[index] = entry;
        Ok(Ok(RecordResult::Restored { agent, text }))
    }

    pub(super) fn erase(
        &self,
        r: &mut Hamlet,
        archive: &str,
    ) -> Result<RecordResult, Refusal> {
        let Some(index) = r
            .archive
            .iter()
            .position(|a| a.id == archive && a.back.is_none())
        else {
            return Err(self.refuse(Reason::ArchiveMissing {
                archive: archive.into(),
            }));
        };
        let entry = r.archive.remove(index);
        self.decision(
            r,
            DecisionKind::Erased {
                agent: entry.agent.number,
                name: entry.agent.name,
                archive: entry.id,
            },
            false,
        );
        Ok(RecordResult::Changed)
    }

    pub(super) fn set_model(
        &self,
        r: &mut Hamlet,
        name: &str,
        model: Option<&str>,
        allowed: &[String],
        pinned: &[String],
        own: &str,
    ) -> Result<RecordResult, Refusal> {
        let Some(agent) =
            r.named(name).filter(|a| a.number != r.mayor).cloned()
        else {
            return Err(self.refuse(Reason::AgentMissing {
                name: name.into(),
                local: false,
            }));
        };
        if pinned.contains(&agent.name) {
            return Err(
                self.refuse(Reason::ModelPinned { name: agent.name })
            );
        }
        if let Some(model) = model
            && !allowed.iter().any(|m| m == model)
        {
            return Err(self.refuse(Reason::ModelNotAllowed {
                model: model.into(),
                allowed: allowed.to_vec(),
            }));
        }
        for living in &mut r.agents {
            if living.number == agent.number {
                living.model = model.map(String::from);
            }
        }
        self.decision(
            r,
            DecisionKind::Model {
                agent: agent.number,
                model: model.unwrap_or(own).into(),
            },
            true,
        );
        Ok(RecordResult::Changed)
    }

    pub(super) fn move_member(
        &self,
        db: &Connection,
        old: &mut Hamlet,
        number: u64,
        to: &str,
    ) -> Result<Result<RecordResult, Refusal>> {
        if number == old.mayor {
            return Ok(Err(self.refuse(Reason::MoveMayor)));
        }
        let Some(mut agent) =
            old.agents.iter().find(|a| a.number == number).cloned()
        else {
            return Ok(Err(self.refuse(Reason::AgentMissing {
                name: number.to_string(),
                local: false,
            })));
        };
        if to == old.id {
            return Ok(Err(self.refuse(Reason::MoveAlreadyThere {
                name: agent.name,
            })));
        }
        let mut target = read(db, to)?
            .context("authorized move target missing")?;
        if let Some(refusal) = target.room_for(Action::Move) {
            return Ok(Err(refusal));
        }
        if let Some(merged) = old.archive.iter().find(|a| {
            a.back.is_none()
                && a.merge
                    .as_ref()
                    .is_some_and(|m| m.kept.number == number)
        }) {
            return Ok(Err(self.refuse(Reason::UnsplitMerge {
                name: agent.name,
                absorbed: merged.agent.name.clone(),
                archive: merged.id.clone(),
            })));
        }
        let next = target.next;
        target.next = target
            .next
            .checked_add(1)
            .context("agent number exhausted")?;
        agent.number = next;
        let name = agent.name.clone();
        target.agents.push(agent);
        old.agents.retain(|a| a.number != number);
        old.roads.retain(|r| !r.has_agent(number));
        for mut protection in
            old.protected.extract_if(.., |p| p.agent == number)
        {
            protection.agent = next;
            target.protected.push(protection);
        }
        if let Some(looks) = old.worn.remove(&number) {
            target.worn.insert(next, looks);
        }
        self.decision(
            old,
            DecisionKind::MovedOut {
                agent: number,
                name: name.clone(),
                hamlet: to.into(),
            },
            false,
        );
        self.decision(
            &mut target,
            DecisionKind::MovedIn {
                agent: next,
                name,
                hamlet: old.id.clone(),
            },
            false,
        );
        // Release the source name before inserting the receiving
        // membership, in this same transaction.
        save(db, old)?;
        save(db, &target)?;
        Ok(Ok(RecordResult::Moved(AgentRef {
            hamlet: to.into(),
            number: next,
        })))
    }
}
