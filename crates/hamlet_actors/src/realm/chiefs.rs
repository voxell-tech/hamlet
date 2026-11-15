//! The realm's row, the chief and the hamlets: the `realm`, `chief`,
//! `hamlet` and `hamlet_chief` tables, and founding a hamlet.

pub(super) mod limits;
mod tier;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::{
    Found, FoundAnswer, FoundRefusal, HamletId, ProfileId, Refusable,
    Tier,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Mode;

use super::act::{Act, Decided, Decision};
use super::callers::RealmWide;
use super::calls::Made;
use super::change::{self, Affected, Rows, Writer};
use super::world::Part;
use super::{hire, record, steps, tasks, tree};

/// The version of `schema.sql` the realm's row is written with.
const SCHEMA_VERSION: i64 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hamlet {
    pub id: HamletId,
    pub mayor: AgentNumber,
    pub tier: Tier,
    pub limits: limits::Limits,
}

impl Hamlet {
    /// The most hires the hamlet holds (H6).
    pub fn most(&self) -> u32 {
        todo!()
    }
}

pub(super) async fn hamlet(
    rows: &Rows<'_>,
    id: &HamletId,
) -> Result<Option<Hamlet>> {
    todo!()
}

pub(super) async fn mayors(
    rows: &Rows<'_>,
) -> Result<Vec<AgentNumber>> {
    Ok(rows
        .values::<u32>("SELECT mayor FROM hamlet", Vec::new())
        .await?
        .into_iter()
        .map(AgentNumber)
        .collect())
}

/// A new hamlet's id, a random UUID.
pub(super) fn fresh() -> HamletId {
    HamletId(uuid::Uuid::new_v4().to_string())
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The realm's row and its one chief, at the first start.
    First,
    /// A hamlet, with the chief as its chief and `mayor` as its
    /// mayor.
    Found {
        hamlet: HamletId,
        name: String,
        mayor: AgentNumber,
        tier: Tier,
    },
    Mayor {
        hamlet: HamletId,
        mayor: AgentNumber,
    },
    Tier {
        hamlet: HamletId,
        tier: Tier,
    },
    Limits(limits::Kept),
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::First => first(writer).await,
        Change::Found {
            hamlet,
            name,
            mayor,
            tier,
        } => found(writer, hamlet, name, mayor, tier).await,
        Change::Mayor { hamlet, mayor } => {
            set_mayor(writer, hamlet, mayor).await
        }
        Change::Tier { hamlet, tier } => {
            tier::set(writer, hamlet, tier).await
        }
        Change::Limits(kept) => limits::set(writer, kept).await,
    }
}

async fn first(writer: &Writer<'_>) -> Result<Affected> {
    writer
        .execute(
            "INSERT INTO realm (id, schema, limits) VALUES (1, ?1, \
             ?2)",
            vec![
                BindParam::Integer(SCHEMA_VERSION),
                BindParam::Text(limits::new_realm()?),
            ],
        )
        .await?;
    writer
        .execute("INSERT INTO chief (id) VALUES (1)", Vec::new())
        .await?;
    Ok(seen(&[Part::Spend]))
}

async fn found(
    writer: &Writer<'_>,
    hamlet: HamletId,
    name: String,
    mayor: AgentNumber,
    tier: Tier,
) -> Result<Affected> {
    writer
        .execute(
            "INSERT INTO hamlet (id, name, mayor, tier, folder, \
             mode, limits, at) VALUES (?1, ?2, ?3, ?4, NULL, ?5, \
             ?6, ?7)",
            vec![
                BindParam::Text(hamlet.0.clone()),
                BindParam::Text(name),
                BindParam::Integer(i64::from(mayor.0)),
                BindParam::Text(change::name(&tier)?),
                BindParam::Text(change::name(&Mode::Manual)?),
                BindParam::Text(limits::new_hamlet()?),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    writer
        .execute(
            "INSERT INTO hamlet_chief (hamlet, chief) VALUES (?1, 1)",
            vec![BindParam::Text(hamlet.0)],
        )
        .await?;
    Ok(seen(&[Part::Hamlets, Part::Spend]))
}

fn seen(parts: &[Part]) -> Affected {
    Affected {
        seen: parts.iter().copied().collect(),
        ..Affected::default()
    }
}

async fn set_mayor(
    writer: &Writer<'_>,
    hamlet: HamletId,
    mayor: AgentNumber,
) -> Result<Affected> {
    todo!()
}

pub(super) fn first_start() -> Vec<change::Change> {
    let hand = Hand::NUMBER;
    vec![
        Change::First.into(),
        tree::Change::Hand.into(),
        tasks::Change::Standing {
            agent: hand,
            payer: Payer::Realm,
        }
        .into(),
        steps::Change::Begin {
            row: hire::row(hand),
        }
        .into(),
    ]
}

pub(super) struct Founding {
    /// The mayor's profile, when the act names one the realm holds.
    profile: Option<tree::Profile>,
    hamlet: HamletId,
    mayor: AgentNumber,
    blank: ProfileId,
}

impl<By: RealmWide> Act<By> for Found {
    type Output = FoundAnswer<FoundRefusal>;
    type Loaded = Founding;
    /// The hamlet and its mayor.
    type Done = (HamletId, AgentNumber);

    async fn load(
        &self,
        _by: &By,
        rows: &Rows<'_>,
    ) -> Result<Founding> {
        let profile = match self.mayor {
            Some(id) => tree::profile(rows, id).await?,
            None => None,
        };
        Ok(Founding {
            profile,
            hamlet: fresh(),
            mayor: tree::fresh(rows).await?,
            blank: tree::fresh_profile(rows).await?,
        })
    }

    fn decide(
        &self,
        by: &By,
        founding: Founding,
    ) -> Decision<Self::Done, Self::Output> {
        if self.name.trim().is_empty() {
            return Decision::Answer(FoundAnswer::refused(
                FoundRefusal::EmptyName,
            ));
        }
        let of = match (self.mayor, founding.profile) {
            (None, _) => tree::Of::Blank(founding.blank),
            (Some(_), Some(profile)) => tree::Of::Profile(profile.id),
            (Some(profile), None) => {
                return Decision::Answer(FoundAnswer::refused(
                    FoundRefusal::NoSuchProfile { profile },
                ));
            }
        };
        let Founding { hamlet, mayor, .. } = founding;
        Decision::Change(
            Decided::new((hamlet.clone(), mayor))
                .change(Change::Found {
                    hamlet: hamlet.clone(),
                    name: self.name.clone(),
                    mayor,
                    tier: self.tier,
                })
                .change(tree::Change::Hire {
                    agent: mayor,
                    hamlet: hamlet.clone(),
                    lead: None,
                    of,
                    purpose: self.purpose.clone(),
                })
                .change(tasks::Change::Standing {
                    agent: mayor,
                    payer: Payer::Hamlet {
                        hamlet: hamlet.clone(),
                    },
                })
                .change(steps::Change::Begin {
                    row: hire::row(mayor),
                })
                .record(record::Entry::Founded {
                    hamlet,
                    by: by.by(),
                }),
        )
    }

    fn answer(
        &self,
        (hamlet, mayor): Self::Done,
        _made: &Made,
    ) -> Self::Output {
        FoundAnswer::Founded { hamlet, mayor }
    }
}

/// The payer of an agent's own spending: its hamlet, or the realm
/// for the Hand.
pub(super) fn payer_of(lineage: &tree::Lineage) -> Payer {
    todo!()
}
