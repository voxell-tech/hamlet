//! The ledger of what was spent: `spend/<date>.jsonl`, one file a day
//! and one line for each model call or road.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{Local, NaiveDate, TimeZone};
use hamlet_home::limits::Period;
use hamlet_model::log;
use serde_json::{Value, json};

use super::{Amount, first_day};
use crate::record;

/// A new task's id. It opens with the time, which says where in the
/// ledger the task's spend begins.
pub fn new_task(agent: &str) -> String {
    format!("{}-{agent}", log::now_millis())
}

/// The day a task began.
fn task_day(task: &str) -> Option<NaiveDate> {
    let ms = task.split('-').next()?.parse::<i64>().ok()?;
    Some(Local.timestamp_millis_opt(ms).single()?.date_naive())
}

/// The folder a hamlet's charges are kept in.
pub struct Ledger {
    dir: PathBuf,
}

/// One line of the ledger.
pub struct Charge<'a> {
    pub agent: &'a str,
    pub task: &'a str,
    /// "model" or "road".
    pub what: &'a str,
    pub model: &'a str,
    pub amount: Amount,
    /// The tokens were counted from the text, not reported.
    pub estimated: bool,
}

impl Ledger {
    pub fn new(home: &Path) -> Self {
        Ledger {
            dir: home.join("spend"),
        }
    }

    /// The file `day`'s charges are kept in.
    pub fn file(&self, day: NaiveDate) -> PathBuf {
        self.dir.join(format!("{day}.jsonl"))
    }

    pub fn charge(&self, day: NaiveDate, c: &Charge) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let line = json!({
            "v": 1,
            "ts": log::now_millis(),
            "agent": c.agent,
            "task": c.task,
            "what": c.what,
            "model": c.model,
            "tokens": c.amount.tokens,
            "micros": c.amount.micros,
            "ms": c.amount.ms,
            "estimated": c.estimated,
        });
        record::append(&self.file(day), &line)
    }

    /// Notes on `day` that the chief `by` started the budget again.
    /// What was spent before stays in the ledger and is no longer
    /// counted against the period.
    pub fn reset(&self, day: NaiveDate, by: &str) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let line = json!({
            "v": 1,
            "ts": log::now_millis(),
            "what": "reset",
            "by": by,
        });
        record::append(&self.file(day), &line)
    }

    /// Every line from `first` on, with its day.
    pub fn since(&self, first: NaiveDate) -> Vec<(NaiveDate, Value)> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return vec![];
        };
        let mut days = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let day = path
                    .file_name()?
                    .to_str()?
                    .strip_suffix(".jsonl")?
                    .parse::<NaiveDate>()
                    .ok()?;
                (day >= first).then_some((day, path))
            })
            .collect::<Vec<_>>();
        days.sort();
        days.into_iter()
            .flat_map(|(day, path)| {
                record::read(&path)
                    .into_iter()
                    .map(move |line| (day, line))
            })
            .collect()
    }
}

/// The spend of a period, and of one task.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Books {
    pub hamlet: Amount,
    pub agents: BTreeMap<String, Amount>,
    pub task: Amount,
}

impl Books {
    /// Reads the ledger for the period `today` is in and for `task`.
    pub fn read(
        ledger: &Ledger,
        period: Period,
        today: NaiveDate,
        task: &str,
    ) -> Books {
        let first = first_day(period, today);
        let from = task_day(task).map_or(first, |d| d.min(first));
        let mut books = Books::default();
        for (day, line) in ledger.since(from) {
            // The chief started the budget again here. A task keeps
            // what it has spent: its cap is its own.
            if line["what"] == "reset" {
                if day >= first {
                    books.hamlet = Amount::default();
                    books.agents.clear();
                }
                continue;
            }
            let amount = Amount::of_line(&line);
            if day >= first {
                books.hamlet.add(amount);
                books
                    .agents
                    .entry(
                        line["agent"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                    )
                    .or_default()
                    .add(amount);
            }
            if !task.is_empty() && line["task"] == task {
                books.task.add(amount);
            }
        }
        books
    }

    pub fn of(&self, agent: &str) -> Amount {
        self.agents.get(agent).copied().unwrap_or_default()
    }
}

#[cfg(test)]
pub(super) mod tests {
    use chrono::Local;
    use hamlet_testing::Scratch;

    use super::*;

    pub(crate) fn ledger(tag: &str) -> (Scratch, Ledger) {
        let dir = Scratch::new(&format!("ledger-{tag}"));
        let ledger = Ledger::new(&dir);
        (dir, ledger)
    }

    pub(crate) fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    /// Charges `agent` as many tokens, and a cent for each.
    pub(crate) fn charge(
        l: &Ledger,
        on: NaiveDate,
        agent: &str,
        tokens: u64,
    ) {
        l.charge(
            on,
            &Charge {
                agent,
                task: "t",
                what: "model",
                model: "m",
                amount: Amount {
                    tokens,
                    micros: tokens * 10_000,
                    ms: 1_000,
                },
                estimated: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn a_new_day_starts_the_budget_again_and_a_month_does_not() {
        let (_dir, l) = ledger("period");
        charge(&l, day(3), "scout", 500);
        charge(&l, day(4), "scout", 40);
        charge(&l, day(4), "mayor", 2);
        let today = Books::read(&l, Period::Day, day(4), "");
        assert_eq!(today.hamlet.tokens, 42);
        assert_eq!(today.of("scout").tokens, 40);
        assert_eq!(today.of("nobody"), Amount::default());
        let month = Books::read(&l, Period::Month, day(4), "");
        assert_eq!(month.hamlet.tokens, 542);
        let next = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let later = Books::read(&l, Period::Month, next, "");
        assert_eq!(later.hamlet, Amount::default());
        // The chief starts it again: what was spent no longer
        // counts, what is spent after does, and a task keeps its own.
        l.reset(day(4), "ada").unwrap();
        charge(&l, day(4), "scout", 7);
        let again = Books::read(&l, Period::Month, day(4), "t");
        assert_eq!(again.hamlet.tokens, 7);
        assert_eq!(again.of("mayor"), Amount::default());
        assert_eq!(again.task.tokens, 549);
    }

    #[test]
    fn a_task_keeps_its_spend_across_midnight() {
        let (_dir, l) = ledger("task");
        let noon = Local
            .from_local_datetime(
                &day(3).and_hms_opt(12, 0, 0).unwrap(),
            )
            .single()
            .unwrap();
        let task = format!("{}-mayor", noon.timestamp_millis());
        for (on, tokens) in [(day(3), 500), (day(4), 40)] {
            l.charge(
                on,
                &Charge {
                    agent: "scout",
                    task: &task,
                    what: "model",
                    model: "m",
                    amount: Amount {
                        tokens,
                        micros: 0,
                        ms: 0,
                    },
                    estimated: false,
                },
            )
            .unwrap();
        }
        let books = Books::read(&l, Period::Day, day(4), &task);
        assert_eq!(
            (books.hamlet.tokens, books.task.tokens),
            (40, 540)
        );
    }

    #[test]
    fn agents_spending_at_the_same_moment_lose_no_charge() {
        let (_dir, l) = ledger("together");
        std::thread::scope(|scope| {
            for agent in ["a", "b", "c", "d", "e", "f", "g", "h"] {
                let l = &l;
                scope.spawn(move || {
                    for _ in 0..50 {
                        charge(l, day(4), agent, 3);
                    }
                });
            }
        });
        let books = Books::read(&l, Period::Day, day(4), "t");
        assert_eq!(books.hamlet.tokens, 8 * 50 * 3);
        assert_eq!(books.task.tokens, 8 * 50 * 3);
        assert_eq!(books.of("e").tokens, 150);
    }
}
