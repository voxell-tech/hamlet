//! Budgets (H8, H12): the three meters of spend, and the arithmetic
//! that decides whether an agent may spend more.

mod ledger;
mod report;
mod spending;

use chrono::{Datelike, Local, NaiveDate, Timelike};
use hamlet_home::limits::{Caps, Limits, Period};
use hamlet_model::catalogue::Price;
use hamlet_model::provider::CallRecord;
use serde_json::Value;

pub use crate::budget::ledger::{Books, Charge, Ledger, new_task};
pub use crate::budget::report::{AgentBudget, Report, report};

/// Spend on the three meters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Amount {
    pub tokens: u64,
    /// Millionths of the provider's account currency.
    pub micros: u64,
    /// Model time.
    pub ms: u64,
}

/// One of the three things a budget counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meter {
    Tokens,
    Cost,
    Time,
}

fn micros(cost: f64) -> u64 {
    (cost.max(0.0) * 1e6).round() as u64
}

impl Amount {
    /// The spend of one model call. `estimate` stands in for the
    /// tokens when the provider reported none. The cost is what
    /// the provider says it billed, else what the tokens come to
    /// at `price`.
    pub fn of(
        call: &CallRecord,
        estimate: u64,
        price: Option<Price>,
    ) -> Amount {
        let reported = call.input_tokens.is_some()
            || call.output_tokens.is_some();
        let (input, output) = if reported {
            (
                call.input_tokens.unwrap_or(0),
                call.output_tokens.unwrap_or(0),
            )
        } else {
            (estimate, 0)
        };
        let priced = price.map_or(0.0, |p| p.of(input, output));
        Amount {
            tokens: input + output,
            micros: micros(call.billed_cost.unwrap_or(priced)),
            ms: call.duration_ms,
        }
    }

    fn of_line(line: &Value) -> Amount {
        let n = |key: &str| line[key].as_u64().unwrap_or(0);
        Amount {
            tokens: n("tokens"),
            micros: n("micros"),
            ms: n("ms"),
        }
    }

    pub fn add(&mut self, other: Amount) {
        self.tokens = self.tokens.saturating_add(other.tokens);
        self.micros = self.micros.saturating_add(other.micros);
        self.ms = self.ms.saturating_add(other.ms);
    }

    fn plus(mut self, other: Amount) -> Amount {
        self.add(other);
        self
    }

    /// Each meter beside its cap, where it has one.
    fn against(&self, caps: &Caps) -> [(Meter, u64, Option<u64>); 3] {
        [
            (Meter::Tokens, self.tokens, caps.tokens),
            (Meter::Cost, self.micros, caps.cost.map(micros)),
            (
                Meter::Time,
                self.ms,
                caps.minutes.map(|m| m.saturating_mul(60_000)),
            ),
        ]
    }

    /// The first meter that has reached its cap.
    pub fn reached(&self, caps: &Caps) -> Option<Meter> {
        self.against(caps)
            .into_iter()
            .find(|(_, spent, cap)| cap.is_some_and(|c| *spent >= c))
            .map(|(meter, ..)| meter)
    }

    /// The first meter that would pass its cap.
    fn passed(&self, caps: &Caps) -> Option<Meter> {
        self.against(caps)
            .into_iter()
            .find(|(_, spent, cap)| cap.is_some_and(|c| *spent > c))
            .map(|(meter, ..)| meter)
    }
}

/// Spend beside the caps it counts against.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Standing {
    pub spent: Amount,
    pub caps: Caps,
}

pub(crate) fn shown(meter: Meter, n: u64) -> String {
    match meter {
        Meter::Tokens => format_tokens(n),
        Meter::Cost => format_cost(n as f64 / 1e6),
        Meter::Time => format!("{} model time", format_duration(n)),
    }
}

impl Standing {
    /// The share that is left on the meter nearest its cap, from 0
    /// to 1. `None` when nothing is capped.
    pub fn left(&self) -> Option<f32> {
        self.spent
            .against(&self.caps)
            .into_iter()
            .filter_map(|(_, spent, cap)| {
                Some(spent as f64 / cap?.max(1) as f64)
            })
            .max_by(f64::total_cmp)
            .map(|used| (1.0 - used).clamp(0.0, 1.0) as f32)
    }

    /// The rest on the meter nearest its cap, as it is shown:
    /// "$1.90", "1.2M tokens". `None` when nothing is capped.
    pub fn remains(&self) -> Option<String> {
        self.spent
            .against(&self.caps)
            .into_iter()
            .filter_map(|(meter, spent, cap)| {
                let cap = cap?;
                Some((
                    meter,
                    spent,
                    cap,
                    spent as f64 / cap.max(1) as f64,
                ))
            })
            .max_by(|a, b| a.3.total_cmp(&b.3))
            .map(|(meter, spent, cap, _)| {
                shown(meter, cap.saturating_sub(spent))
            })
    }

    /// One meter as "spent of cap".
    fn meter(&self, meter: Meter) -> String {
        let (_, spent, cap) = self
            .spent
            .against(&self.caps)
            .into_iter()
            .find(|(m, ..)| *m == meter)
            .expect("every meter is listed");
        match cap {
            Some(cap) => format!(
                "{} of {}",
                shown(meter, spent),
                shown(meter, cap)
            ),
            None => shown(meter, spent),
        }
    }

    /// The meter nearest its cap as "spent of cap". The cost alone
    /// when nothing is capped.
    pub fn summary(&self) -> String {
        let nearest = self
            .spent
            .against(&self.caps)
            .into_iter()
            .filter_map(|(meter, spent, cap)| {
                let cap = cap?;
                Some((meter, spent as f64 / cap.max(1) as f64))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(Meter::Cost, |(meter, _)| meter);
        self.meter(nearest)
    }
}

/// The day it is, where the chief is.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// The first day of the period `today` is in.
pub(crate) fn first_day(
    period: Period,
    today: NaiveDate,
) -> NaiveDate {
    match period {
        Period::Day => today,
        Period::Month => today.with_day(1).unwrap_or(today),
    }
}

/// The share of the period that has gone by, from 0 to 1, in steps
/// of a two-hundredth so that it changes seldom.
pub fn gone(period: Period) -> f32 {
    let now = Local::now();
    let day = now.num_seconds_from_midnight() as f32 / 86_400.0;
    let share = match period {
        Period::Day => day,
        Period::Month => {
            let first = now.date_naive().with_day(1);
            let days = first
                .and_then(|first| {
                    first.checked_add_months(chrono::Months::new(1))
                })
                .zip(first)
                .map_or(30, |(next, first)| {
                    (next - first).num_days()
                });
            (now.day0() as f32 + day) / days as f32
        }
    };
    (share.clamp(0.0, 1.0) * 200.0).floor() / 200.0
}

fn period_words(period: Period) -> (&'static str, &'static str) {
    match period {
        Period::Day => ("today", "at midnight"),
        Period::Month => ("this month", "on the first of next month"),
    }
}

/// Reason nobody in the hamlet may make another model call for the
/// task, as a statement of what ran out and who can change it. `None`
/// when they may.
pub fn stop(limits: &Limits, books: &Books) -> Option<String> {
    let (now, resets) = period_words(limits.budget.period);
    let hamlet = Standing {
        spent: books.hamlet,
        caps: limits.budget.caps(),
    };
    if let Some(meter) = hamlet.spent.reached(&hamlet.caps) {
        return Some(format!(
            "The hamlet has spent its budget for {now} ({}). It \
             starts again {resets}. Only the chief can raise it, on \
             the hamlet's Bylaws page.",
            hamlet.meter(meter)
        ));
    }
    let task = Standing {
        spent: books.task,
        caps: limits.task.caps(),
    };
    task.spent.reached(&task.caps).map(|meter| {
        format!(
            "This task has spent its cap ({}). The chief can give \
             the rest of the work as a new task, or raise the cap \
             under One Task on the hamlet's Bylaws page.",
            task.meter(meter)
        )
    })
}

/// The price of a road.
pub(crate) fn road_price(limits: &Limits) -> Amount {
    Amount {
        tokens: limits.road.tokens,
        micros: micros(limits.road.cost),
        ms: limits.road.minutes.saturating_mul(60_000),
    }
}

/// Reason the hamlet cannot pay `price` on top of what it has spent.
/// `None` when it can.
pub(crate) fn cannot_afford(
    limits: &Limits,
    books: &Books,
    price: Amount,
) -> Option<String> {
    let after = Standing {
        spent: books.hamlet.plus(price),
        caps: limits.budget.caps(),
    };
    let meter = after.spent.passed(&after.caps)?;
    let (now, _) = period_words(limits.budget.period);
    Some(format!(
        "the hamlet cannot pay for it {now}: it costs {} and would \
         bring the hamlet to {}. Only the chief can raise the \
         budget, on the hamlet's Bylaws page",
        shown(
            meter,
            match meter {
                Meter::Tokens => price.tokens,
                Meter::Cost => price.micros,
                Meter::Time => price.ms,
            }
        ),
        after.meter(meter)
    ))
}

/// A rough count of the tokens in a request and its reply, for a
/// provider that reports none: four characters a token.
pub fn estimate(
    request: &[Value],
    tools: &[Value],
    reply: &Value,
) -> u64 {
    let chars = |v: &Value| v.to_string().len() as u64;
    let sent = request.iter().chain(tools).map(chars).sum::<u64>();
    (sent + chars(reply)) / 4
}

/// A count of tokens, as it is shown.
pub fn format_tokens(n: u64) -> String {
    match n {
        0..=999 => format!("{n} tok"),
        1_000..=999_999 => format!("{:.1}k tok", n as f64 / 1e3),
        _ => format!("{:.2}M tok", n as f64 / 1e6),
    }
}

/// A cost in dollars, as it is shown.
pub fn format_cost(dollars: f64) -> String {
    // Something spent is never shown as nothing.
    if dollars > 0.0 && dollars < 0.01 {
        "<$0.01".into()
    } else {
        format!("${dollars:.2}")
    }
}

/// Model time, as it is shown.
pub fn format_duration(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        _ => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ledger::tests::{charge, day, ledger};
    use super::*;

    /// Limits whose only cap is a budget of `cents`.
    fn limits(cents: u64) -> Limits {
        let mut limits = Limits::default();
        limits.budget.cost = Some(cents as f64 / 100.0);
        limits.task.cost = None;
        limits
    }

    #[test]
    fn a_call_is_counted_on_three_meters() {
        let call = CallRecord {
            input_tokens: Some(100),
            output_tokens: Some(20),
            billed_cost: Some(0.0015),
            duration_ms: 900,
            ..CallRecord::default()
        };
        assert_eq!(
            Amount::of(&call, 7, None),
            Amount {
                tokens: 120,
                micros: 1_500,
                ms: 900
            }
        );
        // One that bills nothing costs what the catalogue says: a
        // dollar for a million in and ten for a million out.
        let unbilled = CallRecord {
            billed_cost: None,
            ..call.clone()
        };
        let price = Price {
            input: 1.0,
            output: 10.0,
        };
        assert_eq!(Amount::of(&unbilled, 7, Some(price)).micros, 300);
        assert_eq!(Amount::of(&call, 7, Some(price)).micros, 1_500);
        // A provider that reports no tokens is charged the estimate.
        let silent = CallRecord {
            duration_ms: 5,
            ..CallRecord::default()
        };
        assert_eq!(Amount::of(&silent, 7, None).tokens, 7);
        let reply = json!({"role": "assistant", "content": "four"});
        let sent = [json!({"role": "user", "content": "say four"})];
        assert!(estimate(&sent, &[], &reply) > 10);
    }

    #[test]
    fn the_hamlet_and_a_task_each_stop_with_who_can_change_it() {
        let (_dir, l) = ledger("stop");
        let books =
            |l: &Ledger| Books::read(l, Period::Day, day(4), "t");
        let raised = limits(5_000);
        let limits = limits(1_000);
        charge(&l, day(4), "scout", 100);
        assert_eq!(stop(&limits, &books(&l)), None);

        let mut capped = limits.clone();
        capped.task.cost = Some(1.0);
        let said = stop(&capped, &books(&l)).unwrap();
        assert!(
            said.contains("This task has spent its cap")
                && said.contains("One Task"),
            "{said}"
        );

        charge(&l, day(4), "mayor", 900);
        let said = stop(&limits, &books(&l)).unwrap();
        assert!(
            said.contains(
                "The hamlet has spent its budget for today"
            ) && said.contains("Only the chief"),
            "{said}"
        );
        // The chief raises the budget and the next check passes.
        assert_eq!(stop(&raised, &books(&l)), None);
    }

    #[test]
    fn time_and_cost_are_capped_like_tokens() {
        let mut limits = limits(u64::MAX);
        limits.budget.minutes = Some(1);
        let mut books = Books::default();
        books.hamlet.ms = 59_999;
        assert_eq!(stop(&limits, &books), None);
        books.hamlet.ms = 60_000;
        let said = stop(&limits, &books).unwrap();
        assert!(said.contains("1m model time of 1m model time"));
        limits.budget.minutes = None;
        limits.budget.cost = Some(0.5);
        books.hamlet.micros = 500_000;
        let said = stop(&limits, &books).unwrap();
        assert!(said.contains("$0.50 of $0.50"), "{said}");
    }

    #[test]
    fn a_road_is_refused_when_the_hamlet_cannot_pay() {
        let mut limits = limits(1_000);
        limits.road.cost = 3.0;
        let mut books = Books::default();
        books.hamlet.micros = 7_000_000;
        let price = road_price(&limits);
        assert_eq!(cannot_afford(&limits, &books, price), None);
        books.hamlet.micros = 7_000_001;
        let said = cannot_afford(&limits, &books, price).unwrap();
        assert!(said.contains("costs $3.00"), "{said}");
    }
}
