//! A lead's acts on the tree: hire, end and seat (H40, H5, H18).

use std::collections::BTreeSet;

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::{
    Done, End, EndRefusal, HamletId, Hire, HireAnswer, HireRefusal,
    HowEnded, Named, ProfileId, Refusable, Seat, SeatRefusal,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::{Lead, RealmWide};
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer, number};
use super::super::world::Part;
use super::super::{
    chiefs, hire, record, roads, steps, tasks, waits,
};
use super::{Change, Lineage, Member, Of, Status};

enum Joining {
    Hamlet(HamletId),
    Missing,
    Unnamed,
}

impl Joining {
    fn id(&self) -> Option<&HamletId> {
        match self {
            Self::Hamlet(id) => Some(id),
            Self::Missing | Self::Unnamed => None,
        }
    }
}

pub(in crate::realm) struct Hiring {
    hamlet: Joining,
    under: Option<Lineage>,
    living: u32,
    of: Result<Of, ProfileId>,
    agent: AgentNumber,
}

impl<By: Lead> Act<By> for Hire {
    type Output = HireAnswer<By::Why<HireRefusal>>;
    type Loaded = Hiring;
    type Done = AgentNumber;

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Hiring> {
        let caller = super::caller(by);
        let own = super::own_hamlet(rows, caller).await?;
        let hamlet = match (&own, &self.hamlet) {
            (Some(own), _) => Joining::Hamlet(own.clone()),
            (None, Some(named)) => chiefs::hamlet(rows, named)
                .await?
                .map_or(Joining::Missing, |hamlet| {
                    Joining::Hamlet(hamlet.id)
                }),
            (None, None) => Joining::Unnamed,
        };
        let under = match (&self.under, caller, own) {
            (None, Some(caller), Some(_)) => {
                super::lineage(rows, caller).await?
            }
            (named, ..) => {
                let named = named.as_ref().unwrap_or(&Named::Mayor);
                super::named_in(rows, hamlet.id(), named).await?
            }
        };
        let living = match under
            .as_ref()
            .and_then(|under| under.hamlet.as_ref())
        {
            Some(hamlet) => super::living(rows, &hamlet.id).await?,
            None => 0,
        };
        Ok(Hiring {
            hamlet,
            under,
            living,
            of: super::made_of(rows, self.profile).await?,
            agent: super::fresh(rows).await?,
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
            Joining::Missing => {
                return refuse(HireRefusal::NoSuchHamlet);
            }
            Joining::Hamlet(hamlet) => Some(hamlet),
            Joining::Unnamed => None,
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
        let Some(hamlet) = under.hamlet.as_ref() else {
            return refuse(HireRefusal::NoSuchAgent);
        };
        if by.by() != record::By::Agent(lead)
            && let Err(not_a_lead) = by.leads(&under, lead)
        {
            return Decision::Answer(HireAnswer::refused(not_a_lead));
        }
        let purpose = super::given_purpose(self.purpose.as_deref());
        if purpose.is_none() && super::caller(by).is_some() {
            return refuse(HireRefusal::NoPurpose);
        }
        if named.as_ref().is_some_and(|named| *named != hamlet.id) {
            return refuse(HireRefusal::OtherHamlet { lead });
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
        let of = match hiring.of {
            Ok(of) => of,
            Err(profile) => {
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
                    purpose: purpose.clone(),
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
                    why: purpose,
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

impl<By: Lead> Act<By> for End {
    type Output = Done<By::Why<EndRefusal>>;
    type Loaded = Option<Lineage>;
    type Done = ();

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Option<Lineage>> {
        super::named(rows, by, &self.hire).await
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

impl<By: RealmWide> Act<By> for Seat {
    type Output = Done<SeatRefusal>;
    type Loaded = Option<Lineage>;
    type Done = ();

    async fn load(
        &self,
        _by: &By,
        rows: &Rows<'_>,
    ) -> Result<Option<Lineage>> {
        super::named_in(rows, None, &self.hire).await
    }

    fn decide(
        &self,
        by: &By,
        lineage: Option<Lineage>,
    ) -> Decision<(), Self::Output> {
        let refuse = |why| Decision::Answer(Done::refused(why));
        let Some(lineage) = lineage else {
            return refuse(SeatRefusal::NoSuchAgent);
        };
        let hire = lineage.agent.agent;
        let Some(hamlet) = &lineage.hamlet else {
            return refuse(SeatRefusal::NotAHire);
        };
        if let Status::Ended(left) = &lineage.agent.status {
            return refuse(SeatRefusal::Left {
                hire,
                left: left.clone(),
            });
        }
        if lineage.is_mayor() {
            return Decision::Again(());
        }
        Decision::Change(
            Decided::new(())
                // First: `seat` reads the old mayor from the hamlet.
                .change(Change::Seat { mayor: hire })
                .change(chiefs::Change::Mayor {
                    hamlet: hamlet.id.clone(),
                    mayor: hire,
                })
                .record(record::Entry::Seated { hire, by: by.by() }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

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

pub(in crate::realm) async fn seat(
    writer: &Writer<'_>,
    mayor: AgentNumber,
) -> Result<Affected> {
    let rows = writer.rows();
    let seated = super::lineage_of(&rows, mayor).await?;
    let hamlet = seated.hamlet.context("a seated hire's hamlet")?;
    let old = super::lineage_of(&rows, hamlet.mayor).await?;
    let mut moved = living_agents(&old.hires);
    moved.retain(|&hire| hire != mayor);
    let places = chiefs::mayors(&rows)
        .await?
        .into_iter()
        .chain([Hand::NUMBER, mayor])
        .chain(seated.agent.lead)
        .chain(moved.iter().copied())
        .collect::<BTreeSet<_>>();
    for hire in moved.into_iter().chain([hamlet.mayor]) {
        set_lead(writer, hire, Some(mayor)).await?;
    }
    set_lead(writer, mayor, None).await?;
    Ok(Affected {
        places,
        ..Affected::seen([Part::Agents])
    })
}

#[cfg(test)]
mod tests {
    use hamlet_messages::address::Agent;
    use hamlet_messages::realm::{LeadRefusal, Left, Rank, Tier};
    use hamlet_messages::sender::Client;
    use hamlet_messages::workspace::reach::Mode;

    use super::*;
    use crate::realm::chiefs::limits;
    use crate::realm::testing::Started;
    use crate::realm::{change, versions};

    /// The lineage of hire 5 in one hamlet: the mayor 2, hire 3 under
    /// it, and 5 under 3.
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

    fn mayor_2() -> Lineage {
        let five = lineage_of_5();
        Lineage {
            agent: five.leads[1].clone(),
            leads: Vec::new(),
            ..five
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

    fn seat_5() -> Seat {
        Seat {
            hire: Named::Agent {
                agent: AgentNumber(5),
            },
        }
    }

    fn by(agent: u32) -> Agent {
        Agent(AgentNumber(agent))
    }

    #[test]
    fn a_hire_refuses_a_lead_elsewhere_a_full_hamlet_and_a_missing_profile()
     {
        let hire = hire_for(None);
        let decide = |hiring| hire.decide(&Client, hiring);
        let refused =
            |why| Decision::Answer(HireAnswer::refused(why));
        let full = Hiring {
            living: 50,
            ..under_5("h")
        };
        let missing = Hiring {
            of: Err(ProfileId(9)),
            ..under_5("h")
        };

        assert!(matches!(decide(under_5("h")), Decision::Change(_)));
        assert_eq!(
            decide(under_5("k")),
            refused(HireRefusal::OtherHamlet {
                lead: AgentNumber(5)
            })
        );
        assert_eq!(
            decide(full),
            refused(HireRefusal::Full {
                hamlet: HamletId("h".to_owned()),
                most: 50,
            })
        );
        assert_eq!(
            decide(missing),
            refused(HireRefusal::NoSuchProfile {
                profile: ProfileId(9)
            })
        );
    }

    fn under_5(hamlet: &str) -> Hiring {
        Hiring {
            hamlet: Joining::Hamlet(HamletId(hamlet.to_owned())),
            under: Some(lineage_of_5()),
            living: 3,
            of: Ok(Of::Blank(ProfileId(6))),
            agent: AgentNumber(6),
        }
    }

    fn hire_for(purpose: Option<&str>) -> Hire {
        Hire {
            hamlet: None,
            under: None,
            profile: None,
            purpose: purpose.map(str::to_owned),
        }
    }

    #[test]
    fn an_agents_hire_says_what_it_is_for() {
        let by_3 =
            |purpose| hire_for(purpose).decide(&by(3), under_5("h"));
        let no_purpose = Decision::Answer(HireAnswer::Refused {
            why: LeadRefusal::Act(HireRefusal::NoPurpose),
        });

        for purpose in [None, Some(""), Some(" \n ")] {
            assert_eq!(by_3(purpose), no_purpose, "{purpose:?}");
        }
        assert!(matches!(by_3(Some("dig")), Decision::Change(_)));
        let by_the_hand = hire_for(None).decide(&by(1), under_5("h"));
        assert_eq!(by_the_hand, no_purpose);
        let by_the_chief =
            hire_for(None).decide(&Client, under_5("h"));
        assert!(matches!(by_the_chief, Decision::Change(_)));
    }

    #[test]
    fn a_blank_purpose_is_kept_as_none_and_a_purpose_trimmed() {
        let kept = |purpose| {
            let Decision::Change(decided) =
                hire_for(purpose).decide(&Client, under_5("h"))
            else {
                panic!("the chief hires under 5");
            };
            decided.changes.into_iter().find_map(
                |change| match change {
                    change::Change::Tree(Change::Hire {
                        purpose,
                        ..
                    }) => Some(purpose),
                    _ => None,
                },
            )
        };

        assert_eq!(kept(Some("  ")), Some(None));
        assert_eq!(
            kept(Some(" dig \n")),
            Some(Some("dig".to_owned()))
        );
    }

    #[test]
    fn a_hire_under_the_hand_names_no_agent() {
        let under_the_hand = || Hiring {
            under: Some(hand()),
            ..under_5("h")
        };
        let hire = hire_for(Some("dig"));

        assert_eq!(
            hire.decide(&Client, under_the_hand()),
            Decision::Answer(HireAnswer::refused(
                HireRefusal::NoSuchAgent
            ))
        );
        for agent in [1, 3] {
            assert_eq!(
                hire.decide(&by(agent), under_the_hand()),
                Decision::Answer(HireAnswer::Refused {
                    why: LeadRefusal::Act(HireRefusal::NoSuchAgent)
                }),
                "by {agent}"
            );
        }
    }

    #[test]
    fn an_agent_not_leading_a_lead_elsewhere_hears_its_rank_first() {
        let hire = hire_for(Some("dig"));
        let decide = |agent| hire.decide(&by(agent), under_5("k"));
        let other = HireRefusal::OtherHamlet {
            lead: AgentNumber(5),
        };

        assert_eq!(
            decide(4),
            Decision::Answer(HireAnswer::Refused {
                why: LeadRefusal::Rank(Rank::NotALead {
                    hire: AgentNumber(5)
                })
            })
        );
        assert_eq!(
            decide(1),
            Decision::Answer(HireAnswer::Refused {
                why: LeadRefusal::Act(other)
            })
        );
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
        let (left, how) = left_5();

        refused(hand(), EndRefusal::TheHand);
        refused(mayor_2(), EndRefusal::TheMayor);
        refused(
            left,
            EndRefusal::Left {
                hire: AgentNumber(5),
                left: how,
            },
        );
    }

    #[test]
    fn a_seat_refuses_the_hand_and_a_hire_that_left() {
        let decide =
            |lineage| seat_5().decide(&Client, Some(lineage));
        let refused = |why| Decision::Answer(Done::Refused { why });
        let (left, how) = left_5();

        assert_eq!(decide(hand()), refused(SeatRefusal::NotAHire));
        assert_eq!(
            decide(left),
            refused(SeatRefusal::Left {
                hire: AgentNumber(5),
                left: how,
            })
        );
        assert_eq!(decide(mayor_2()), Decision::Again(()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_seat_puts_the_old_mayor_and_its_hires_under_the_new_one()
     {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        for (agent, lead) in [(3, 2), (4, 2), (5, 3)] {
            let lead = Some(AgentNumber(lead));
            started.hire(&hamlet, AgentNumber(agent), lead).await;
        }
        let places = async || {
            let mut places = Vec::new();
            for agent in 1..=5 {
                let rows = started.rows();
                places.push(
                    versions::place(&rows, AgentNumber(agent))
                        .await
                        .unwrap(),
                );
            }
            places
        };
        let before = places().await;
        let lineage =
            super::super::lineage(&started.rows(), AgentNumber(5))
                .await
                .unwrap();
        let Decision::Change(decided) =
            seat_5().decide(&Client, lineage)
        else {
            panic!("the chief seats 5");
        };
        change::commit(&started.realm, &started.ctx, decided.changes)
            .await
            .unwrap();

        let rows = started.rows();
        let mut leads = Vec::new();
        for agent in 2..=5 {
            let lineage =
                super::super::lineage_of(&rows, AgentNumber(agent));
            leads.push(
                lineage.await.unwrap().agent.lead.map(|lead| lead.0),
            );
        }
        assert_eq!(leads, [Some(5), Some(5), Some(5), None]);
        let read =
            chiefs::hamlet(&rows, &hamlet).await.unwrap().unwrap();
        assert_eq!(read.mayor, AgentNumber(5));
        let after = places().await;
        assert!(
            after
                .iter()
                .zip(&before)
                .all(|(after, before)| after > before),
            "every place moved: {before:?} to {after:?}"
        );
        started.stop().await;
    }
}
