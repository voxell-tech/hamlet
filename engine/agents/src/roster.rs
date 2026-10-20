//! The agents and the hamlets with their state in words, told as they
//! change, and a hamlet's record.

use std::time::{Duration, Instant};

use bevy::app::{App, PostUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::Has;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{
    Local, Query, Res, ResMut, SystemParam, SystemState,
};
use bevy::ecs::world::World;
use hamlet_core::agents::{
    Account, Activity, Busy, Citizen, Doing, Left, LivesIn, Mayor,
    MergedInto, Paper, Purpose, Settlement, Treasury, Works,
};
use hamlet_core::core::{HomeDir, Settings};
use hamlet_core::oversight::Waiting;
use hamlet_engine_api::Failure;
use hamlet_engine_api::agents::{
    AgentsEvent, ClosedHamlet, Record, Roster, RosterAgent,
    RosterHamlet, Vote,
};
use hamlet_home::limits;
use hamlet_rules::hamlet::HOME;
use hamlet_rules::realm::Realm;
use hamlet_rules::vote::{self, Standing};
use hamlet_rules::work::Mode;

use crate::Chief;
use crate::watch::{Gather, Watchers};

/// The time between two reads of the hamlets closed down.
const CLOSED_EVERY: Duration = Duration::from_secs(2);

/// The folders worked on before that are told.
const BEFORE: usize = 4;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, tell_roster.in_set(Gather));
}

/// The roster's methods on [`World`].
pub(crate) trait RosterWorldExt {
    fn roster(&mut self) -> Roster;

    /// The record of the hamlet `name`.
    fn record(&self, name: &str) -> Result<Record, Failure>;
}

impl RosterWorldExt for World {
    fn roster(&mut self) -> Roster {
        let closed = closed(&self.resource::<HomeDir>().0.root);
        let mut facts = SystemState::<RosterFacts>::new(self);
        facts
            .get(self)
            .map(|facts| facts.roster(closed))
            .unwrap_or_default()
    }

    fn record(&self, name: &str) -> Result<Record, Failure> {
        let place = self
            .try_query::<&Settlement>()
            .and_then(|mut all| {
                all.iter(self).find(|at| at.name == name).cloned()
            })
            .ok_or(Failure::NotFound)?
            .record();
        let model = self.resource::<Settings>().config.model.clone();
        let rules = Realm::open(place.home())
            .map(|realm| realm.rules)
            .unwrap_or_default();
        let user = self.resource::<Chief>().name.clone();
        let record = match place.decree(&user, "/record") {
            Some(Ok(decree)) => decree.lines.join("\n"),
            Some(Err(why)) => format!("{why:#}"),
            None => String::new(),
        };
        Ok(Record {
            budget: place.report(&model).lines(),
            votes: vote::votes(&place)
                .iter()
                .rev()
                .map(|vote| Vote {
                    id: vote.id,
                    report: vote
                        .report(&rules)
                        .trim_end()
                        .to_string(),
                    open: matches!(
                        vote.standing,
                        Standing::Open | Standing::Passed
                    ),
                })
                .collect(),
            record,
            limits: std::fs::read_to_string(limits::path(
                place.root(),
            ))
            .unwrap_or_default(),
        })
    }
}

/// The hamlets closed down under `home`.
fn closed(home: &std::path::Path) -> Vec<ClosedHamlet> {
    Realm::open(home)
        .map(|realm| realm.closed())
        .unwrap_or_default()
        .into_iter()
        .map(|place| ClosedHamlet {
            title: place.title.unwrap_or_else(|| place.name.clone()),
            name: place.name,
        })
        .collect()
}

type Told<'a> = (
    &'a Citizen,
    &'a Name,
    &'a LivesIn,
    Option<&'a Purpose>,
    &'a Activity,
    Option<&'a Account>,
    Option<&'a MergedInto>,
    (Has<Busy>, Has<Mayor>, Has<Left>),
);

/// What the roster is made of.
#[derive(SystemParam)]
pub(crate) struct RosterFacts<'w, 's> {
    agents: Query<'w, 's, Told<'static>>,
    hamlets: Query<
        'w,
        's,
        (
            Entity,
            &'static Settlement,
            &'static Name,
            Option<&'static Treasury>,
            Option<&'static Works>,
        ),
    >,
    waiting: Query<'w, 's, &'static Waiting>,
    names: Query<'w, 's, &'static Name>,
}

impl RosterFacts<'_, '_> {
    fn roster(&self, closed: Vec<ClosedHamlet>) -> Roster {
        let wide = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let id_of = |hamlet: Entity| {
            self.hamlets
                .get(hamlet)
                .map(|(_, settlement, ..)| settlement.name.clone())
                .unwrap_or_default()
        };
        let period = |hamlet: Entity| {
            self.hamlets
                .get(hamlet)
                .ok()
                .and_then(|(.., treasury, _)| treasury)
                .map(|treasury| treasury.0.period.clone())
                .filter(|period| !period.is_empty())
                .unwrap_or_else(|| "today".into())
        };
        let mut agents = self
            .agents
            .iter()
            .map(
                |(
                    citizen,
                    name,
                    lives,
                    purpose,
                    activity,
                    account,
                    into,
                    (busy, mayor, gone),
                )| {
                    let name = name.as_str().to_string();
                    let merged_into = into
                        .and_then(|into| self.names.get(into.0).ok())
                        .map(|name| name.as_str().to_string());
                    let state = match (gone, &merged_into) {
                        (true, Some(host)) => {
                            format!("Merged into {host}")
                        }
                        (true, None) => "Removed".to_string(),
                        _ => now(busy, *activity),
                    };
                    RosterAgent {
                        number: citizen.0,
                        hamlet: id_of(lives.0),
                        mayor,
                        purpose: purpose
                            .map(|purpose| purpose.0.clone())
                            .unwrap_or_default(),
                        state,
                        busy,
                        gone,
                        merged_into,
                        waiting: wide(
                            self.waiting
                                .iter()
                                .filter(|waits| waits.by == name)
                                .count(),
                        ),
                        model: account
                            .map(|account| account.0.model.clone())
                            .unwrap_or_default(),
                        account: account
                            .map(|account| account.0.summary())
                            .unwrap_or_default(),
                        spent: account
                            .map(|account| {
                                format!(
                                    "Spent {}: {}",
                                    period(lives.0),
                                    account.0.cost()
                                )
                            })
                            .unwrap_or_default(),
                        name,
                    }
                },
            )
            .collect::<Vec<_>>();
        agents.sort_by(|a, b| {
            (&a.hamlet, a.gone, !a.mayor, a.number)
                .cmp(&(&b.hamlet, b.gone, !b.mayor, b.number))
        });
        let mut hamlets = self
            .hamlets
            .iter()
            .map(|(_, settlement, title, treasury, works)| {
                let here = agents
                    .iter()
                    .filter(|agent| agent.hamlet == settlement.name)
                    .collect::<Vec<_>>();
                let living = here
                    .iter()
                    .filter(|agent| !agent.gone)
                    .collect::<Vec<_>>();
                let working =
                    living.iter().filter(|agent| agent.busy).count();
                let waiting = living
                    .iter()
                    .map(|agent| agent.waiting)
                    .sum::<u32>();
                let work = works.map(|works| &works.0);
                RosterHamlet {
                    id: settlement.name.clone(),
                    title: title.as_str().to_string(),
                    mayor: living
                        .iter()
                        .find(|agent| agent.mayor)
                        .map(|agent| agent.name.clone())
                        .unwrap_or_default(),
                    living: wide(living.len()),
                    working: wide(working),
                    waiting,
                    gone: wide(here.len() - living.len()),
                    state: standing(living.len(), working, waiting),
                    account: treasury
                        .map(|treasury| treasury.0.summary())
                        .unwrap_or_default(),
                    folder: work
                        .and_then(|work| work.folder.as_ref())
                        .map(|folder| folder.display().to_string()),
                    manual: work
                        .is_none_or(|work| work.mode == Mode::Manual),
                    mode: work
                        .map(|work| work.mode.says().to_string())
                        .unwrap_or_default(),
                    before: work
                        .map(|work| {
                            work.before
                                .iter()
                                .take(BEFORE)
                                .map(|folder| {
                                    folder.display().to_string()
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>();
        // The first hamlet first, then by name.
        hamlets.sort_by_key(|hamlet| {
            (hamlet.id != HOME, hamlet.title.to_lowercase())
        });
        Roster {
            agents,
            hamlets,
            closed,
        }
    }
}

/// The step an agent is on, in words.
fn now(busy: bool, activity: Activity) -> String {
    if !busy {
        return "At rest".into();
    }
    let paper = |paper: Paper| match paper {
        Paper::File => "a file",
        Paper::Notes => "its notes",
        Paper::Skill => "a skill",
    };
    match activity.doing {
        Doing::Unknown => "Working".into(),
        Doing::Think => "Thinking".into(),
        Doing::Read(read) => format!("Reading {}", paper(read)),
        Doing::Write(written) => {
            format!("Writing {}", paper(written))
        }
        Doing::Run => "Running a command".into(),
        Doing::Fetch => "Fetching from the network".into(),
        Doing::CustomTool => "Using a tool added by hand".into(),
        Doing::Await => "Waiting for an agent's answer".into(),
    }
}

/// The state of a hamlet: the agents `living` in it, and those of
/// them `working` or `waiting` for the chief.
fn standing(living: usize, working: usize, waiting: u32) -> String {
    let count = match living {
        1 => "1 agent".to_string(),
        n => format!("{n} agents"),
    };
    match (waiting, working) {
        (0, 0) => format!("{count}, all at rest"),
        (0, n) => format!("{count}, {n} at work"),
        (w, _) => format!("{count}, {w} waiting for you"),
    }
}

/// What the watchers were last told of the roster, and when the
/// hamlets closed down are next read.
#[derive(Default)]
struct Shown {
    roster: Option<Roster>,
    closed: Vec<ClosedHamlet>,
    due: Option<Instant>,
}

fn tell_roster(
    facts: RosterFacts,
    home: Res<HomeDir>,
    mut watchers: ResMut<Watchers>,
    mut shown: Local<Shown>,
) {
    if !watchers.watched() {
        *shown = Shown::default();
        return;
    }
    let at = Instant::now();
    if shown.due.is_none_or(|due| at >= due) {
        shown.due = Some(at + CLOSED_EVERY);
        shown.closed = closed(&home.0.root);
    }
    let roster = facts.roster(shown.closed.clone());
    if shown.roster.as_ref() != Some(&roster) {
        watchers.tell(AgentsEvent::Roster(roster.clone()));
        shown.roster = Some(roster);
    }
}
