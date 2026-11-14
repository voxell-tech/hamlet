//! A lead's acts on the tree: hire, end and seat (H40, H5, H18), for
//! the chief and for an agent alike, and the writes of an end and a
//! seating.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U4 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::{
    Done, End, EndRefusal, Hire, HireAnswer, HireRefusal, ProfileId,
    Seat, SeatRefusal,
};
use hamlet_messages::workspace::AgentNumber;

use super::super::act::{Act, Decision};
use super::super::callers::{Lead, RealmWide};
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer};
use super::super::record;
use super::Lineage;

/// What a hire decides over.
pub(in crate::realm) struct Hiring {
    /// The lead named, or the caller, or the mayor, with its
    /// lineage; `None` when none is found.
    under: Option<Lineage>,
    /// The hamlet's living hires, for its tier.
    living: u32,
    /// The profile named, when one was: `None` inside when missing.
    profile: Option<Option<super::Profile>>,
    /// The hire's number, and a blank profile's.
    agent: AgentNumber,
    blank: ProfileId,
}

impl<By: Lead> Act<By> for Hire {
    type Output = HireAnswer<By::Why<HireRefusal>>;
    type Loaded = Hiring;
    type Done = AgentNumber;

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Hiring> {
        todo!()
    }

    fn decide(
        &self,
        by: &By,
        hiring: Hiring,
    ) -> Decision<AgentNumber, Self::Output> {
        todo!()
    }

    fn answer(
        &self,
        agent: AgentNumber,
        made: &Made,
    ) -> Self::Output {
        todo!()
    }
}

impl<By: Lead> Act<By> for End {
    type Output = Done<By::Why<EndRefusal>>;
    /// The hire named and its lineage.
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

/// Seats a hire as its hamlet's mayor, for the chief or the Hand:
/// the Actions tuple admits no other caller.
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

/// `tree::apply`'s arm for `Change::End`: the agent ends with how, by
/// whom and why; its hires go to its lead. Reports its reach as moved
/// (to `Ended`), the places of its lead, of the moved hires and of
/// the agents it had a road to, and `Part::Agents`.
pub(in crate::realm) async fn end(
    writer: &Writer<'_>,
    agent: AgentNumber,
    by: record::By,
    why: Option<String>,
) -> Result<Affected> {
    todo!()
}

/// `tree::apply`'s arm for `Change::Seat`: `mayor` becomes the root
/// of its hamlet's tree, the old mayor under it. Reports the places
/// that moved.
pub(in crate::realm) async fn seat(
    writer: &Writer<'_>,
    mayor: AgentNumber,
) -> Result<Affected> {
    todo!()
}

#[cfg(test)]
mod tests {
    use hamlet_messages::address::Agent;
    use hamlet_messages::realm::{LeadRefusal, Named, Rank};

    use super::*;
    use crate::realm::{hire, steps, waits};

    /// The lineage of hire 5 in one hamlet: the Hand 1, the mayor 2,
    /// hire 3 under it, and 5 under 3. Built by hand, with no file.
    fn lineage_of_5() -> Lineage {
        todo!()
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
