//! The chief's limits, a hamlet's and the realm's own: the `limits`
//! column of the `hamlet` and `realm` rows, and `SetLimits` (H22).

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::realm::budget::{self, BudgetLimit, Payer};
use hamlet_messages::realm::{
    Done, HamletId, HamletRefusal, Limit, Refusable, ReservedAct,
    Review, SetLimits,
};
use hamlet_messages::sender::Client;
use serde::{Deserialize, Serialize};

use super::super::act::{Act, Decided, Decision};
use super::super::callers::Decider as _;
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer};
use super::super::record;
use super::super::world::Part;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::realm) struct Limits {
    budget: Budget,
    pub road: RoadRules,
    pub reserved: Reserved,
    pub asking: Asking,
    /// How many days of decisions each hire is told (H17).
    pub told_days: u32,
    /// How long after a chief's task a lead may not end the hire
    /// (H7).
    pub protected_minutes: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            budget: Budget::default(),
            road: RoadRules::default(),
            reserved: Reserved::default(),
            asking: Asking::default(),
            told_days: 7,
            protected_minutes: 0,
        }
    }
}

impl Limits {
    /// A hamlet's limits from its row's `limits` column.
    pub(super) fn of_row(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }

    fn set(&mut self, limit: &Limit) -> Result<(), HamletRefusal> {
        match *limit {
            Limit::Questions { most } => self.asking.questions = most,
            Limit::Refusals { most } => self.asking.refusals = most,
            Limit::Review { review } => self.asking.review = review,
            Limit::ProtectedMinutes { minutes } => {
                self.protected_minutes = minutes;
            }
            Limit::ToldDays { days } => self.told_days = days,
            Limit::Reserved { act, reserved } => {
                *self.reserved.of(act) = reserved;
            }
            Limit::RoadPrice { micros } => self.road.price = micros,
            Limit::RoadsMost { most } => self.road.most = most,
            Limit::RoadsGrowOverDays { days } => {
                self.road.grows_over_days = days;
            }
            Limit::RoadsBetween { .. } => {
                return Err(HamletRefusal::RealmsLimit);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct RealmLimits {
    budget: Budget,
    /// Whether mayors may agree roads between their hamlets.
    roads_between: bool,
}

impl Default for RealmLimits {
    fn default() -> Self {
        Self {
            budget: Budget::default(),
            roads_between: true,
        }
    }
}

/// The leads' acts a chief keeps for itself.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(default)]
pub(in crate::realm) struct Reserved {
    pub end: bool,
    pub hire: bool,
    pub merge: bool,
    pub roads: bool,
}

impl Reserved {
    fn of(&mut self, act: ReservedAct) -> &mut bool {
        match act {
            ReservedAct::End => &mut self.end,
            ReservedAct::Hire => &mut self.hire,
            ReservedAct::Merge => &mut self.merge,
            ReservedAct::Roads => &mut self.roads,
        }
    }
}

/// What may ask a chief in one turn (H10, H11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::realm) struct Asking {
    pub questions: u32,
    pub refusals: u32,
    pub review: Review,
}

impl Default for Asking {
    fn default() -> Self {
        Self {
            questions: 3,
            refusals: 3,
            review: Review::Remote,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::realm) struct Budget {
    /// In millionths of a dollar. `None` bounds nothing.
    pub micros: Option<u64>,
    /// Model time. `None` bounds nothing.
    pub ms: Option<u64>,
    pub period: Period,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            micros: Some(50_000_000),
            ms: None,
            period: Period::Day,
        }
    }
}

impl Budget {
    fn set(&mut self, limit: &BudgetLimit) {
        match *limit {
            BudgetLimit::Cost { micros } => {
                self.micros = Some(micros);
            }
            BudgetLimit::Time { ms } => {
                self.ms = Some(ms);
            }
            BudgetLimit::Period { period } => {
                self.period = match period {
                    budget::Period::Day => Period::Day,
                    budget::Period::Month => Period::Month,
                };
            }
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(in crate::realm) enum Period {
    Day,
    Month,
}

/// The price of a laid road and how many a hamlet holds (H3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::realm) struct RoadRules {
    /// In millionths of a dollar.
    pub price: u64,
    pub most: u32,
    /// The days with no letter after which a laid road shows as
    /// grown over.
    pub grows_over_days: u32,
}

impl Default for RoadRules {
    fn default() -> Self {
        Self {
            price: 10_000,
            most: 24,
            grows_over_days: 7,
        }
    }
}

/// The `limits` column of a new hamlet's row: the defaults.
pub(super) fn new_hamlet() -> Result<String> {
    Ok(serde_json::to_string(&Limits::default())?)
}

/// The `limits` column of the realm's row at its first start: the
/// defaults.
pub(super) fn new_realm() -> Result<String> {
    Ok(serde_json::to_string(&RealmLimits::default())?)
}

pub(in crate::realm) async fn budget(
    rows: &Rows<'_>,
    payer: &Payer,
) -> Result<Budget> {
    Ok(match payer {
        Payer::Realm => realm(rows).await?.budget,
        Payer::Hamlet { hamlet } => {
            super::hamlet(rows, hamlet)
                .await?
                .with_context(|| {
                    format!("the limits of the hamlet `{}`", hamlet.0)
                })?
                .limits
                .budget
        }
    })
}

async fn realm(rows: &Rows<'_>) -> Result<RealmLimits> {
    let text = rows
        .value::<String>(
            "SELECT limits FROM realm WHERE id = 1",
            Vec::new(),
        )
        .await?
        .context("the realm's row")?;
    Ok(serde_json::from_str(&text)?)
}

/// Limits with the row that keeps them.
#[derive(Debug, PartialEq)]
pub(in crate::realm) struct Kept(Row);

#[derive(Debug, PartialEq)]
enum Row {
    Realm(RealmLimits),
    Hamlet(HamletId, Limits),
}

impl Kept {
    fn budget_mut(&mut self) -> &mut Budget {
        match &mut self.0 {
            Row::Realm(limits) => &mut limits.budget,
            Row::Hamlet(_, limits) => &mut limits.budget,
        }
    }

    fn set(&mut self, limit: &Limit) -> Result<(), HamletRefusal> {
        match (&mut self.0, limit) {
            (
                Row::Realm(limits),
                &Limit::RoadsBetween { allowed },
            ) => {
                limits.roads_between = allowed;
                Ok(())
            }
            (Row::Realm(_), _) => Err(HamletRefusal::HamletsLimit),
            (Row::Hamlet(_, limits), limit) => limits.set(limit),
        }
    }
}

impl Act<Client> for SetLimits {
    type Output = Done<HamletRefusal>;
    /// `None` for a hamlet the realm does not hold.
    type Loaded = Option<Kept>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<Kept>> {
        Ok(match &self.hamlet {
            None => Some(Kept(Row::Realm(realm(rows).await?))),
            Some(id) => {
                super::hamlet(rows, id).await?.map(|hamlet| {
                    Kept(Row::Hamlet(hamlet.id, hamlet.limits))
                })
            }
        })
    }

    fn decide(
        &self,
        by: &Client,
        kept: Option<Kept>,
    ) -> Decision<(), Self::Output> {
        let Some(mut kept) = kept else {
            return Decision::Answer(Done::refused(
                HamletRefusal::NoSuchHamlet,
            ));
        };
        if self.budget.is_empty() && self.limits.is_empty() {
            return Decision::Answer(Done::Done);
        }
        for limit in &self.budget {
            kept.budget_mut().set(limit);
        }
        for limit in &self.limits {
            if let Err(refusal) = kept.set(limit) {
                return Decision::Answer(Done::refused(refusal));
            }
        }
        Decision::Change(
            Decided::new(())
                .change(super::Change::Limits(kept))
                .record(record::Entry::Limits {
                    hamlet: self.hamlet.clone(),
                    by: by.by(),
                }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

pub(super) async fn set(
    writer: &Writer<'_>,
    kept: Kept,
) -> Result<Affected> {
    let (sql, params) = match kept.0 {
        Row::Realm(limits) => (
            "UPDATE realm SET limits = ?1 WHERE id = 1",
            vec![BindParam::Text(serde_json::to_string(&limits)?)],
        ),
        Row::Hamlet(id, limits) => (
            "UPDATE hamlet SET limits = ?2 WHERE id = ?1",
            vec![
                BindParam::Text(id.0),
                BindParam::Text(serde_json::to_string(&limits)?),
            ],
        ),
    };
    writer.execute(sql, params).await?;
    Ok(Affected::seen([Part::Spend]))
}

#[cfg(test)]
mod tests {
    use hamlet_messages::workspace::AgentNumber;

    use super::super::super::testing::Started;
    use super::super::super::{change, record};
    use super::*;

    fn set(
        hamlet: Option<&str>,
        budget: Vec<BudgetLimit>,
    ) -> SetLimits {
        SetLimits {
            hamlet: hamlet.map(|hamlet| HamletId(hamlet.to_owned())),
            budget,
            limits: Vec::new(),
        }
    }

    #[test]
    fn set_limits_sets_each_limit_in_order() {
        let realm = || Some(Kept(Row::Realm(RealmLimits::default())));
        assert_eq!(
            set(Some("x"), Vec::new()).decide(&Client, None),
            Decision::Answer(Done::refused(
                HamletRefusal::NoSuchHamlet
            ))
        );
        assert_eq!(
            set(None, Vec::new()).decide(&Client, realm()),
            Decision::Answer(Done::Done)
        );
        let limits = vec![
            BudgetLimit::Cost { micros: 1 },
            BudgetLimit::Time { ms: 2 },
            BudgetLimit::Cost { micros: 3 },
            BudgetLimit::Period {
                period: budget::Period::Month,
            },
        ];
        let act = SetLimits {
            limits: vec![Limit::RoadsBetween { allowed: false }],
            ..set(None, limits)
        };
        let Decision::Change(decided) = act.decide(&Client, realm())
        else {
            panic!("the limits change");
        };
        let budget = Budget {
            micros: Some(3),
            ms: Some(2),
            period: Period::Month,
        };
        let kept = Kept(Row::Realm(RealmLimits {
            budget,
            roads_between: false,
        }));
        assert_eq!(
            decided.changes,
            [change::Change::from(super::super::Change::Limits(
                kept
            ))]
        );
        assert_eq!(
            decided.entries,
            [record::Entry::Limits {
                hamlet: None,
                by: record::By::Chief
            }]
        );
    }

    #[test]
    fn a_row_with_no_limits_reads_the_defaults() {
        assert_eq!(Limits::of_row("{}").unwrap(), Limits::default());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn limits_are_kept_on_their_row() {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        let cost = vec![BudgetLimit::Cost { micros: 7 }];
        for named in [None, Some("h")] {
            let act = set(named, cost.clone());
            let kept =
                act.load(&Client, &started.rows()).await.unwrap();
            let Decision::Change(decided) = act.decide(&Client, kept)
            else {
                panic!("the limits change");
            };
            for made in decided.changes {
                change::commit(
                    &started.realm,
                    &started.ctx,
                    vec![made],
                )
                .await
                .unwrap();
            }
        }
        let rows = started.rows();

        for payer in [Payer::Realm, Payer::Hamlet { hamlet }] {
            assert_eq!(
                budget(&rows, &payer).await.unwrap().micros,
                Some(7)
            );
        }
        let missing = set(Some("x"), cost);
        assert_eq!(missing.load(&Client, &rows).await.unwrap(), None);
        started.stop().await;
    }
}
