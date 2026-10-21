//! Local roads, office roads and their atomic charges.

use anyhow::{Result, ensure};
use hamlet_protocol::v0::RoadLevel;
use rusqlite::{Connection, params};

use super::{
    Action, By, DecisionKind, End, Hamlet, Reason, RecordOperation,
    RecordResult, Refusal, Road,
};
use crate::spend::money;

impl Hamlet {
    /// Active roads, including each ordinary agent's implied road to
    /// the office.
    pub fn roads_of(&self) -> Vec<Road> {
        let lives = |end| match end {
            End::Office => true,
            End::Agent(n) => {
                n != self.mayor
                    && self.agents.iter().any(|a| a.number == n)
            }
        };
        let mut roads = self
            .roads
            .iter()
            .filter(|r| lives(r.a) && lives(r.b))
            .cloned()
            .collect::<Vec<_>>();
        for agent in &self.agents {
            if agent.number != self.mayor
                && !roads.iter().any(|r| {
                    r.joins(End::Office, End::Agent(agent.number))
                })
            {
                roads.push(Road::new(
                    End::Office,
                    End::Agent(agent.number),
                    agent.since,
                    By::Office,
                ));
            }
        }
        roads
    }

    /// A road between two living agents.
    pub fn has_road(&self, a: u64, b: u64) -> bool {
        let end = |n| {
            if n == self.mayor {
                End::Office
            } else {
                End::Agent(n)
            }
        };
        a != b
            && self.roads_of().iter().any(|r| r.joins(end(a), end(b)))
    }
}

/// The drawn level at `now` of a road that carried `messages` and
/// was last used at `last_used` (H3).
pub fn level(
    last_used: i64,
    messages: u64,
    now: i64,
    grows_over_days: i64,
) -> RoadLevel {
    if last_used > 0
        && now.saturating_sub(last_used)
            > crate::clock::days(grows_over_days)
    {
        RoadLevel::Overgrown
    } else if messages < 3 {
        RoadLevel::Dirt
    } else if messages < 10 {
        RoadLevel::Paved
    } else {
        RoadLevel::Rail
    }
}

impl RecordOperation {
    pub(super) fn lay_road(
        &self,
        db: &Connection,
        r: &mut Hamlet,
        a: &str,
        b: &str,
    ) -> Result<Result<Road, Refusal>> {
        // Admission roads have their own operation wording.
        let refuse = |reason| {
            Err(Refusal::new(Action::LayRoad, &r.id, reason))
        };
        let aa = r.named(a);
        let bb = r.named(b);
        if aa.is_some_and(|a| a.number == r.mayor)
            || bb.is_some_and(|b| b.number == r.mayor)
        {
            return Ok(refuse(Reason::OfficeRoad));
        }
        let Some(aa) = aa else {
            return Ok(refuse(Reason::AgentMissing {
                name: a.into(),
                local: true,
            }));
        };
        let Some(bb) = bb else {
            return Ok(refuse(Reason::AgentMissing {
                name: b.into(),
                local: true,
            }));
        };
        let (aa, bb) = (End::Agent(aa.number), End::Agent(bb.number));
        if aa == bb {
            return Ok(refuse(Reason::RoadSelf));
        }
        if r.roads.iter().any(|r| r.joins(aa, bb)) {
            return Ok(refuse(Reason::RoadExists {
                a: a.into(),
                b: b.into(),
            }));
        }
        let built = r
            .roads_of()
            .iter()
            .filter(|r| r.a != End::Office && r.b != End::Office)
            .count();
        if built >= r.limits.chief.road_room() {
            return Ok(refuse(Reason::RoadsFull { most: built }));
        }
        if let Some(text) = self.road_afford(db, r)? {
            return Ok(refuse(Reason::RoadBudget { text }));
        }
        let price = r.limits.chief.road_price();
        db.execute(
            "INSERT INTO record_road_charge(hamlet, agent, task, \
             at, tokens, micros, ms) VALUES (?1, ?2, ?3, ?4, ?5, \
             ?6, ?7)",
            params![
                r.id,
                i64::try_from(r.mayor)?,
                self.task,
                self.at,
                i64::try_from(price.tokens)?,
                i64::try_from(price.micros)?,
                i64::try_from(price.ms)?
            ],
        )?;
        let road = Road::new(aa, bb, self.at, self.actor.by());
        r.roads.push(road.clone());
        self.decision(
            r,
            DecisionKind::RoadBuilt { a: aa, b: bb },
            true,
        );
        Ok(Ok(road))
    }

    fn road_afford(
        &self,
        db: &Connection,
        r: &Hamlet,
    ) -> Result<Option<String>> {
        let Some(budget) = &r.limits.road_budget else {
            return Ok(None);
        };
        // The spend owner must refresh its window and other spend
        // before work can continue.
        ensure!(
            (budget.start..budget.end).contains(&self.at),
            "the road budget window must be refreshed"
        );
        let (micros, ms) = db.query_row(
            "SELECT coalesce(sum(micros), 0), coalesce(sum(ms), 0) \
             FROM record_road_charge WHERE hamlet = ?1 AND at >= ?2 \
             AND at < ?3",
            params![r.id, budget.start, budget.end],
            |row| {
                Ok((
                    super::unsigned(row, 0)?,
                    super::unsigned(row, 1)?,
                ))
            },
        )?;
        let price = r.limits.chief.road_price();
        let cost_after = budget
            .other
            .micros
            .saturating_add(micros)
            .saturating_add(price.micros);
        let time_after = budget
            .other
            .ms
            .saturating_add(ms)
            .saturating_add(price.ms);
        let exceeded = if let Some(cap) =
            budget.cost.filter(|cap| cost_after > *cap)
        {
            Some((money(price.micros), money(cost_after), money(cap)))
        } else {
            budget.ms.filter(|cap| time_after > *cap).map(|cap| {
                (
                    format!("{} model time", duration(price.ms)),
                    format!("{} model time", duration(time_after)),
                    format!("{} model time", duration(cap)),
                )
            })
        };
        Ok(exceeded.map(|(price, after, cap)| {
            format!(
                "the hamlet cannot pay for it {}: it costs {price} \
                 and would bring the hamlet to {after} of {cap}. \
                 Only the chief can raise the budget, on the \
                 hamlet's Bylaws page",
                if budget.month { "this month" } else { "today" }
            )
        }))
    }

    pub(super) fn take_up_road(
        &self,
        r: &mut Hamlet,
        a: &str,
        b: &str,
    ) -> Result<Road, Refusal> {
        let aa = r.named(a);
        let bb = r.named(b);
        if aa.is_some_and(|a| a.number == r.mayor)
            || bb.is_some_and(|b| b.number == r.mayor)
        {
            return Err(self.refuse(Reason::OfficeRoad));
        }
        let index = r.roads.iter().position(|road| {
            aa.zip(bb).is_some_and(|(a, b)| {
                road.joins(End::Agent(a.number), End::Agent(b.number))
            })
        });
        let Some(index) = index else {
            return Err(self.refuse(Reason::RoadMissing {
                a: a.into(),
                b: b.into(),
            }));
        };
        let road = r.roads.remove(index);
        self.decision(
            r,
            DecisionKind::RoadRemoved {
                a: road.a,
                b: road.b,
            },
            true,
        );
        Ok(road)
    }

    pub(super) fn carry_road(
        &self,
        r: &mut Hamlet,
        from: u64,
        to: u64,
    ) -> Result<Road, Refusal> {
        if !r.has_road(from, to) {
            return Err(self.refuse(Reason::RoadMissing {
                a: r.address_of(from).into(),
                b: r.address_of(to).into(),
            }));
        }
        let a = if from == r.mayor {
            End::Office
        } else {
            End::Agent(from)
        };
        let b = if to == r.mayor {
            End::Office
        } else {
            End::Agent(to)
        };
        let index =
            match r.roads.iter().position(|road| road.joins(a, b)) {
                Some(index) => index,
                None => {
                    let road = r
                        .roads_of()
                        .into_iter()
                        .find(|road| road.joins(a, b))
                        .expect("checked active road");
                    r.roads.push(road);
                    r.roads.len() - 1
                }
            };
        let road = &mut r.roads[index];
        road.messages = road.messages.saturating_add(1);
        road.last_from = Some(a);
        road.last_used = self.at;
        Ok(road.clone())
    }

    pub(super) fn seat(
        &self,
        r: &mut Hamlet,
        number: u64,
    ) -> Result<RecordResult, Refusal> {
        let Some(agent) =
            r.agents.iter().find(|a| a.number == number)
        else {
            return Err(self.refuse(Reason::ProposedMayorMissing));
        };
        if r.mayor == number {
            return Err(self.refuse(Reason::AlreadyMayor {
                name: agent.name.clone(),
            }));
        }
        let from = r.mayor;
        r.mayor = number;
        if !r
            .roads
            .iter()
            .any(|road| road.joins(End::Office, End::Agent(from)))
        {
            r.roads.push(Road::new(
                End::Office,
                End::Agent(from),
                self.at,
                By::Office,
            ));
        }
        self.decision(
            r,
            DecisionKind::MayorChanged { from, to: number },
            false,
        );
        Ok(RecordResult::Changed)
    }
}

fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60)
    }
}
