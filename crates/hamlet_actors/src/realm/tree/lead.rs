//! A lead's acts on the tree: hire, end and seat (H40, H5, H18).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U4 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::{
    Done, End, EndRefusal, HamletId, Hire, HireAnswer, HireRefusal,
    Named, ProfileId, Refusable, Seat, SeatRefusal,
};
use hamlet_messages::workspace::AgentNumber;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::{Decider, Lead, RealmWide};
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer, number};
use super::super::{chiefs, hire, record, steps, tasks};
use super::{Change, Lineage, Of, Status};

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

/// Reports its reach as moved (to `Ended`), the places of its lead,
/// of the moved hires and of the agents it had a road to, and
/// `Part::Agents`.
pub(in crate::realm) async fn end(
    writer: &Writer<'_>,
    agent: AgentNumber,
    by: record::By,
    why: Option<String>,
) -> Result<Affected> {
    todo!()
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
    use hamlet_messages::agent::AgentKind;
    use hamlet_messages::realm::{
        HamletId, LeadRefusal, Named, Rank, Tier,
    };
    use hamlet_messages::workspace::reach::Mode;

    use super::super::{Member, Status};
    use super::*;
    use crate::realm::chiefs::{self, limits};
    use crate::realm::{hire, steps, waits};

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
    #[ignore = "the scaffold: U4 fills it"]
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
    #[ignore = "the scaffold: U4 fills it"]
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
}
