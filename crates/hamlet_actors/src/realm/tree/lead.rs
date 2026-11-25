//! A lead's acts on the tree: hire, end and seat (H40, H5, H18).

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use hamlet_actor::{Action, BindParam, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::Profile;
use hamlet_messages::agent::{AgentKind, Input, InputId, Submit};
use hamlet_messages::profile::{
    Read, ReadAnswer, Revision, VersionName,
};
use hamlet_messages::realm::oversight::Permission;
use hamlet_messages::realm::profiles::ProfileRefusal;
use hamlet_messages::realm::{
    AgentName, Asked, End, EndRefusal, HamletId, Hire, HireAnswer,
    HireRefusal, HowEnded, LeadRefusal, Left, Named, OnlyAHire,
    Refusable, Seat, Tool, ToolCall,
};
use hamlet_messages::sender::{Hand, sender};
use hamlet_messages::workspace::AgentNumber;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use super::super::act::{self, Act, Decided, Decision};
use super::super::callers::{self, Lead, RealmWide};
use super::super::calls::{Calls, Delivery, Made};
use super::super::change::{self, Affected, Rows, Writer, number};
use super::super::peer::{self, When};
use super::super::world::Part;
use super::super::{
    Realm, chiefs, hire, laid, marker, oversight, parcels, profiles,
    record, retire, roads, steps, tasks, waits, workers,
};
use super::{Change, Lineage, Node, Of, Status};

enum Joining {
    Hamlet(HamletId),
    Missing(HamletId),
    Unnamed,
}

impl Joining {
    fn id(&self) -> Option<&HamletId> {
        match self {
            Self::Hamlet(id) => Some(id),
            Self::Missing(_) | Self::Unnamed => None,
        }
    }
}

pub(in crate::realm) struct Hiring {
    hamlet: Joining,
    under: Option<Lineage>,
    living: u32,
    /// The parts of the hire, and the hire as it is shown.
    of: Result<(Of, AgentName), HireRefusal>,
    agent: AgentNumber,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::realm) struct Pinned {
    pub hire: Hire,
    pub lacked: Option<VersionName>,
}

impl Pinned {
    pub(in crate::realm) async fn asked(
        realm: &Realm,
        ctx: &Ctx<Realm>,
        key: &marker::Key,
        hire: Hire,
    ) -> Result<Result<Self, HireRefusal>> {
        let rows = Rows::outside(ctx)?;
        let (Some(version), Some(named)) =
            (&hire.version, hire.profile)
        else {
            return Ok(Ok(Self { hire, lacked: None }));
        };
        if marker::find::<IgnoredAny>(&rows, key).await?.is_some() {
            return Ok(Ok(Self { hire, lacked: None }));
        }
        let Ok(profile) = profiles::shared(&rows, named).await?
        else {
            return Ok(Ok(Self { hire, lacked: None }));
        };
        let at = Read {
            at: Some(Revision::Version {
                version: version.clone(),
            }),
            path: None,
        };
        let read = match realm
            .peers
            .call(&Profile(profile), &at, When::BeforeAnswer)
            .await
        {
            Ok(read) => read,
            Err(error) if !peer::refused(&error) => {
                return Ok(Err(HireRefusal::Unanswered {
                    label: profiles::label_of(&rows, profile).await?,
                    version: version.clone(),
                }));
            }
            Err(error) => return Err(error),
        };
        let lacked = (version.0 == hamlet_git::MAIN
            || matches!(read, ReadAnswer::Refused { .. }))
        .then(|| version.clone());
        Ok(Ok(Self { hire, lacked }))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::realm) struct Pinning<M>(pub M);

impl<M: Message> Action for Pinning<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Pinning<M> {
    type To = M::To;
    type From = M::From;
}

impl Handler<Pinning<Asked<Hire>>> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Pinning(asked): Pinning<Asked<Hire>>,
    ) -> Result<HireAnswer<HireRefusal>> {
        let name = <Asked<Hire> as Action>::NAME;
        let by = sender(&ctx, &asked)?;
        let id = marker::Id::Request(asked.id);
        let key = marker::Key::of(&by, id, name, &asked.act)?;
        let pinned = match Pinned::asked(&self, &ctx, &key, asked.act)
            .await?
        {
            Ok(pinned) => pinned,
            Err(why) => return Ok(HireAnswer::refused(why)),
        };
        act::run_keyed(&self, &ctx, by, Some(key), name, pinned).await
    }
}

impl Handler<Pinning<Tool<Hire>>> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Pinning(tool): Pinning<Tool<Hire>>,
    ) -> Result<HireAnswer<LeadRefusal<HireRefusal>>> {
        let name = <Tool<Hire> as Action>::NAME;
        let by = sender(&ctx, &tool)?;
        let (key, called, hire) = act::called(&by, name, tool)?;
        let pinned =
            match Pinned::asked(&self, &ctx, &key, hire).await? {
                Ok(pinned) => pinned,
                Err(why) => {
                    return Ok(HireAnswer::refused(
                        LeadRefusal::Act(why),
                    ));
                }
            };
        let ran = act::run_called(
            &self,
            &ctx,
            by,
            (key, called),
            name,
            Hire::TOOL,
            pinned,
        )
        .await?;
        Ok(callers::to_tool(ran))
    }
}

impl<By: Lead> Act<By> for Pinned {
    type Output = HireAnswer<By::Why<HireRefusal>>;
    type Loaded = Hiring;
    type Done = AgentName;

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Hiring> {
        let hire = &self.hire;
        let caller = super::caller(by);
        let own = super::own_hamlet(rows, caller).await?;
        let hamlet = match (&own, &hire.hamlet) {
            (Some(own), _) => Joining::Hamlet(own.clone()),
            (None, Some(named)) => {
                chiefs::hamlet(rows, named).await?.map_or_else(
                    || Joining::Missing(named.clone()),
                    |hamlet| Joining::Hamlet(hamlet.id),
                )
            }
            (None, None) => Joining::Unnamed,
        };
        let under = match (&hire.under, caller, own) {
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
        let joins = under
            .as_ref()
            .and_then(|under| under.hamlet.as_ref())
            .map(|hamlet| &hamlet.id);
        let made = super::made_of(
            rows,
            hire.profile,
            hire.name.clone(),
            joins,
            &by.by(),
        )
        .await?;
        let agent = super::fresh(rows).await?;
        let of = match (made, &self.lacked) {
            (Ok(Of::Profile(profile)), Some(version)) => {
                Err(HireRefusal::NoVersion {
                    label: profiles::label_of(rows, profile).await?,
                    version: version.clone(),
                })
            }
            (Ok(of), _) => {
                let shown =
                    record::new_hire(agent, &of.name(rows).await?);
                Ok((of, shown))
            }
            (Err(ProfileRefusal::NoSuchProfile { profile }), _) => {
                Err(HireRefusal::NoSuchProfile { profile })
            }
            (Err(ProfileRefusal::Apart { label }), _) => {
                Err(HireRefusal::Apart { label })
            }
        };
        Ok(Hiring {
            hamlet,
            under,
            living,
            of,
            agent,
        })
    }

    fn decide(
        &self,
        by: &By,
        hiring: Hiring,
    ) -> Decision<AgentName, Self::Output> {
        let refuse = |why| {
            Decision::Answer(HireAnswer::refused(By::refused(why)))
        };
        let named = match hiring.hamlet {
            Joining::Missing(id) => {
                return refuse(HireRefusal::NoSuchHamlet { id });
            }
            Joining::Hamlet(hamlet) => Some(hamlet),
            Joining::Unnamed => None,
        };
        let hire = &self.hire;
        let lead_named = hire.under.as_ref().unwrap_or(&Named::Mayor);
        let Some(under) = hiring.under else {
            return refuse(
                if named.is_none() && *lead_named == Named::Mayor {
                    HireRefusal::NoHamlet
                } else {
                    HireRefusal::NoSuchAgent {
                        address: lead_named.to_string(),
                    }
                },
            );
        };
        let lead = under.agent.agent;
        if by.by() != record::By::Agent(lead)
            && let Err(not_a_lead) = by.leads(&under)
        {
            return Decision::Answer(HireAnswer::refused(not_a_lead));
        }
        let (AgentKind::Hire, Some(hamlet)) =
            (under.agent.kind, under.hamlet.as_ref())
        else {
            return refuse(HireRefusal::NotAHire {
                name: under.agent.name.name.clone(),
            });
        };
        let purpose = super::given_purpose(hire.purpose.as_deref());
        if purpose.is_none() && super::caller(by).is_some() {
            return refuse(HireRefusal::NoPurpose);
        }
        if named.as_ref().is_some_and(|named| *named != hamlet.id) {
            return refuse(HireRefusal::OtherHamlet {
                lead: under.agent.name.clone(),
            });
        }
        if let Status::Ended(left) = &under.agent.status {
            return refuse(HireRefusal::LeadLeft {
                lead: under.agent.name.clone(),
                left: left.clone(),
            });
        }
        if hiring.living >= hamlet.most() {
            return refuse(HireRefusal::Full {
                hamlet: hamlet.id.clone(),
                most: hamlet.most(),
            });
        }
        let (of, shown) = match hiring.of {
            Ok(made) => made,
            Err(why) => return refuse(why),
        };
        let agent = hiring.agent;
        Decision::Change(
            Decided::new(shown)
                .change(Change::Hire {
                    agent,
                    hamlet: hamlet.id.clone(),
                    lead: Some(lead),
                    of,
                    version: hire.version.clone(),
                    purpose,
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
                }),
        )
    }

    fn answer(&self, agent: AgentName, _made: &Made) -> Self::Output {
        HireAnswer::Hired { agent }
    }
}

pub(in crate::realm) struct Ending {
    lineage: Option<Lineage>,
    /// The changes that stop the hire's living workers.
    workers: Vec<change::Change>,
    how: HowEnded,
}

/// What an end ended.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(in crate::realm) enum Ended {
    Hire(AgentName),
    /// A worker, stopped: its starter is told it gave no answer.
    Worker(AgentName),
}

impl<By: Lead> Act<By> for End {
    type Output = By::OnAgent<By::Why<EndRefusal>>;
    type Loaded = Ending;
    type Done = Ended;

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Ending> {
        let lineage = super::named(rows, by, &self.hire).await?;
        let workers = match &lineage {
            Some(lineage) => {
                workers::end_with(rows, lineage.agent.agent).await?
            }
            None => Vec::new(),
        };
        Ok(Ending {
            lineage,
            workers,
            how: how_of(rows, by.by()).await?,
        })
    }

    fn decide(
        &self,
        by: &By,
        ending: Ending,
    ) -> Decision<Ended, Self::Output> {
        let refuse = |why| {
            Decision::Answer(Self::Output::refused(By::refused(why)))
        };
        let Some(lineage) = ending.lineage else {
            return refuse(EndRefusal::NoSuchAgent {
                address: self.hire.to_string(),
            });
        };
        let hire = lineage.agent.agent;
        let name = lineage.agent.name.clone();
        if let Err(not_a_lead) = by.leads(&lineage) {
            return Decision::Answer(Self::Output::refused(
                not_a_lead,
            ));
        }
        if lineage.agent.kind == AgentKind::Hand {
            return refuse(EndRefusal::TheHand);
        }
        if lineage.is_mayor() {
            return refuse(EndRefusal::TheMayor);
        }
        if lineage.agent.kind == AgentKind::Worker {
            let mut decided = Decided::new(Ended::Worker(name));
            decided.changes =
                workers::ending(hire, HowEnded::Stopped, None);
            return Decision::Change(decided);
        }
        let mut decided = Decided::new(Ended::Hire(name)).record(
            record::Entry::Ended {
                hire,
                by: by.by(),
                why: self.why.clone(),
            },
        );
        decided.changes = hire_ending(
            hire,
            ending.how,
            self.why.clone(),
            ending.workers,
        );
        Decision::Change(decided)
    }

    fn deliveries(&self, _by: &By, ended: &Ended) -> Calls {
        match ended {
            Ended::Hire(_) => Calls::none(),
            Ended::Worker(worker) => Calls::after_answer(
                Delivery::WorkerAnswer(worker.agent),
            ),
        }
    }

    fn answer(&self, ended: Ended, _made: &Made) -> Self::Output {
        let (Ended::Hire(agent) | Ended::Worker(agent)) = ended;
        By::on_agent(agent)
    }
}

impl<By: RealmWide> Act<By> for Seat {
    type Output = By::Seated;
    type Loaded = Option<Lineage>;
    type Done = (AgentName, HamletId);

    async fn load(
        &self,
        _by: &By,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        super::named_in(rows, None, &self.hire).await
    }

    fn decide(
        &self,
        by: &By,
        named: Self::Loaded,
    ) -> Decision<(AgentName, HamletId), Self::Output> {
        let refuse =
            |why| Decision::Answer(Self::Output::refused(why));
        let Some(lineage) = named else {
            return refuse(OnlyAHire::NoSuchAgent {
                address: self.hire.to_string(),
            });
        };
        let hire = lineage.agent.agent;
        let (AgentKind::Hire, Some(hamlet)) =
            (lineage.agent.kind, &lineage.hamlet)
        else {
            return refuse(OnlyAHire::NotAHire {
                name: lineage.agent.name.name.clone(),
            });
        };
        let seated = (lineage.agent.name.clone(), hamlet.id.clone());
        if lineage.is_mayor() {
            return Decision::Again(seated);
        }
        Decision::Change(
            Decided::new(seated)
                // First: `seat` reads the old mayor from the hamlet.
                .change(Change::Seat { mayor: hire })
                .change(chiefs::Change::Mayor {
                    hamlet: hamlet.id.clone(),
                    mayor: hire,
                })
                .record(record::Entry::Seated {
                    hire,
                    by: by.by(),
                    why: self.why.clone(),
                }),
        )
    }

    fn answer(
        &self,
        (hire, hamlet): (AgentName, HamletId),
        _made: &Made,
    ) -> Self::Output {
        By::seated(hire, hamlet)
    }
}

/// The changes that end `hire`, close its steps and stop its living
/// `workers`.
pub(in crate::realm) fn hire_ending(
    hire: AgentNumber,
    how: HowEnded,
    why: Option<String>,
    workers: Vec<change::Change>,
) -> Vec<change::Change> {
    [
        Change::End {
            agent: hire,
            how,
            why,
        }
        .into(),
        waits::Change::Lapse { agent: hire }.into(),
        parcels::Change::ReceiverLeft { receiver: hire }.into(),
        steps::Change::Close {
            row: hire::row(hire),
        }
        .into(),
    ]
    .into_iter()
    .chain(workers)
    .collect()
}

/// The changes that end `hire`, whose step failed for good, and its
/// lead's notice of it. None for the Hand, a mayor or a hire that
/// ended: an end refuses them.
pub(in crate::realm) async fn failed(
    rows: &Rows<'_>,
    hire: AgentNumber,
    why: String,
) -> Result<(Vec<change::Change>, Calls)> {
    let lineage = super::lineage_of(rows, hire).await?;
    if lineage.agent.kind == AgentKind::Hand
        || lineage.is_mayor()
        || matches!(lineage.agent.status, Status::Ended(_))
    {
        return Ok((Vec::new(), Calls::none()));
    }
    let left = Left {
        how: HowEnded::Failed,
        why: Some(why.clone()),
    };
    let calls = match lineage.agent.lead {
        Some(lead) if super::left(rows, lead).await?.is_none() => {
            let ended = EndRefusal::Left {
                hire: lineage.agent.name.clone(),
                left: left.clone(),
            };
            let text = format!("{ended}: {why}");
            Calls::after_answer(Delivery::Line {
                to: lead,
                submit: Submit {
                    id: InputId(format!("hire/{}", hire.0)),
                    task: None,
                    input: Input::Notice {
                        text,
                        conversation: None,
                        held: None,
                    },
                },
            })
        }
        _ => Calls::none(),
    };
    let workers = workers::end_with(rows, hire).await?;
    let changes = hire_ending(hire, left.how, left.why, workers);
    Ok((changes, calls))
}

async fn how_of(rows: &Rows<'_>, by: record::By) -> Result<HowEnded> {
    Ok(match by {
        record::By::Chief => HowEnded::ByTheChief,
        record::By::Agent(Hand::NUMBER) => HowEnded::ByTheHand,
        record::By::Agent(lead) => HowEnded::ByALead {
            lead: record::agent_name(rows, lead).await?,
        },
    })
}

pub(in crate::realm) async fn end(
    writer: &Writer<'_>,
    agent: AgentNumber,
    how: HowEnded,
    why: Option<String>,
) -> Result<Affected> {
    let rows = writer.rows();
    let lineage = super::lineage_of(&rows, agent).await?;
    let lead = lineage.agent.lead;
    let moved = if matches!(how, HowEnded::HamletRemoved { .. }) {
        Vec::new()
    } else {
        living_agents(&lineage.hires)
    };
    let places = roads::of(&rows, agent)
        .await?
        .into_iter()
        .map(|(other, _)| other)
        .chain(lead)
        .chain(moved.iter().copied())
        .collect::<BTreeSet<_>>();
    let roads_seen = (!laid::of(&rows, agent).await?.is_empty())
        .then_some(Part::Roads);
    let retires = !matches!(how, HowEnded::Merged { .. })
        && lineage.agent.kind == AgentKind::Hire;
    let ended = super::set_ended(writer, agent, &how, why).await?;
    for hire in moved {
        set_lead(writer, hire, lead).await?;
    }
    // A merge's own row retires the hire it ends.
    let left = if ended && retires {
        steps::apply(
            writer,
            steps::Change::Begin {
                row: retire::row(agent),
            },
        )
        .await?
    } else {
        Affected::default()
    };
    let revoked = oversight::apply(
        writer,
        oversight::Change::Revoke {
            agent,
            permission: Permission::Network,
        },
    )
    .await?;
    Ok(Affected {
        places,
        reaches: revoked.reaches.into_iter().chain([agent]).collect(),
        begun: left.begun,
        ..Affected::seen(
            [Part::Agents, Part::Reaches]
                .into_iter()
                .chain(roads_seen),
        )
    })
}

fn living_agents(nodes: &[Node]) -> Vec<AgentNumber> {
    nodes
        .iter()
        .filter(|node| !matches!(node.status, Status::Ended(_)))
        .map(|node| node.agent)
        .collect()
}

pub(super) async fn set_lead(
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
    use hamlet_messages::realm::{
        Done, DoneTo, LeadRefusal, ProfileId, Rank, Tier,
    };
    use hamlet_messages::sender::Client;
    use hamlet_messages::workspace::reach::Mode;

    use super::*;
    use crate::realm::chiefs::limits;
    use crate::realm::testing::Started;
    use crate::realm::{change, versions};

    /// The lineage of hire 5 in one hamlet: the mayor 2, hire 3 under
    /// it, and 5 under 3.
    fn lineage_of_5() -> Lineage {
        let node = |agent, lead: Option<u32>, status| Node {
            agent: AgentNumber(agent),
            name: record::bare(AgentNumber(agent)),
            kind: AgentKind::Hire,
            status,
            lead: lead.map(AgentNumber),
            has_purpose: false,
        };
        Lineage {
            agent: node(5, Some(3), Status::Hiring),
            leads: vec![
                node(3, Some(2), Status::Live),
                node(2, None, Status::Live),
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
            agent: Node {
                agent: Hand::NUMBER,
                name: record::bare(Hand::NUMBER),
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

    fn end_5() -> End {
        End {
            hire: Named::Agent {
                agent: AgentNumber(5),
            },
            why: None,
        }
    }

    fn ending(lineage: Lineage) -> Ending {
        Ending {
            lineage: Some(lineage),
            workers: Vec::new(),
            how: HowEnded::ByTheChief,
        }
    }

    fn seat_5() -> Seat {
        Seat {
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
            of: Err(HireRefusal::NoSuchProfile {
                profile: ProfileId(9),
            }),
            ..under_5("h")
        };

        assert!(matches!(decide(under_5("h")), Decision::Change(_)));
        assert_eq!(
            decide(under_5("k")),
            refused(HireRefusal::OtherHamlet {
                lead: record::bare(AgentNumber(5))
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
            of: Ok((
                Of::Blank {
                    profile: ProfileId(6),
                    name: "Hire 4".to_owned(),
                    kind: profiles::Kind::Blank,
                },
                record::new_hire(AgentNumber(6), "Hire 4"),
            )),
            agent: AgentNumber(6),
        }
    }

    fn hire_for(purpose: Option<&str>) -> Pinned {
        Pinned {
            hire: Hire {
                hamlet: None,
                under: None,
                profile: None,
                version: None,
                purpose: purpose.map(str::to_owned),
                name: None,
            },
            lacked: None,
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
    fn an_agent_not_leading_a_lead_elsewhere_hears_its_rank_first() {
        let hire = hire_for(Some("dig"));
        let decide = |agent| hire.decide(&by(agent), under_5("k"));
        let other = HireRefusal::OtherHamlet {
            lead: record::bare(AgentNumber(5)),
        };

        assert_eq!(
            decide(4),
            Decision::Answer(HireAnswer::Refused {
                why: LeadRefusal::Rank(Rank::NotALead {
                    hire: record::bare(AgentNumber(5))
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
            |lead| end_5().decide(&by(lead), ending(lineage_of_5()));
        assert!(matches!(decide(3), Decision::Change(_)));
        assert_eq!(
            decide(4),
            Decision::Answer(DoneTo::Refused {
                why: LeadRefusal::Rank(Rank::NotALead {
                    hire: record::bare(AgentNumber(5))
                })
            })
        );
    }

    #[test]
    fn an_end_lapses_the_waits_and_closes_the_steps() {
        let Decision::Change(decided) =
            end_5().decide(&by(3), ending(lineage_of_5()))
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
    fn an_end_refuses_the_hand_and_the_mayor() {
        let refused = |lineage, why| {
            assert_eq!(
                end_5().decide(&Client, ending(lineage)),
                Decision::Answer(Done::Refused { why })
            );
        };

        refused(hand(), EndRefusal::TheHand);
        refused(mayor_2(), EndRefusal::TheMayor);
    }

    #[test]
    fn a_seat_refuses_the_hand() {
        let decide =
            |lineage| seat_5().decide(&Client, Some(lineage));
        let refused = |why| Decision::Answer(Done::Refused { why });

        assert_eq!(
            decide(hand()),
            refused(OnlyAHire::NotAHire {
                name: "hand".to_owned()
            })
        );
        assert_eq!(
            decide(mayor_2()),
            Decision::Again((
                record::bare(AgentNumber(2)),
                HamletId("h".to_owned())
            ))
        );
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
