//! A living hamlet as a client is shown it: its place, its budget's
//! report and its roads.

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::realm::budget::{duration, money};
use hamlet_messages::realm::world::{HamletSeen, Seen, Status};
use hamlet_messages::realm::{HamletId, Tier};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Mode;
use hamlet_protocol::client as protocol;

use super::{count, wire};
use crate::realm::change::{Rows, number};
use crate::realm::chiefs::{self, limits};

/// A living hamlet, its budget's spending and its roads with when
/// each last carried a letter.
#[derive(Clone)]
pub(super) struct Place {
    hamlet: protocol::Hamlet,
    spending: Spending,
    roads: Vec<(protocol::Road, Option<i64>)>,
}

/// What was spent in a period, until its end.
#[derive(Clone, Copy, Default)]
pub(super) struct Spending {
    pub micros: u64,
    pub ms: u64,
    pub tokens: u64,
    pub cap_micros: Option<u64>,
    pub cap_ms: Option<u64>,
    pub period: Option<Period>,
}

/// A budget's period, its bounds in ms.
#[derive(Clone, Copy)]
pub(super) struct Period {
    pub of: limits::Period,
    pub began: i64,
    pub ends: i64,
}

pub(super) async fn place(
    rows: &Rows<'_>,
    seen: &Seen,
    hamlet: &HamletSeen,
    spending: Spending,
) -> Result<Place> {
    let id = &hamlet.hamlet;
    let key = || vec![BindParam::Text(id.0.clone())];
    let kept = chiefs::hamlet(rows, id).await?;
    let (x, z) = rows
        .row::<(Option<f64>, Option<f64>)>(
            "SELECT x, z FROM hamlet WHERE id = ?1",
            key(),
        )
        .await?
        .unwrap_or_default();
    let model = rows
        .value::<Option<String>>(
            "SELECT model FROM model_choice WHERE of = 'hamlet/' || \
             ?1",
            key(),
        )
        .await?
        .flatten();
    let folder = kept
        .as_ref()
        .and_then(|kept| kept.folder.as_deref())
        .map(hamlet_os::fs::path::text);
    let mut before = Vec::new();
    for was in rows
        .values::<Option<String>>(
            "SELECT folder FROM hamlet_folder WHERE hamlet = ?1 \
             ORDER BY at",
            key(),
        )
        .await?
        .into_iter()
        .flatten()
    {
        if Some(&was) != folder.as_ref() && !before.contains(&was) {
            before.push(was);
        }
    }
    let chiefs = rows
        .values::<String>(
            "SELECT chief.name FROM hamlet_chief JOIN chief ON \
             chief.id = hamlet_chief.chief WHERE \
             hamlet_chief.hamlet = ?1 ORDER BY chief.id",
            key(),
        )
        .await?;
    let mode = kept.as_ref().map_or(Mode::Manual, |kept| kept.mode);
    let limits = kept.map(|kept| kept.limits).unwrap_or_default();
    let mode_words = match (mode, folder.is_some()) {
        (_, false) => {
            "Hires work in their lodges; no folder is chosen."
        }
        (Mode::Manual, true) => {
            "Hires ask you before each edit and command in the \
             folder."
        }
        (Mode::Auto, true) => {
            "Hires edit the folder and run commands without asking."
        }
    };
    Ok(Place {
        hamlet: protocol::Hamlet {
            at: match (x, z) {
                (Some(x), Some(z)) => Some(protocol::TownAt { x, z }),
                _ => None,
            },
            before,
            budget: report(spending, 0),
            chiefs,
            folder,
            id: id.0.clone(),
            limits: limits.shown(model),
            mayor: wire(hamlet.mayor),
            mode: match mode {
                Mode::Manual => protocol::HamletMode::Manual,
                Mode::Auto => protocol::HamletMode::Auto,
            },
            mode_words: mode_words.to_owned(),
            name: hamlet.name.clone(),
            roads: Vec::new(),
            tier: match hamlet.tier {
                Tier::Fifty => 50,
                Tier::Hundred => 100,
                Tier::TwoHundred => 200,
                Tier::FiveHundred => 500,
            },
        },
        spending,
        roads: roads(rows, seen, id).await?,
    })
}

impl Place {
    pub(super) fn at(&self, now: i64) -> protocol::Hamlet {
        let grows = self
            .hamlet
            .limits
            .road
            .grows_over_days
            .saturating_mul(DAY);
        protocol::Hamlet {
            budget: report(self.spending, now),
            roads: self
                .roads
                .iter()
                .map(|(road, last)| protocol::Road {
                    level: level(road.messages, *last, grows, now),
                    ..road.clone()
                })
                .collect(),
            ..self.hamlet.clone()
        }
    }
}

const DAY: i64 = 86_400_000;

/// How worn a road is: by how many letters it carried, grown over
/// once unused for `grows` ms (H3).
fn level(
    messages: u64,
    last: Option<i64>,
    grows: i64,
    now: i64,
) -> protocol::RoadLevel {
    let grown = last.is_none_or(|last| now - last > grows);
    match messages {
        0 => protocol::RoadLevel::Dirt,
        _ if grown => protocol::RoadLevel::Overgrown,
        1..=4 => protocol::RoadLevel::Dirt,
        5..=24 => protocol::RoadLevel::Paved,
        _ => protocol::RoadLevel::Rail,
    }
}

/// The roads of the living hires of `hamlet`, each pair once: a
/// hire's to its lead or a mayor's to the Hand, a mayor's to each
/// other mayor, and those laid or agreed, with the letters along
/// each.
async fn roads(
    rows: &Rows<'_>,
    seen: &Seen,
    hamlet: &HamletId,
) -> Result<Vec<(protocol::Road, Option<i64>)>> {
    let mayors = seen
        .hamlets
        .iter()
        .map(|place| place.mayor)
        .collect::<BTreeSet<_>>();
    let laid = rows
        .rows::<(u32, u32, String)>(
            "SELECT a, b, kind FROM road WHERE up_at IS NULL ORDER \
             BY a, b",
            Vec::new(),
        )
        .await?;
    let mut ways = Vec::new();
    for hire in seen.agents.iter().filter(|agent| {
        agent.status == Status::Live
            && agent.worker.is_none()
            && agent.hamlet.as_ref() == Some(hamlet)
    }) {
        let one = hire.agent;
        ways.push((
            one,
            hire.lead.unwrap_or(Hand::NUMBER),
            protocol::RoadKind::Tree,
        ));
        if mayors.contains(&one) {
            for other in mayors.iter().filter(|other| **other != one)
            {
                ways.push((one, *other, protocol::RoadKind::Mayors));
            }
        }
        for (a, b, kind) in &laid {
            let other = match (AgentNumber(*a), AgentNumber(*b)) {
                (a, b) if a == one => b,
                (a, b) if b == one => a,
                _ => continue,
            };
            let kind = if kind == "agreed" {
                protocol::RoadKind::Agreed
            } else {
                protocol::RoadKind::Laid
            };
            ways.push((one, other, kind));
        }
    }
    let mut paired = BTreeSet::new();
    let mut roads = Vec::new();
    for (one, other, kind) in ways {
        let pair = (one.min(other), one.max(other));
        if !paired.insert(pair) {
            continue;
        }
        let used = rows
            .rows::<(u32, i64)>(
                "SELECT sender, at FROM letter WHERE (sender = ?1 \
                 AND receiver = ?2) OR (sender = ?2 AND receiver = \
                 ?1) ORDER BY at DESC, id DESC",
                vec![number(pair.0), number(pair.1)],
            )
            .await?;
        roads.push((
            protocol::Road {
                a: wire(pair.0),
                b: wire(pair.1),
                kind,
                last_from: used
                    .first()
                    .map(|(sender, _)| wire(AgentNumber(*sender))),
                level: protocol::RoadLevel::Dirt,
                messages: u64::try_from(used.len())
                    .unwrap_or(u64::MAX),
            },
            used.first().map(|(_, at)| *at),
        ));
    }
    Ok(roads)
}

fn report(spending: Spending, now: i64) -> protocol::Report {
    let spending = spending.at(now);
    let show = |amount: u64, time: bool| {
        if time {
            duration(amount)
        } else {
            money(amount)
        }
    };
    let nearest = [
        (spending.micros, spending.cap_micros, false),
        (spending.ms, spending.cap_ms, true),
    ]
    .into_iter()
    .filter_map(|(spent, cap, time)| {
        cap.map(|cap| (spent, cap, time))
    })
    .max_by(|a, b| fraction(a.0, a.1).total_cmp(&fraction(b.0, b.1)));
    let period =
        spending.period.map_or(limits::Period::Day, |p| p.of);
    let summary = format!(
        "Hamlet {}: {}",
        period.words(),
        nearest.map_or_else(
            || money(spending.micros),
            |(spent, cap, time)| {
                format!(
                    "{} of {}",
                    show(spent, time),
                    show(cap, time)
                )
            }
        )
    );
    let gone = spending.period.map_or(0.0, |period| {
        fraction(
            u64::try_from(now - period.began).unwrap_or(0),
            u64::try_from(period.ends - period.began).unwrap_or(1),
        )
        .min(1.0)
    });
    protocol::Report {
        caps: protocol::ReportCaps {
            micros: spending.cap_micros.map(count),
            ms: spending.cap_ms.map(count),
        },
        gone: (gone * 200.0).floor() / 200.0,
        left: nearest.map(|(spent, cap, _)| {
            (1.0 - fraction(spent, cap)).max(0.0)
        }),
        lines: Vec::new(),
        period: period.shown(),
        period_words: period.words().to_owned(),
        remains: nearest.map(|(spent, cap, time)| {
            show(cap.saturating_sub(spent), time)
        }),
        spent: protocol::Amount {
            micros: count(spending.micros),
            ms: count(spending.ms),
            tokens: count(spending.tokens),
        },
        summary,
    }
}

impl Spending {
    /// What was spent as of `now`: nothing once its period ended.
    pub(super) fn at(self, now: i64) -> Self {
        match self.period {
            Some(period) if now >= period.ends => Self {
                micros: 0,
                ms: 0,
                tokens: 0,
                ..self
            },
            _ => self,
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a share of a budget is shown as a float"
)]
fn fraction(part: u64, whole: u64) -> f64 {
    part as f64 / whole.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_road_unused_past_its_days_is_overgrown() {
        let week = 7 * DAY;
        assert_eq!(
            level(6, Some(0), week, 8 * DAY),
            protocol::RoadLevel::Overgrown
        );
        assert_eq!(
            level(6, Some(0), week, DAY),
            protocol::RoadLevel::Paved
        );
        assert_eq!(
            level(0, None, week, DAY),
            protocol::RoadLevel::Dirt
        );
    }
}
