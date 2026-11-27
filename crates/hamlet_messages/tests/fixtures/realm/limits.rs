//! The fixtures of limits, budgets and spending.

use hamlet_messages::realm::budget::{
    Budget, BudgetLimit, Charge, MaySpend, Payer, Period,
    ResetBudget, SpendRefusal,
};
use hamlet_messages::realm::oversight::Hold;
use hamlet_messages::realm::{
    HamletRefusal, HowEnded, Limit, ReservedAct, Review, SetLimits,
    SetTier, Standing, Tier, TierRefusal,
};

use super::super::{Fixtures, call};
use super::{HIRE, asked, done, hamlet, standing_task};

pub(super) fn the_limits_and_the_spending(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_set_tier",
        &asked(SetTier {
            hamlet: hamlet(),
            tier: Tier::TwoHundred,
        }),
    );
    done(
        fixtures,
        "realm/chief_set_tier",
        vec![
            (
                "no_such_hamlet",
                TierRefusal::NoSuchHamlet { id: hamlet() },
            ),
            ("too_small", TierRefusal::TooSmall { living: 120 }),
        ],
    );
    fixtures.args(
        "realm/chief_set_limits",
        &asked(SetLimits {
            hamlet: Some(hamlet()),
            budget: vec![
                BudgetLimit::Cost { micros: 5_000_000 },
                BudgetLimit::Time { ms: 3_600_000 },
            ],
            limits: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.realm",
        &asked(SetLimits {
            hamlet: None,
            budget: vec![BudgetLimit::Cost { micros: 20_000_000 }],
            limits: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.no_caps",
        &asked(SetLimits {
            hamlet: Some(hamlet()),
            budget: vec![BudgetLimit::NoCost, BudgetLimit::NoTime],
            limits: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.limits",
        &asked(SetLimits {
            hamlet: Some(hamlet()),
            budget: vec![BudgetLimit::Period {
                period: Period::Month,
            }],
            limits: vec![
                Limit::Questions { most: 3 },
                Limit::Refusals { most: 2 },
                Limit::Review {
                    review: Review::Always,
                },
                Limit::Review {
                    review: Review::Remote,
                },
                Limit::Review {
                    review: Review::Never,
                },
                Limit::ProtectedMinutes { minutes: 30 },
                Limit::ToldDays { days: 7 },
                Limit::Reserved {
                    act: ReservedAct::End,
                    reserved: true,
                },
                Limit::Reserved {
                    act: ReservedAct::Hire,
                    reserved: false,
                },
                Limit::Reserved {
                    act: ReservedAct::Merge,
                    reserved: true,
                },
                Limit::Reserved {
                    act: ReservedAct::Roads,
                    reserved: true,
                },
                Limit::RoadPrice { micros: 10_000 },
                Limit::RoadsMost { most: 100 },
                Limit::RoadsGrowOverDays { days: 30 },
            ],
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.roads_between",
        &asked(SetLimits {
            hamlet: None,
            budget: vec![BudgetLimit::Period {
                period: Period::Day,
            }],
            limits: vec![Limit::RoadsBetween { allowed: true }],
        }),
    );
    fixtures.args(
        "realm/chief_reset_budget",
        &asked(ResetBudget {
            payer: Payer::Hamlet { hamlet: hamlet() },
        }),
    );
    fixtures.args(
        "realm/chief_reset_budget.realm",
        &asked(ResetBudget {
            payer: Payer::Realm,
        }),
    );
    done(
        fixtures,
        "realm/chief_set_limits",
        vec![
            (
                "no_such_hamlet",
                HamletRefusal::NoSuchHamlet { id: hamlet() },
            ),
            ("misshapen_limits", HamletRefusal::MisshapenLimits),
        ],
    );
    done(
        fixtures,
        "realm/chief_reset_budget",
        vec![(
            "no_such_hamlet",
            HamletRefusal::NoSuchHamlet { id: hamlet() },
        )],
    );
    fixtures.args(
        "realm/may_spend",
        &MaySpend {
            task: standing_task(),
        },
    );
    done(
        fixtures,
        "realm/may_spend",
        vec![
            (
                "you_left",
                SpendRefusal::Standing(Standing::YouLeft {
                    how: HowEnded::ByTheChief,
                    why: None,
                }),
            ),
            (
                "not_a_member",
                SpendRefusal::Standing(Standing::NotAMember),
            ),
            (
                "no_such_task",
                SpendRefusal::Budget(Budget::NoSuchTask),
            ),
            (
                "over_cost",
                SpendRefusal::Budget(Budget::OverCost {
                    payer: Payer::Hamlet { hamlet: hamlet() },
                    spent_micros: 5_000_120,
                    budget_micros: 5_000_000,
                }),
            ),
            (
                "over_time",
                SpendRefusal::Budget(Budget::OverTime {
                    payer: Payer::Realm,
                    spent_ms: 3_600_400,
                    budget_ms: 3_600_000,
                }),
            ),
            (
                "over_allowance",
                SpendRefusal::Budget(Budget::OverAllowance {
                    hire: HIRE,
                    name: "hire#12 (Mira)".into(),
                    spent_micros: 2_000_300,
                    allowance_micros: 2_000_000,
                }),
            ),
            (
                "held",
                SpendRefusal::Held(Hold::Objection {
                    task: standing_task(),
                }),
            ),
            (
                "withdrawn",
                SpendRefusal::Held(Hold::Withdrawn {
                    task: standing_task(),
                }),
            ),
        ],
    );
    fixtures.args(
        "realm/charge",
        &Charge {
            call: call("model-call-1"),
            task: standing_task(),
            micros: 1_250,
            took_ms: 830,
            tokens: None,
        },
    );
    fixtures.args(
        "realm/charge.tokens",
        &Charge {
            call: call("model-call-2"),
            task: standing_task(),
            micros: 2_500,
            took_ms: 1_200,
            tokens: Some(1_480),
        },
    );
    fixtures.answer("realm/charge", &());
}
