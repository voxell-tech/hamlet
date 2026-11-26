//! The realm's row, the chief and the hamlets: the `realm`, `chief`,
//! `hamlet` and `hamlet_chief` tables, and founding a hamlet.

pub(super) mod hamlet;
pub(super) mod limits;
mod tier;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use hamlet_actor::{Action, BindParam, Ctx, Handler};
use hamlet_messages::profile::VersionName;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::{
    Asked, Found, FoundAnswer, FoundRefusal, HamletId, ProfileId,
    Refusable, Tier, Tool,
};
use hamlet_messages::sender::{Hand, sender};
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Mode;

use super::act::{self, Act, Decided, Decision};
use super::callers::{ByHand, RealmWide, Witness};
use super::calls::Made;
use super::change::{self, Affected, Rows, Writer, number};
use super::tree::{Pinned, Pinning, Unanswered, Versioned};
use super::world::Part;
use super::{
    Realm, hire, marker, profiles, record, steps, tasks, tree,
};

const SCHEMA_VERSION: i64 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hamlet {
    pub id: HamletId,
    pub mayor: AgentNumber,
    pub tier: Tier,
    pub limits: limits::Limits,
    pub folder: Option<PathBuf>,
    pub mode: Mode,
}

impl Hamlet {
    /// The most hires the hamlet holds (H6).
    pub fn most(&self) -> u32 {
        match self.tier {
            Tier::Fifty => 50,
            Tier::Hundred => 100,
            Tier::TwoHundred => 200,
            Tier::FiveHundred => 500,
        }
    }
}

pub(super) async fn hamlet(
    rows: &Rows<'_>,
    id: &HamletId,
) -> Result<Option<Hamlet>> {
    let Some((mayor, tier, limits, folder, mode)) = rows
        .row::<(u32, String, String, Option<String>, String)>(
            "SELECT mayor, tier, limits, folder, mode FROM hamlet \
             WHERE id = ?1",
            vec![BindParam::Text(id.0.clone())],
        )
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(Hamlet {
        id: id.clone(),
        mayor: AgentNumber(mayor),
        tier: change::from_case_name(tier)?,
        limits: limits::Limits::of_row(&limits)?,
        folder: folder.map(PathBuf::from),
        mode: change::from_case_name(mode)?,
    }))
}

/// Whether a hamlet besides `id` lives.
pub(super) async fn another_lives(
    rows: &Rows<'_>,
    id: &HamletId,
) -> Result<bool> {
    Ok(rows
        .row::<(u32,)>(
            "SELECT 1 FROM hamlet WHERE id != ?1 LIMIT 1",
            vec![BindParam::Text(id.0.clone())],
        )
        .await?
        .is_some())
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

pub(super) async fn name(rows: &Rows<'_>) -> Result<String> {
    named(rows).await?.context("the realm has no chief")
}

/// The chief's name, once the realm's first start has kept it.
pub(super) async fn named(rows: &Rows<'_>) -> Result<Option<String>> {
    rows.value::<String>("SELECT name FROM chief", Vec::new())
        .await
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    First,
    Chief {
        name: String,
    },
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
        Change::Chief { name } => {
            let named = writer
                .execute(
                    "INSERT INTO chief (id, name) VALUES (1, ?1) ON \
                     CONFLICT (id) DO UPDATE SET name = \
                     excluded.name WHERE name != excluded.name",
                    vec![BindParam::Text(name)],
                )
                .await?;
            Ok(if named > 0 {
                Affected::seen([Part::State])
            } else {
                Affected::default()
            })
        }
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
    Ok(Affected::seen([Part::Spend]))
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
                number(mayor),
                BindParam::Text(change::case_name(&tier)?),
                BindParam::Text(change::case_name(&Mode::Manual)?),
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
    Ok(Affected::seen([Part::Hamlets, Part::Spend]))
}

async fn set_mayor(
    writer: &Writer<'_>,
    hamlet: HamletId,
    mayor: AgentNumber,
) -> Result<Affected> {
    writer
        .execute(
            "UPDATE hamlet SET mayor = ?2 WHERE id = ?1",
            vec![BindParam::Text(hamlet.0), number(mayor)],
        )
        .await?;
    Ok(Affected::seen([Part::Hamlets]))
}

/// No changes once the realm's row is written: the host runs the
/// first start again when the start after it fails.
pub(super) async fn first_start(
    rows: &Rows<'_>,
) -> Result<Vec<change::Change>> {
    if rows
        .value::<i64>("SELECT id FROM realm", Vec::new())
        .await?
        .is_some()
    {
        return Ok(Vec::new());
    }
    let hand = Hand::NUMBER;
    Ok(vec![
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
    ])
}

pub(super) struct Founding {
    of: Result<tree::Of, FoundRefusal>,
    hamlet: HamletId,
    mayor: AgentNumber,
}

impl Versioned for Found {
    fn pin(&self) -> Option<(ProfileId, &VersionName)> {
        self.mayor.zip(self.version.as_ref())
    }
}

impl Handler<Pinning<Asked<Found>>> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Pinning(asked): Pinning<Asked<Found>>,
    ) -> Result<FoundAnswer<FoundRefusal>> {
        let name = <Asked<Found> as Action>::NAME;
        let by = sender(&ctx, &asked)?;
        let id = marker::Id::Request(asked.id);
        let key = marker::Key::of(&by, id, name, &asked.act)?;
        pinned(&self, &ctx, by, key, name, asked.act).await
    }
}

impl Handler<Pinning<ByHand<Tool<Found>>>> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Pinning(hand): Pinning<ByHand<Tool<Found>>>,
    ) -> Result<FoundAnswer<FoundRefusal>> {
        let name = <Tool<Found> as Action>::NAME;
        let by = sender(&ctx, &hand)?;
        let (key, _, found) = act::called(&by, name, hand.0)?;
        pinned(&self, &ctx, by, key, name, found).await
    }
}

/// Runs `found` by `by` under `key`, once its profile showed whether
/// it has the version named.
async fn pinned<By: RealmWide + Witness>(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    by: By,
    key: marker::Key,
    name: &'static str,
    found: Found,
) -> Result<FoundAnswer<FoundRefusal>> {
    match Pinned::asked(realm, ctx, &key, found).await? {
        Ok(pinned) => {
            act::run_keyed(realm, ctx, by, Some(key), name, pinned)
                .await
        }
        Err(why) => Ok(FoundAnswer::refused(why.into())),
    }
}

impl From<Unanswered> for FoundRefusal {
    fn from(Unanswered { label, version }: Unanswered) -> Self {
        Self::Unanswered { label, version }
    }
}

impl<By: RealmWide> Act<By> for Pinned<Found> {
    type Output = FoundAnswer<FoundRefusal>;
    type Loaded = Founding;
    /// The hamlet and its mayor.
    type Done = (HamletId, AgentNumber);

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Founding> {
        Ok(Founding {
            of: match self.act.mayor {
                Some(mayor) => {
                    match profiles::shared(rows, mayor).await? {
                        Ok(profile) => match &self.lacked {
                            Some(version) => {
                                Err(FoundRefusal::NoVersion {
                                    label: profiles::label_of(
                                        rows, profile,
                                    )
                                    .await?,
                                    version: version.clone(),
                                })
                            }
                            None => Ok(tree::Of::Profile(profile)),
                        },
                        Err(_) => Err(FoundRefusal::NoSuchProfile {
                            profile: mayor,
                        }),
                    }
                }
                None => {
                    Ok(tree::blank(rows, None, None, &by.by())
                        .await?)
                }
            },
            hamlet: fresh(),
            mayor: tree::fresh(rows).await?,
        })
    }

    fn decide(
        &self,
        by: &By,
        founding: Founding,
    ) -> Decision<Self::Done, Self::Output> {
        let found = &self.act;
        let name = match hamlet::checked_name(&found.name) {
            Ok(name) => name.to_owned(),
            Err(_) => {
                return Decision::Answer(FoundAnswer::refused(
                    FoundRefusal::BadName {
                        name: found.name.trim().to_owned(),
                    },
                ));
            }
        };
        let purpose =
            match tree::given_purpose(found.purpose.as_deref()) {
                Some(purpose) => purpose,
                None if by.by() == record::By::Chief => {
                    format!("Mayor of {name}.")
                }
                None => {
                    return Decision::Answer(FoundAnswer::refused(
                        FoundRefusal::NoPurpose,
                    ));
                }
            };
        let Founding { of, hamlet, mayor } = founding;
        let of = match of {
            Ok(of) => of,
            Err(why) => {
                return Decision::Answer(FoundAnswer::refused(why));
            }
        };
        Decision::Change(
            Decided::new((hamlet.clone(), mayor))
                .change(Change::Found {
                    hamlet: hamlet.clone(),
                    name,
                    mayor,
                    tier: found.tier,
                })
                .change(tree::Change::Hire {
                    agent: mayor,
                    hamlet: hamlet.clone(),
                    lead: None,
                    of,
                    version: found
                        .pin()
                        .map(|(_, version)| version.clone()),
                    purpose: Some(purpose.clone()),
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
                .record(record::Entry::Hired {
                    hire: mayor,
                    by: by.by(),
                })
                .record(record::Entry::Founded {
                    hamlet,
                    mayor,
                    by: by.by(),
                    why: Some(purpose),
                }),
        )
    }

    /// As today's, once its mayor is live.
    fn answers_after(
        &self,
        &(_, mayor): &Self::Done,
    ) -> Vec<steps::StepId> {
        vec![hire::row(mayor)]
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
    lineage.hamlet.as_ref().map_or(Payer::Realm, |hamlet| {
        Payer::Hamlet {
            hamlet: hamlet.id.clone(),
        }
    })
}

#[cfg(test)]
mod tests {
    use hamlet_actor::Actor as _;
    use hamlet_messages::sender::Client;

    use super::super::testing::Started;
    use super::*;

    fn found(name: &str, mayor: Option<ProfileId>) -> Pinned<Found> {
        Pinned {
            act: Found {
                name: name.to_owned(),
                mayor,
                version: None,
                tier: Tier::Fifty,
                purpose: None,
            },
            lacked: None,
        }
    }

    fn founding() -> Founding {
        Founding {
            of: Ok(tree::Of::Blank {
                profile: ProfileId(2),
                name: "Mayor".to_owned(),
                kind: profiles::Kind::Empty,
            }),
            hamlet: HamletId("h".to_owned()),
            mayor: AgentNumber(2),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_first_start_run_again_writes_nothing() {
        let started = Started::new().await;
        started
            .realm
            .clone()
            .on_create(started.ctx.clone())
            .await
            .unwrap();

        let rows = started.rows();
        for table in ["realm", "agent", "task", "step"] {
            let counted = format!("SELECT COUNT(*) FROM {table}");
            let count = rows.value::<u32>(&counted, Vec::new()).await;
            assert_eq!(
                count.unwrap(),
                Some(1),
                "the rows of `{table}`"
            );
        }
        started.stop().await;
    }

    #[test]
    fn a_found_refuses_a_bad_name_and_a_missing_profile() {
        let refused =
            |why| Decision::Answer(FoundAnswer::refused(why));
        for (name, shown) in [(" ", ""), (" a:b ", "a:b")] {
            assert_eq!(
                found(name, None).decide(&Client, founding()),
                refused(FoundRefusal::BadName {
                    name: shown.to_owned()
                })
            );
        }
        let Decision::Change(decided) =
            found(" Mill ", None).decide(&Client, founding())
        else {
            panic!("a good name founds");
        };
        assert!(decided.changes.iter().any(|change| matches!(
            change,
            change::Change::Chiefs(Change::Found { name, .. })
                if name == "Mill"
        )));
        let missing = Founding {
            of: Err(FoundRefusal::NoSuchProfile {
                profile: ProfileId(7),
            }),
            ..founding()
        };
        assert_eq!(
            found("Mill", Some(ProfileId(7)))
                .decide(&Client, missing),
            refused(FoundRefusal::NoSuchProfile {
                profile: ProfileId(7)
            })
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_hamlet_reads_back_with_its_new_mayor() {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        started
            .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
            .await;
        started
            .commit(Change::Mayor {
                hamlet: hamlet.clone(),
                mayor: AgentNumber(3),
            })
            .await
            .unwrap();
        let rows = started.rows();

        let read =
            super::hamlet(&rows, &hamlet).await.unwrap().unwrap();
        assert_eq!(
            (read.mayor, read.tier, read.most()),
            (AgentNumber(3), Tier::Fifty, 50)
        );
        assert_eq!(mayors(&rows).await.unwrap(), [AgentNumber(3)]);
        let missing = HamletId("x".to_owned());
        assert_eq!(
            super::hamlet(&rows, &missing).await.unwrap(),
            None
        );
        let lineage = async |agent| {
            tree::lineage_of(&rows, agent).await.unwrap()
        };
        assert_eq!(
            payer_of(&lineage(AgentNumber(3)).await),
            Payer::Hamlet { hamlet }
        );
        assert_eq!(
            payer_of(&lineage(Hand::NUMBER).await),
            Payer::Realm
        );
        started.stop().await;
    }
}
