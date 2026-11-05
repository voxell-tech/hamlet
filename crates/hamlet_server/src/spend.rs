//! What the realm spends (H8, H12): each payer's books for its
//! period, a task's, an agent's, and whether another model call may
//! be made.

use anyhow::{Context, Result};
use chrono::{Datelike, Timelike};
use diesel::dsl::max;
use diesel::prelude::*;
use hamlet_protocol::client as protocol;
use hamlet_store::Db;
use hamlet_store::functions::total;
use hamlet_store::ids::{AgentId, ChiefId, HamletId, TaskId};
use hamlet_store::schema::{
    attempt, budget_reset, road_charge, usage,
};

use crate::clock;
use crate::society::agents::AgentKind;
use crate::society::limits::{self, Budget, Micros, Period};
use crate::society::{agents, hires, tree, workers};

/// Spend in money, model time and tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Amount {
    pub micros: Micros,
    pub ms: i64,
    pub tokens: i64,
}

impl Amount {
    fn add(&mut self, other: Self) {
        self.micros = self.micros.saturating_add(other.micros);
        self.ms = self.ms.saturating_add(other.ms);
        self.tokens = self.tokens.saturating_add(other.tokens);
    }
}

/// `micros` in dollars to the cent, a half cent rounding up.
pub(crate) fn money(micros: Micros) -> String {
    if (1..10_000).contains(&micros) {
        return "<$0.01".into();
    }
    let cents = (i128::from(micros) + 5_000) / 10_000;
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// `dollars` in micros, to the nearest.
pub(crate) fn micros(dollars: f64) -> Micros {
    (dollars * 1_000_000.0).round() as i64
}

fn duration(ms: i64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60)
    }
}

/// When the period holding `at` began.
fn period_start(period: Period, at: i64) -> Result<i64> {
    let local = clock::local(at)?;
    let midnight = local
        .with_hour(0)
        .and_then(|t| t.with_minute(0))
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .context("local midnight")?;
    let start = match period {
        Period::Day => midnight,
        Period::Month => {
            midnight.with_day(1).context("the first of the month")?
        }
    };
    Ok(start.timestamp_millis())
}

/// What `payer`, a hamlet or the realm, spent in its period at `at`:
/// its model calls and its roads, after its last reset.
pub(crate) fn of_payer(
    db: &mut Db,
    payer: Option<&HamletId>,
    period: Period,
    at: i64,
) -> Result<Amount> {
    let start = period_start(period, at)?;
    let reset = {
        let mut query = budget_reset::table
            .filter(budget_reset::at.ge(start))
            .order(budget_reset::id.desc())
            .select((budget_reset::usage, budget_reset::road_charge))
            .into_boxed();
        query = match payer {
            Some(hamlet) => {
                query.filter(budget_reset::hamlet.eq(hamlet))
            }
            None => query.filter(budget_reset::hamlet.is_null()),
        };
        query
            .first::<(i64, i64)>(db)
            .optional()?
            .unwrap_or((-1, -1))
    };
    let mut query = usage::table
        .inner_join(
            attempt::table.on(attempt::runtime
                .eq(usage::runtime)
                .and(attempt::key.eq(usage::key))),
        )
        .filter(usage::at.ge(start))
        .filter(usage::ordinal.gt(reset.0))
        .select((
            total(usage::micros),
            total(usage::ms),
            total(usage::input + usage::output),
        ))
        .into_boxed();
    query = match payer {
        Some(hamlet) => query.filter(attempt::payer.eq(hamlet)),
        None => query.filter(attempt::payer.is_null()),
    };
    let (micros, ms, tokens) =
        query.first::<(Option<i64>, Option<i64>, Option<i64>)>(db)?;
    let mut amount = Amount {
        micros: micros.unwrap_or(0),
        ms: ms.unwrap_or(0),
        tokens: tokens.unwrap_or(0),
    };
    if let Some(hamlet) = payer {
        let roads = road_charge::table
            .filter(road_charge::hamlet.eq(hamlet))
            .filter(road_charge::at.ge(start))
            .filter(road_charge::id.gt(reset.1))
            .select(total(road_charge::micros))
            .first::<Option<i64>>(db)?;
        amount.micros =
            amount.micros.saturating_add(roads.unwrap_or(0));
    }
    Ok(amount)
}

/// What task `task` spent in its whole life.
pub(crate) fn of_task(db: &mut Db, task: TaskId) -> Result<Amount> {
    let (micros, ms, tokens) = usage::table
        .inner_join(
            attempt::table.on(attempt::runtime
                .eq(usage::runtime)
                .and(attempt::key.eq(usage::key))),
        )
        .filter(attempt::task.eq(task))
        .select((
            total(usage::micros),
            total(usage::ms),
            total(usage::input + usage::output),
        ))
        .first::<(Option<i64>, Option<i64>, Option<i64>)>(db)?;
    let roads = road_charge::table
        .filter(road_charge::task.eq(task))
        .select(total(road_charge::micros))
        .first::<Option<i64>>(db)?;
    Ok(Amount {
        micros: micros
            .unwrap_or(0)
            .saturating_add(roads.unwrap_or(0)),
        ms: ms.unwrap_or(0),
        tokens: tokens.unwrap_or(0),
    })
}

/// What the agents `ids` spent since `since`.
fn of_agents(
    db: &mut Db,
    ids: &[AgentId],
    since: i64,
) -> Result<Amount> {
    let (micros, ms, tokens) = usage::table
        .inner_join(
            attempt::table.on(attempt::runtime
                .eq(usage::runtime)
                .and(attempt::key.eq(usage::key))),
        )
        .filter(attempt::agent.eq_any(ids))
        .filter(usage::at.ge(since))
        .select((
            total(usage::micros),
            total(usage::ms),
            total(usage::input + usage::output),
        ))
        .first::<(Option<i64>, Option<i64>, Option<i64>)>(db)?;
    Ok(Amount {
        micros: micros.unwrap_or(0),
        ms: ms.unwrap_or(0),
        tokens: tokens.unwrap_or(0),
    })
}

/// What `agent` spent in its payer's period at `at`.
pub(crate) fn of_agent(
    db: &mut Db,
    agent: AgentId,
    at: i64,
) -> Result<Amount> {
    let payer = payer_of(db, agent)?;
    let period = budget_of(db, payer.as_ref())?.period;
    of_agents(db, &[agent], period_start(period, at)?)
}

fn payer_of(db: &mut Db, agent: AgentId) -> Result<Option<HamletId>> {
    crate::talk::tasks::payer_of(db, agent)
}

/// The budget of `payer`, a hamlet's or the realm's.
pub(crate) fn budget_of(
    db: &mut Db,
    payer: Option<&HamletId>,
) -> Result<Budget> {
    Ok(match payer {
        Some(hamlet) => limits::of(db, hamlet)?.budget,
        None => limits::realm(db)?.budget,
    })
}

/// Why another model call may not be made, in words for its agent and
/// its chief, or `None` when it may (H8, H12).
pub(crate) fn stopped(
    db: &mut Db,
    agent: AgentId,
    payer: Option<&HamletId>,
    task: Option<TaskId>,
    at: i64,
) -> Result<Option<String>> {
    let budget = budget_of(db, payer)?;
    let spent = of_payer(db, payer, budget.period, at)?;
    let whose = match payer {
        Some(hamlet) => format!("The hamlet `{hamlet}`'s budget"),
        None => "The realm's budget".to_owned(),
    };
    if let Some(cap) = budget.cost
        && spent.micros >= cap
    {
        return Ok(Some(format!(
            "{whose} of {} is spent. A chief can raise it, or start \
             its period again with /reset-budget.",
            money(cap)
        )));
    }
    if let Some(minutes) = budget.minutes
        && spent.ms >= clock::minutes(minutes)
    {
        return Ok(Some(format!(
            "{whose} of {} of model time is spent.",
            duration(clock::minutes(minutes))
        )));
    }
    if let Some(task) = task {
        let caps = match payer {
            Some(hamlet) => limits::of(db, hamlet)?.task,
            None => limits::TaskCaps::default(),
        };
        let spent = of_task(db, task)?;
        if let Some(cap) = caps.cost
            && spent.micros >= cap
        {
            return Ok(Some(format!(
                "This task may spend at most {}, and has.",
                money(cap)
            )));
        }
        if let Some(minutes) = caps.minutes
            && spent.ms >= clock::minutes(minutes)
        {
            return Ok(Some(format!(
                "This task may use at most {} of model time, and \
                 has.",
                duration(clock::minutes(minutes))
            )));
        }
    }
    let start = period_start(budget.period, at)?;
    for (lead, allowance) in allowances(db, agent)? {
        let mut under = tree::under(db, lead)?;
        under.push(lead);
        let mut ids = under.clone();
        for hire in &under {
            ids.extend(
                workers::of_agent(db, *hire)?.iter().map(|w| w.agent),
            );
        }
        let spent = of_agents(db, &ids, start)?;
        if spent.micros >= allowance {
            let name = crate::society::addresses::name_of(db, lead)?;
            return Ok(Some(format!(
                "`{name}`'s allowance of {} is spent. Its lead or a \
                 chief can raise it.",
                money(allowance)
            )));
        }
    }
    Ok(None)
}

/// The allowances bounding what `agent` spends: its own and every
/// lead's above it; a worker counts as its starter.
fn allowances(
    db: &mut Db,
    agent: AgentId,
) -> Result<Vec<(AgentId, Micros)>> {
    let mut at = match agents::get(db, agent)?.kind {
        AgentKind::Worker => match workers::get(db, agent)?.starter {
            Some(starter) => starter,
            None => return Ok(Vec::new()),
        },
        AgentKind::Hand => return Ok(Vec::new()),
        AgentKind::Hire => agent,
    };
    let mut out = Vec::new();
    loop {
        let Some(hire) = hires::find(db, at)? else {
            break;
        };
        if let Some(allowance) = hire.allowance {
            out.push((at, allowance));
        }
        match hire.parent {
            Some(parent) => at = parent,
            None => break,
        }
    }
    Ok(out)
}

/// Starts the budget period of `payer` again from now, keeping the
/// ledger.
pub(crate) fn reset(
    db: &mut Db,
    payer: Option<&HamletId>,
    chief: ChiefId,
    at: i64,
) -> Result<()> {
    let last_usage = usage::table
        .select(max(usage::ordinal))
        .first::<Option<i64>>(db)?
        .unwrap_or(-1);
    let last_road = road_charge::table
        .select(max(road_charge::id))
        .first::<Option<i64>>(db)?
        .unwrap_or(-1);
    diesel::insert_into(budget_reset::table)
        .values((
            budget_reset::hamlet.eq(payer),
            budget_reset::at.eq(at),
            budget_reset::chief.eq(chief),
            budget_reset::usage.eq(last_usage),
            budget_reset::road_charge.eq(last_road),
        ))
        .execute(db)?;
    Ok(())
}

/// The report of `payer`'s spend in its period at `at`, as the window
/// shows it.
pub(crate) fn report(
    db: &mut Db,
    payer: Option<&HamletId>,
    at: i64,
) -> Result<protocol::Report> {
    let budget = budget_of(db, payer)?;
    let spent = of_payer(db, payer, budget.period, at)?;
    let caps = [
        (spent.micros, budget.cost, false),
        (spent.ms, budget.minutes.map(clock::minutes), true),
    ];
    let show = |amount: i64, time: bool| {
        if time {
            duration(amount)
        } else {
            money(amount)
        }
    };
    let nearest = caps
        .iter()
        .filter_map(|(spent, cap, time)| {
            cap.map(|cap| (*spent, cap, *time))
        })
        .max_by(|a, b| {
            (a.0 as f64 / a.1.max(1) as f64)
                .total_cmp(&(b.0 as f64 / b.1.max(1) as f64))
        });
    let period_words = match budget.period {
        Period::Day => "today",
        Period::Month => "this month",
    };
    let whose = if payer.is_some() { "Hamlet" } else { "Realm" };
    let summary = format!(
        "{whose} {period_words}: {}",
        nearest
            .map(|(spent, cap, time)| {
                format!(
                    "{} of {}",
                    show(spent, time),
                    show(cap, time)
                )
            })
            .unwrap_or_else(|| money(spent.micros))
    );
    let local = clock::local(at)?;
    let day = f64::from(local.num_seconds_from_midnight()) / 86400.0;
    let gone = match budget.period {
        Period::Day => day,
        Period::Month => {
            let days = days_in_month(local.year(), local.month());
            (f64::from(local.day()) - 1.0 + day) / f64::from(days)
        }
    };
    Ok(protocol::Report {
        caps: protocol::ReportCaps {
            micros: budget.cost,
            ms: budget.minutes.map(clock::minutes),
        },
        gone: (gone * 200.0).floor() / 200.0,
        left: nearest.map(|(spent, cap, _)| {
            (1.0 - spent as f64 / cap.max(1) as f64).max(0.0)
        }),
        lines: Vec::new(),
        period: match budget.period {
            Period::Day => protocol::Period::Day,
            Period::Month => protocol::Period::Month,
        },
        period_words: period_words.into(),
        remains: nearest.map(|(spent, cap, time)| {
            show((cap - spent).max(0), time)
        }),
        spent: protocol::Amount {
            micros: spent.micros,
            ms: spent.ms,
            tokens: spent.tokens,
        },
        summary,
    })
}

/// What `conversation`'s model calls used, as its transcript shows
/// it.
pub(crate) fn usage_view(
    db: &mut Db,
    conversation: hamlet_store::ids::ConversationId,
) -> Result<protocol::UsageView> {
    let calls = attempt::table
        .filter(attempt::conversation.eq(conversation))
        .count()
        .get_result::<i64>(db)?;
    let used = usage::table
        .inner_join(
            attempt::table.on(attempt::runtime
                .eq(usage::runtime)
                .and(attempt::key.eq(usage::key))),
        )
        .filter(attempt::conversation.eq(conversation))
        .order(usage::ordinal)
        .select((
            usage::input,
            usage::output,
            usage::cache_read,
            usage::micros,
            usage::ms,
        ))
        .load::<(i64, i64, i64, i64, i64)>(db)?;
    let total = |pick: fn(&(i64, i64, i64, i64, i64)) -> i64| {
        u64::try_from(used.iter().map(pick).sum::<i64>()).unwrap_or(0)
    };
    let input = total(|u| u.0);
    let output = total(|u| u.1);
    let micros = total(|u| u.3);
    let unreported = u64::try_from(calls)
        .unwrap_or(0)
        .saturating_sub(used.len() as u64);
    Ok(protocol::UsageView {
        cached: total(|u| u.2),
        calls: u64::try_from(calls).unwrap_or(0),
        input,
        last_input: used
            .last()
            .map_or(0, |u| u64::try_from(u.0).unwrap_or(0)),
        micros,
        ms: total(|u| u.4),
        output,
        reasoning: 0,
        summary: format!(
            "{} in, {} out, {}",
            input,
            output,
            money(i64::try_from(micros).unwrap_or(i64::MAX))
        ),
        unreported,
    })
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|date| date.pred_opt())
        .map_or(30, |date| date.day())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_rounds_to_the_cent() {
        assert_eq!(money(0), "$0.00");
        assert_eq!(money(5_000), "<$0.01");
        assert_eq!(money(1_234_999), "$1.23");
        assert_eq!(money(1_235_000), "$1.24");
    }
}
