//! Model and road spend, period resets and task caps (H8, H12).

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use chrono::{Datelike, Local, Timelike};
use hamlet_protocol::client::{
    self as protocol, Period, Report, ReportCaps, UsageView,
};
use hamlet_protocol::runtime::{Price, Usage};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::harness::BindingId;
use crate::service::Service;

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub(crate) struct Amount {
    pub tokens: i64,
    pub micros: i64,
    pub ms: i64,
}

impl Amount {
    fn add(&mut self, other: Self) {
        self.tokens = self.tokens.saturating_add(other.tokens);
        self.micros = self.micros.saturating_add(other.micros);
        self.ms = self.ms.saturating_add(other.ms);
    }
}

pub(crate) struct Books {
    pub hamlet: Amount,
    pub agents: BTreeMap<u64, Amount>,
    pub task: Amount,
}

/// `dollars` in micros, to the nearest; past an `i64`, its largest.
pub(crate) fn micros(dollars: f64) -> i64 {
    (dollars * 1_000_000.0).round() as i64
}

/// `micros` in dollars to the cent, a half cent rounding up.
pub(crate) fn money(micros: impl Into<i128>) -> String {
    let micros = micros.into();
    if (1..10_000).contains(&micros) {
        return "<$0.01".into();
    }
    let cents = (micros + 5_000) / 10_000;
    format!("${}.{:02}", cents / 100, cents % 100)
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

/// Reads a hamlet's current period and a task's complete lifetime.
pub(crate) fn books(
    db: &Connection,
    hamlet: &str,
    period: &Period,
    task: Option<&str>,
    at: i64,
) -> Result<Books> {
    let date = crate::clock::local(at)?;
    let today = date.format("%Y-%m-%d").to_string();
    let first = match period {
        Period::Month => date.format("%Y-%m-01").to_string(),
        Period::Day => today.clone(),
    };
    let reset = db
        .query_row(
            "SELECT models,roads FROM spend_reset WHERE hamlet = ?1 \
             AND date(at/1000, 'unixepoch', 'localtime') BETWEEN ?2 \
             AND ?3 ORDER BY id DESC LIMIT 1",
            params![hamlet, first, today],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?
        .unwrap_or_default();
    let mut result = Books {
        hamlet: Amount::default(),
        agents: BTreeMap::new(),
        task: Amount::default(),
    };
    let mut query = db.prepare(
        "SELECT b.number, a.task, u.at, \
         u.input+u.output+u.cache_read+u.cache_write+u.estimated, \
         u.micros, u.ms, u.ordinal, 0, coalesce(s.payer,b.hamlet) \
         FROM harness_usage u JOIN harness_attempt a \
         USING(runtime,key) JOIN harness_binding b ON \
         b.id=a.binding LEFT JOIN society_binding s ON \
         s.binding=b.id WHERE coalesce(s.payer,b.hamlet)=?1 OR \
         a.task=?2 UNION ALL SELECT \
         agent,task,at,tokens,micros,ms,id,1,hamlet FROM \
         record_road_charge WHERE hamlet=?1 OR task=?2",
    )?;
    let rows = query.query_map(params![hamlet, task], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, i64>(2)?,
            Amount {
                tokens: r.get(3)?,
                micros: r.get(4)?,
                ms: r.get(5)?,
            },
            r.get::<_, i64>(6)?,
            r.get::<_, bool>(7)?,
            r.get::<_, String>(8)?,
        ))
    })?;
    for row in rows {
        let (agent, own_task, time, amount, ordinal, road, payer) =
            row?;
        if task.is_some() && own_task.as_deref() == task {
            result.task.add(amount);
        }
        let day =
            crate::clock::local(time)?.format("%Y-%m-%d").to_string();
        if payer != hamlet
            || day < first
            || day > today
            || ordinal <= if road { reset.1 } else { reset.0 }
        {
            continue;
        }
        result.hamlet.add(amount);
        result
            .agents
            .entry(u64::try_from(agent)?)
            .or_default()
            .add(amount);
    }
    Ok(result)
}

pub(crate) fn can_spend(
    db: &Connection,
    hamlet: &str,
    task: Option<&str>,
    at: i64,
) -> Result<Option<String>> {
    let limits = crate::limits::of(db, hamlet)?;
    let period = &limits.budget.period;
    let books = books(db, hamlet, period, task, at)?;
    for (kind, spent, cost, minutes) in [
        (
            "budget",
            books.hamlet,
            limits.budget.cost,
            limits.budget.minutes,
        ),
        ("task", books.task, limits.task.cost, limits.task.minutes),
    ] {
        for (amount, cap, time) in meters(spent, cost, minutes) {
            let Some(cap) = cap else {
                continue;
            };
            if amount < cap {
                continue;
            }
            let reached = format!(
                "{} of {}",
                show(amount, time),
                show(cap, time)
            );
            return Ok(Some(if kind == "task" {
                format!(
                    "This task has spent its cap ({reached}). The \
                     chief can give the rest of the work as a new \
                     task, or raise the cap under One Task in the \
                     hamlet's limits."
                )
            } else {
                let (when, again) = match period {
                    Period::Day => ("today", "at midnight"),
                    Period::Month => {
                        ("this month", "on the first of next month")
                    }
                };
                format!(
                    "The hamlet has spent its budget for {when} \
                     ({reached}). It starts again {again}. Only the \
                     chief can raise it in the hamlet's limits."
                )
            }));
        }
    }
    Ok(None)
}

/// What `spent` has of the caps `cost` and `minutes`, in micros and
/// milliseconds, each with whether it is time.
fn meters(
    spent: Amount,
    cost: Option<f64>,
    minutes: Option<i64>,
) -> [(i64, Option<i64>, bool); 2] {
    [
        (spent.micros, cost.map(micros), false),
        (spent.ms, minutes.map(crate::clock::minutes), true),
    ]
}
fn show(value: i64, time: bool) -> String {
    if time {
        format!("{} model time", duration(value))
    } else {
        money(value)
    }
}

/// What `usage` costs: what the endpoint billed, else what pi priced
/// it at, else `price` with cached tokens at its input rate.
pub(crate) fn amount(usage: &Usage, price: &Price) -> Amount {
    let reported = usage.input
        + usage.output
        + usage.cache_read
        + usage.cache_write;
    let estimate = if reported == 0 {
        usage.estimated_tokens.unwrap_or(0)
    } else {
        0
    };
    let priced = (usage.input
        + usage.cache_read
        + usage.cache_write
        + estimate) as f64
        * price.input
        + usage.output as f64 * price.output;
    let micros = match usage.billed {
        Some(billed) => micros(billed),
        None if usage.cost > 0.0 => micros(usage.cost),
        None => priced.round() as i64,
    };
    let count = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
    Amount {
        tokens: count(reported + estimate),
        micros,
        ms: count(usage.ms.unwrap_or(0)),
    }
}

impl Service {
    pub(crate) fn budget(&mut self, hamlet: &str) -> Result<Report> {
        self.report(hamlet, None)
    }

    /// The hamlet's spend view, `fallback` naming the model of an
    /// agent that runs the home's default.
    pub(crate) fn report(
        &mut self,
        hamlet: &str,
        fallback: Option<&str>,
    ) -> Result<Report> {
        let home = match fallback {
            Some(fallback) => fallback.to_owned(),
            None => self.settings()?.model,
        };
        let record = self
            .home
            .store()
            .record(hamlet)?
            .context("budget hamlet")?;
        let limits = &record.limits.chief;
        let at = Local::now();
        let period = &limits.budget.period;
        let books = books(
            self.db(),
            hamlet,
            period,
            None,
            at.timestamp_millis(),
        )?;
        // The first of equally near caps.
        let caps = meters(
            books.hamlet,
            limits.budget.cost,
            limits.budget.minutes,
        );
        let nearest = caps
            .into_iter()
            .filter_map(|(spent, cap, time)| {
                cap.map(|cap| (spent, cap, time))
            })
            .rev()
            .max_by(|a, b| {
                (a.0 as f64 / a.1.max(1) as f64)
                    .total_cmp(&(b.0 as f64 / b.1.max(1) as f64))
            });
        let period_words = match period {
            Period::Day => "today",
            Period::Month => "this month",
        };
        let summary = format!(
            "Hamlet {period_words}: {}",
            nearest
                .map(|(spent, cap, time)| format!(
                    "{} of {}",
                    show(spent, time),
                    show(cap, time)
                ))
                .unwrap_or_else(|| money(books.hamlet.micros))
        );
        let mut agents = record.agents.iter().collect::<Vec<_>>();
        agents.sort_by_key(|a| a.number != record.mayor);
        let lines = agents
            .iter()
            .map(|a| {
                format!(
                    "- {}: {}, {}",
                    if a.number == record.mayor {
                        "mayor"
                    } else {
                        &a.name
                    },
                    limits.model_of(
                        a,
                        a.number == record.mayor,
                        &home
                    ),
                    money(
                        books
                            .agents
                            .get(&a.number)
                            .copied()
                            .unwrap_or_default()
                            .micros
                    )
                )
            })
            .collect::<Vec<_>>();
        let day = at.num_seconds_from_midnight() as f64 / 86400.0;
        let next = if at.month() == 12 {
            chrono::NaiveDate::from_ymd_opt(at.year() + 1, 1, 1)
        } else {
            chrono::NaiveDate::from_ymd_opt(
                at.year(),
                at.month() + 1,
                1,
            )
        }
        .context("next month")?;
        let days = next.pred_opt().context("last day")?.day();
        let gone = ((if *period == Period::Day {
            day
        } else {
            (at.day() as f64 - 1.0 + day) / days as f64
        }) * 200.0)
            .floor()
            / 200.0;
        let spent = books.hamlet;
        Ok(Report {
            caps: ReportCaps {
                micros: caps[0].1,
                ms: caps[1].1,
            },
            gone,
            left: nearest.map(|(spent, cap, _)| {
                (1.0 - spent as f64 / cap.max(1) as f64).max(0.0)
            }),
            lines,
            period: period.clone(),
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

    /// Starts the budget period of `hamlet` again from now, keeping
    /// the ledger; gives the words of `/reset-budget`.
    pub fn reset_budget(&mut self, hamlet: &str) -> Result<String> {
        self.db().execute(
            "INSERT INTO spend_reset(hamlet,at,by,models,roads) \
             VALUES (?1,?2,?3,(SELECT COALESCE(MAX(ordinal),0) FROM \
             harness_usage),(SELECT COALESCE(MAX(id),0) FROM \
             record_road_charge))",
            params![hamlet, crate::clock::now(), self.chief],
        )?;
        Ok("The budget starts again from here. What was spent \
            before is kept in the ledger."
            .into())
    }
}

/// The conversation's charged calls, including attempts and
/// summaries.
#[cfg(any(test, feature = "harness"))]
pub fn usage(
    db: &Connection,
    binding: BindingId,
) -> Result<UsageView> {
    usage_of(db, &[binding])
}

/// The model usage of `bindings` together, as of a conversation and
/// the branches it was forked or rewound from.
pub fn usage_of(
    db: &Connection,
    bindings: &[BindingId],
) -> Result<UsageView> {
    let bindings = serde_json::to_string(
        &bindings.iter().map(|b| b.0).collect::<Vec<_>>(),
    )?;
    let mut query = db.prepare(
        "SELECT u.input+u.cache_read+u.cache_write,u.output,u.\
         micros,u.ms,u.cache_read,u.detail FROM harness_usage u \
         JOIN harness_attempt a USING(runtime,key) WHERE a.binding \
         IN (SELECT value FROM json_each(?1)) ORDER BY u.ordinal",
    )?;
    let mut calls = 0;
    let mut unreported = 0;
    let mut input = 0;
    let mut output = 0;
    let mut micros = 0;
    let mut ms = 0;
    let mut cached = 0;
    let mut reasoning = 0;
    let mut last_input = 0;
    for row in query.query_map([bindings], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
        ))
    })? {
        let (i, o, c, t, cache, detail) = row?;
        let detail = serde_json::from_str::<Value>(&detail)?;
        calls += 1;
        unreported += i64::from(i + o == 0);
        input += i;
        output += o;
        micros += c;
        ms += t;
        cached += cache;
        reasoning += detail["reasoning"].as_i64().unwrap_or(0);
        if i > 0 {
            last_input = i;
        }
    }
    let count = input + output;
    let tokens = if calls == unreported {
        "Tokens not reported".into()
    } else if count < 1000 {
        format!("{count} tok")
    } else if count < 1_000_000 {
        format!("{:.1}k tok", count as f64 / 1000.0)
    } else {
        format!("{:.2}M tok", count as f64 / 1_000_000.0)
    };
    let summary = if calls == 0 {
        "No model calls yet".into()
    } else {
        format!(
            "{tokens}, {}",
            if micros > 0 {
                money(micros)
            } else {
                show(ms, true)
            }
        )
    };
    let count = |n: i64| u64::try_from(n).context("a usage count");
    Ok(UsageView {
        cached: count(cached)?,
        calls: count(calls)?,
        input: count(input)?,
        last_input: count(last_input)?,
        micros: count(micros)?,
        ms: count(ms)?,
        output: count(output)?,
        reasoning: count(reasoning)?,
        summary,
        unreported: count(unreported)?,
    })
}
