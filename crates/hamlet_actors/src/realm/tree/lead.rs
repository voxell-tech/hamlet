//! A lead's acts on the tree: hire, end and seat (H40, H5, H18).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U4 fills this file"
)]

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::{
    Done, End, EndRefusal, HamletId, Hire, HireAnswer, HireRefusal,
    HowEnded, Named, ProfileId, Refusable, Seat, SeatRefusal,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::{Decider, Lead, RealmWide};
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer, number};
use super::super::world::Part;
use super::super::{
    chiefs, hire, record, roads, steps, tasks, waits,
};
use super::{Change, Lineage, Member, Of, Status};

pub(in crate::realm) struct Hiring {
    /// The calling hire's hamlet, else the one named: `None` inside
    /// when the one named is missing.
    hamlet: Option<Option<HamletId>>,
    /// The lead named, else the calling hire, else the mayor.
    under: Option<Lineage>,
    /// How many living hires the lead's hamlet holds.
    living: u32,
    /// The profile named, when it is there.
    profile: Option<super::Profile>,
    agent: AgentNumber,
    blank: ProfileId,
}

impl<By: Lead> Act<By> for Hire {
    type Output = HireAnswer<By::Why<HireRefusal>>;
    type Loaded = Hiring;
    type Done = AgentNumber;

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Hiring> {
        let caller = caller(by);
        let own = own_hamlet(rows, caller).await?;
        let hamlet = match (&own, &self.hamlet) {
            (Some(own), _) => Some(Some(own.clone())),
            (None, Some(named)) => Some(
                chiefs::hamlet(rows, named)
                    .await?
                    .map(|hamlet| hamlet.id),
            ),
            (None, None) => None,
        };
        let lead = match (&self.under, own) {
            (None, Some(_)) => caller,
            (named, _) => {
                let named = named.as_ref().unwrap_or(&Named::Mayor);
                let hamlet = hamlet.as_ref().and_then(Option::as_ref);
                super::resolve(rows, hamlet, named).await?
            }
        };
        let under = match lead {
            Some(lead) => super::lineage(rows, lead).await?,
            None => None,
        };
        let living = match under
            .as_ref()
            .and_then(|under| under.hamlet.as_ref())
        {
            Some(hamlet) => super::living(rows, &hamlet.id).await?,
            None => 0,
        };
        let profile = match self.profile {
            Some(id) => super::profile(rows, id).await?,
            None => None,
        };
        Ok(Hiring {
            hamlet,
            under,
            living,
            profile,
            agent: super::fresh(rows).await?,
            blank: super::fresh_profile(rows).await?,
        })
    }

    fn decide(
        &self,
        by: &By,
        hiring: Hiring,
    ) -> Decision<AgentNumber, Self::Output> {
        let refuse = |why| {
            Decision::Answer(HireAnswer::refused(By::refused(why)))
        };
        let named = match hiring.hamlet {
            Some(None) => return refuse(HireRefusal::NoSuchHamlet),
            Some(Some(hamlet)) => Some(hamlet),
            None => None,
        };
        let Some(under) = hiring.under else {
            let under_the_mayor =
                matches!(self.under, None | Some(Named::Mayor));
            return refuse(if named.is_none() && under_the_mayor {
                HireRefusal::NoHamlet
            } else {
                HireRefusal::NoSuchAgent
            });
        };
        let lead = under.agent.agent;
        // The hire joins its lead's hamlet, which is the one named
        // when there is one; the Hand leads in none.
        let Some(hamlet) = under.hamlet.as_ref().filter(|hamlet| {
            named.as_ref().is_none_or(|named| *named == hamlet.id)
        }) else {
            return refuse(HireRefusal::NoSuchAgent);
        };
        if by.by() != record::By::Agent(lead)
            && let Err(not_a_lead) = by.leads(&under, lead)
        {
            return Decision::Answer(HireAnswer::refused(not_a_lead));
        }
        if let Status::Ended(left) = &under.agent.status {
            return refuse(HireRefusal::LeadLeft {
                lead,
                left: left.clone(),
            });
        }
        if hiring.living >= hamlet.most() {
            return refuse(HireRefusal::Full {
                hamlet: hamlet.id.clone(),
                most: hamlet.most(),
            });
        }
        let of = match (self.profile, hiring.profile) {
            (None, _) => Of::Blank(hiring.blank),
            (Some(_), Some(profile)) => Of::Profile(profile.id),
            (Some(profile), None) => {
                return refuse(HireRefusal::NoSuchProfile {
                    profile,
                });
            }
        };
        let agent = hiring.agent;
        Decision::Change(
            Decided::new(agent)
                .change(Change::Hire {
                    agent,
                    hamlet: hamlet.id.clone(),
                    lead: Some(lead),
                    of,
                    purpose: self.purpose.clone(),
                })
                .change(tasks::Change::Standing {
                    agent,
                    payer: chiefs::payer_of(&under),
                })
                .change(steps::Change::Begin {
                    row: hire::row(agent),
                })
                .record(record::Entry::Hired {
                    hire: agent,
                    by: by.by(),
                    why: self.purpose.clone(),
                }),
        )
    }

    fn answer(
        &self,
        agent: AgentNumber,
        _made: &Made,
    ) -> Self::Output {
        HireAnswer::Hired { agent }
    }
}

/// The calling agent, when an agent calls.
fn caller(by: &impl Decider) -> Option<AgentNumber> {
    match by.by() {
        record::By::Agent(agent) => Some(agent),
        record::By::Chief => None,
    }
}

/// The hamlet of `caller` when it is a hire.
async fn own_hamlet(
    rows: &Rows<'_>,
    caller: Option<AgentNumber>,
) -> Result<Option<HamletId>> {
    let Some(caller) = caller else {
        return Ok(None);
    };
    Ok(rows
        .value::<String>(
            "SELECT hamlet FROM hire WHERE agent = ?1",
            vec![number(caller)],
        )
        .await?
        .map(HamletId))
}

impl<By: Lead> Act<By> for End {
    type Output = Done<By::Why<EndRefusal>>;
    type Loaded = Option<Lineage>;
    type Done = ();

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Option<Lineage>> {
        let own = own_hamlet(rows, caller(by)).await?;
        named_lineage(rows, own.as_ref(), &self.hire).await
    }

    fn decide(
        &self,
        by: &By,
        lineage: Option<Lineage>,
    ) -> Decision<(), Self::Output> {
        let refuse =
            |why| Decision::Answer(Done::refused(By::refused(why)));
        let Some(lineage) = lineage else {
            return refuse(EndRefusal::NoSuchAgent);
        };
        let hire = lineage.agent.agent;
        if let Err(not_a_lead) = by.leads(&lineage, hire) {
            return Decision::Answer(Done::refused(not_a_lead));
        }
        if lineage.agent.kind == AgentKind::Hand {
            return refuse(EndRefusal::TheHand);
        }
        if lineage.is_mayor() {
            return refuse(EndRefusal::TheMayor);
        }
        if let Status::Ended(left) = &lineage.agent.status {
            return refuse(EndRefusal::Left {
                hire,
                left: left.clone(),
            });
        }
        Decision::Change(
            Decided::new(())
                .change(Change::End {
                    agent: hire,
                    by: by.by(),
                    why: self.why.clone(),
                })
                .change(waits::Change::Lapse { agent: hire })
                // No later drive makes it live when it is still
                // being hired.
                .change(steps::Change::Close {
                    row: hire::row(hire),
                })
                .record(record::Entry::Ended {
                    hire,
                    by: by.by(),
                    why: self.why.clone(),
                }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

/// The lineage of the agent `named` names, reading `Mayor` as
/// `hamlet`'s.
async fn named_lineage(
    rows: &Rows<'_>,
    hamlet: Option<&HamletId>,
    named: &Named,
) -> Result<Option<Lineage>> {
    match super::resolve(rows, hamlet, named).await? {
        Some(agent) => super::lineage(rows, agent).await,
        None => Ok(None),
    }
}

impl<By: RealmWide> Act<By> for Seat {
    type Output = Done<SeatRefusal>;
    type Loaded = Option<Lineage>;
    type Done = ();

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Option<Lineage>> {
        todo!()
    }

    fn decide(
        &self,
        by: &By,
        lineage: Option<Lineage>,
    ) -> Decision<(), Self::Output> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) -> Self::Output {
        todo!()
    }
}

/// Reports its reach as moved (to `Ended`), and the places of its
/// lead, of the moved hires and of the agents it had a road to.
pub(in crate::realm) async fn end(
    writer: &Writer<'_>,
    agent: AgentNumber,
    by: record::By,
    why: Option<String>,
) -> Result<Affected> {
    let rows = writer.rows();
    let lineage = super::lineage_of(&rows, agent).await?;
    let lead = lineage.agent.lead;
    let moved = living_agents(&lineage.hires);
    let places = roads::of(&rows, agent)
        .await?
        .into_iter()
        .map(|(other, _)| other)
        .chain(lead)
        .chain(moved.iter().copied())
        .collect::<BTreeSet<_>>();
    let how = match by {
        record::By::Chief => HowEnded::ByTheChief,
        record::By::Agent(Hand::NUMBER) => HowEnded::ByTheHand,
        record::By::Agent(lead) => HowEnded::ByALead { lead },
    };
    writer
        .execute(
            "UPDATE agent SET status = 'ended', ended_how = ?2, \
             ended_why = ?3 WHERE id = ?1",
            vec![
                number(agent),
                BindParam::Text(serde_json::to_string(&how)?),
                why.map_or(BindParam::Null, BindParam::Text),
            ],
        )
        .await?;
    for hire in moved {
        set_lead(writer, hire, lead).await?;
    }
    Ok(Affected {
        places,
        reaches: BTreeSet::from([agent]),
        ..Affected::seen([Part::Agents, Part::Reaches])
    })
}

/// The agents of `members` that have not ended.
fn living_agents(members: &[Member]) -> Vec<AgentNumber> {
    members
        .iter()
        .filter(|member| !matches!(member.status, Status::Ended(_)))
        .map(|member| member.agent)
        .collect()
}

async fn set_lead(
    writer: &Writer<'_>,
    hire: AgentNumber,
    lead: Option<AgentNumber>,
) -> Result<()> {
    writer
        .execute(
            "UPDATE hire SET lead = ?2 WHERE agent = ?1",
            vec![number(hire), lead.map_or(BindParam::Null, number)],
        )
        .await?;
    Ok(())
}

/// Reports the places that moved.
pub(in crate::realm) async fn seat(
    writer: &Writer<'_>,
    mayor: AgentNumber,
) -> Result<Affected> {
    todo!()
}

#[cfg(test)]
mod tests {
    use hamlet_messages::address::Agent;
    use hamlet_messages::realm::{LeadRefusal, Left, Rank, Tier};
    use hamlet_messages::sender::Client;
    use hamlet_messages::workspace::reach::Mode;

    use super::*;
    use crate::realm::chiefs::limits;

    /// The lineage of hire 5 in one hamlet: the Hand 1, the mayor 2,
    /// hire 3 under it, and 5 under 3, still hiring.
    fn lineage_of_5() -> Lineage {
        let member = |agent, lead: Option<u32>, status| Member {
            agent: AgentNumber(agent),
            kind: AgentKind::Hire,
            status,
            lead: lead.map(AgentNumber),
            has_purpose: false,
        };
        Lineage {
            agent: member(5, Some(3), Status::Hiring),
            leads: vec![
                member(3, Some(2), Status::Live),
                member(2, None, Status::Live),
            ],
            hires: Vec::new(),
            hamlet: Some(chiefs::Hamlet {
                id: HamletId("h".to_owned()),
                mayor: AgentNumber(2),
                tier: Tier::Fifty,
                limits: limits::Limits::default(),
                folder: None,
                mode: Mode::Manual,
            }),
        }
    }

    fn hand() -> Lineage {
        Lineage {
            agent: Member {
                agent: Hand::NUMBER,
                kind: AgentKind::Hand,
                status: Status::Live,
                lead: None,
                has_purpose: false,
            },
            leads: Vec::new(),
            hires: Vec::new(),
            hamlet: None,
        }
    }

    fn left_5() -> (Lineage, Left) {
        let left = Left {
            how: HowEnded::ByTheChief,
            why: None,
        };
        let mut lineage = lineage_of_5();
        lineage.agent.status = Status::Ended(left.clone());
        (lineage, left)
    }

    fn end_5() -> End {
        End {
            hire: Named::Agent {
                agent: AgentNumber(5),
            },
            why: None,
        }
    }

    fn by(agent: u32) -> Agent {
        Agent(AgentNumber(agent))
    }

    #[test]
    fn a_lead_ends_only_what_is_under_it() {
        let decide =
            |lead| end_5().decide(&by(lead), Some(lineage_of_5()));
        assert!(matches!(decide(3), Decision::Change(_)));
        assert_eq!(
            decide(4),
            Decision::Answer(Done::Refused {
                why: LeadRefusal::Rank(Rank::NotALead {
                    hire: AgentNumber(5)
                })
            })
        );
    }

    #[test]
    fn an_end_lapses_the_waits_and_closes_the_steps() {
        let Decision::Change(decided) =
            end_5().decide(&by(3), Some(lineage_of_5()))
        else {
            panic!("the lead of 5 ends it");
        };
        assert!(
            decided.changes.contains(
                &waits::Change::Lapse {
                    agent: AgentNumber(5)
                }
                .into()
            )
        );
        assert!(
            decided.changes.contains(
                &steps::Change::Close {
                    row: hire::row(AgentNumber(5))
                }
                .into()
            )
        );
    }

    #[test]
    fn an_end_refuses_the_hand_the_mayor_and_a_hire_that_left() {
        let refused = |lineage, why| {
            assert_eq!(
                end_5().decide(&Client, Some(lineage)),
                Decision::Answer(Done::Refused { why })
            );
        };
        let five = lineage_of_5();
        let mayor = Lineage {
            agent: five.leads[1].clone(),
            leads: Vec::new(),
            ..five
        };
        let (left, how) = left_5();

        refused(hand(), EndRefusal::TheHand);
        refused(mayor, EndRefusal::TheMayor);
        refused(
            left,
            EndRefusal::Left {
                hire: AgentNumber(5),
                left: how,
            },
        );
    }
}
