//! A hamlet's tier, set by the chief (H6).

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::realm::{
    Done, HamletId, Refusable, SetTier, Tier, TierRefusal,
};
use hamlet_messages::sender::Client;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::Decider as _;
use super::super::calls::Made;
use super::super::change::{self, Affected, Rows, Writer};
use super::super::world::Part;
use super::super::{record, tree};

pub(in crate::realm) struct Tiering {
    hamlet: Option<super::Hamlet>,
    living: u32,
}

impl Act<Client> for SetTier {
    type Output = Done<TierRefusal>;
    type Loaded = Tiering;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Tiering> {
        Ok(Tiering {
            hamlet: super::hamlet(rows, &self.hamlet).await?,
            living: tree::living(rows, &self.hamlet).await?,
        })
    }

    fn decide(
        &self,
        by: &Client,
        tiering: Tiering,
    ) -> Decision<(), Self::Output> {
        let Some(hamlet) = tiering.hamlet else {
            return Decision::Answer(Done::refused(
                TierRefusal::NoSuchHamlet,
            ));
        };
        if hamlet.tier == self.tier {
            return Decision::Again(());
        }
        let tiered = super::Hamlet {
            tier: self.tier,
            ..hamlet
        };
        if tiering.living > tiered.most() {
            return Decision::Answer(Done::refused(
                TierRefusal::TooSmall {
                    living: tiering.living,
                },
            ));
        }
        Decision::Change(
            Decided::new(())
                .change(super::Change::Tier {
                    hamlet: self.hamlet.clone(),
                    tier: self.tier,
                })
                .record(record::Entry::Tier {
                    hamlet: self.hamlet.clone(),
                    tier: self.tier,
                    by: by.by(),
                }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

pub(in crate::realm) async fn set(
    writer: &Writer<'_>,
    hamlet: HamletId,
    tier: Tier,
) -> Result<Affected> {
    writer
        .execute(
            "UPDATE hamlet SET tier = ?2 WHERE id = ?1",
            vec![
                BindParam::Text(hamlet.0),
                BindParam::Text(change::case_name(&tier)?),
            ],
        )
        .await?;
    Ok(Affected::seen([Part::Hamlets]))
}

#[cfg(test)]
mod tests {
    use hamlet_messages::workspace::AgentNumber;
    use hamlet_messages::workspace::reach::Mode;

    use super::super::limits::Limits;
    use super::*;

    #[test]
    fn a_tier_holds_as_many_living_hires_as_its_most() {
        let hamlet = HamletId("h".to_owned());
        let set = SetTier {
            hamlet: hamlet.clone(),
            tier: Tier::Fifty,
        };
        let tiering = |living| Tiering {
            hamlet: Some(super::super::Hamlet {
                id: hamlet.clone(),
                mayor: AgentNumber(2),
                tier: Tier::Hundred,
                limits: Limits::default(),
                folder: None,
                mode: Mode::Manual,
            }),
            living,
        };

        assert!(matches!(
            set.decide(&Client, tiering(50)),
            Decision::Change(_)
        ));
        assert_eq!(
            set.decide(&Client, tiering(51)),
            Decision::Answer(Done::Refused {
                why: TierRefusal::TooSmall { living: 51 }
            })
        );
    }
}
