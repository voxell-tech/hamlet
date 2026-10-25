//! The hamlets and their agents as rows of the Inspector's lists.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::Has;
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, row,
};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_client::realm::{
    Account, Activity, AgentNumber, Busy, HOME, HamletId, Left,
    LivesIn, Mayor, MergedInto, Purpose, Treasury,
};
use hamlet_client::waits::Waiting;
use hamlet_realm::pick::Thing;

use super::{Naming, Search};
use crate::icons::Glyph;
use crate::looks::{card_button, card_name, note, stated};
use crate::roll::Roll;
use crate::{View, with_cx, world_menu};

/// A check that fires when the agents or the hamlets changed,
/// something began or ended its wait, or the words typed did.
pub(crate) fn stale()
-> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut seen = None::<(Option<Roll>, String, Option<Entity>)>;
    move |world: &World| {
        let roll = world.get_resource::<Roll>().copied();
        let naming = world.get_resource::<Naming>().and_then(|n| n.0);
        let search = world.get_resource::<Search>();
        let typed = search.map_or("", |search| search.0.as_str());
        let same = seen.as_ref().is_some_and(|(r, s, n)| {
            (*r, s.as_str(), *n) == (roll, typed, naming)
        });
        if !same {
            seen = Some((roll, typed.to_string(), naming));
        }
        !same
    }
}

/// A hamlet in the overview.
#[derive(Clone, PartialEq)]
pub(super) struct HamletRow {
    pub hamlet: Entity,
    /// The name it is known by in commands, and the one shown.
    pub id: String,
    pub title: String,
    pub mayor: String,
    pub living: usize,
    pub working: usize,
    pub waiting: usize,
    pub gone: usize,
    pub account: String,
}

/// The state of a hamlet in a word: the agents `living` in it, and
/// those of them `working` or `waiting` for the chief.
pub(crate) fn standing(
    living: usize,
    working: usize,
    waiting: usize,
) -> (Glyph, String, Tone) {
    let count = match living {
        1 => "1 agent".to_string(),
        n => format!("{n} agents"),
    };
    match (waiting, working) {
        (0, 0) => {
            (Glyph::Moon, format!("{count}, all at rest"), Tone::Dim)
        }
        (0, n) => (
            Glyph::Bolt,
            format!("{count}, {n} at work"),
            Tone::Accent,
        ),
        (w, _) => (
            Glyph::Bell,
            format!("{count}, {w} waiting for you"),
            Tone::Critical,
        ),
    }
}

impl HamletRow {
    pub(super) fn state(&self) -> (Glyph, String, Tone) {
        standing(self.living, self.working, self.waiting)
    }
}

/// An agent of a hamlet, living or gone.
#[derive(Clone, PartialEq)]
pub(super) struct AgentRow {
    pub agent: Entity,
    pub number: u64,
    pub name: String,
    pub mayor: bool,
    pub purpose: String,
    /// Its work, or the way it left.
    pub state: String,
    pub busy: bool,
    pub waiting: usize,
    pub gone: bool,
    pub account: String,
}

type Told<'a> = (
    Entity,
    &'a AgentNumber,
    &'a Name,
    &'a LivesIn,
    Option<&'a Purpose>,
    Option<&'a Activity>,
    Option<&'a Account>,
    Option<&'a MergedInto>,
    (Has<Busy>, Has<Mayor>, Has<Left>),
);

/// Everyone of `hamlet`, or of every hamlet: the mayor, the living
/// by their numbers, then those that are gone.
pub(super) fn agent_rows(
    world: &World,
    hamlet: Option<Entity>,
) -> Vec<AgentRow> {
    let Some(mut agents) = world.try_query::<Told>() else {
        return Vec::new();
    };
    let waits = world
        .try_query::<&Waiting>()
        .map(|mut waits| {
            waits
                .iter(world)
                .map(|w| w.by.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let named =
        |agent: Entity| hamlet_client::realm::name_of(world, agent);
    let mut rows = agents
        .iter(world)
        .filter(|told| hamlet.is_none_or(|h| told.3.0 == h))
        .map(
            |(
                agent,
                number,
                name,
                _,
                purpose,
                activity,
                account,
                into,
                (busy, mayor, gone),
            )| {
                let name = name.as_str().to_string();
                let state = match (gone, into) {
                    (true, Some(host)) => {
                        format!("Merged into {}", named(host.0))
                    }
                    (true, None) => "Removed".to_string(),
                    _ => activity.cloned().unwrap_or_default().words,
                };
                AgentRow {
                    agent,
                    number: number.0,
                    waiting: waits
                        .iter()
                        .filter(|by| **by == name)
                        .count(),
                    name,
                    mayor,
                    purpose: purpose
                        .map(|p| {
                            crate::hover::short(
                                &p.0,
                                crate::hover::PURPOSE_SHOWN,
                            )
                        })
                        .unwrap_or_default(),
                    state,
                    busy,
                    gone,
                    account: account
                        .map(Account::summary)
                        .unwrap_or_default(),
                }
            },
        )
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| (row.gone, !row.mayor, row.number));
    rows
}

pub(super) fn hamlet_rows(world: &World) -> Vec<HamletRow> {
    let Some(mut hamlets) = world.try_query::<(
        Entity,
        &HamletId,
        &Name,
        Option<&Treasury>,
    )>() else {
        return Vec::new();
    };
    let everyone = agent_rows(world, None);
    let lives =
        |agent: Entity| world.get::<LivesIn>(agent).map(|l| l.0);
    let mut rows = hamlets
        .iter(world)
        .map(|(hamlet, id, name, treasury)| {
            let here = everyone
                .iter()
                .filter(|row| lives(row.agent) == Some(hamlet))
                .collect::<Vec<_>>();
            let living = here
                .iter()
                .filter(|row| !row.gone)
                .collect::<Vec<_>>();
            HamletRow {
                hamlet,
                id: id.0.clone(),
                title: name.as_str().to_string(),
                mayor: living
                    .iter()
                    .find(|row| row.mayor)
                    .map(|row| row.name.clone())
                    .unwrap_or_default(),
                living: living.len(),
                working: living.iter().filter(|row| row.busy).count(),
                waiting: living.iter().map(|row| row.waiting).sum(),
                gone: here.len() - living.len(),
                account: treasury
                    .map(|t| t.0.summary.clone())
                    .unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    // The first hamlet first, then by name.
    rows.sort_by_key(|row| {
        (row.id != HOME, row.title.to_lowercase())
    });
    rows
}

/// A row that can be pressed: a title with a word of status beside
/// it, over dim lines. The right button opens the menu of `thing`.
pub(super) fn pressable(
    thing: Thing,
    title: String,
    beside: (Glyph, String, Tone),
    lines: Vec<String>,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    let lines = lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .map(|line| note(line).boxed())
        .collect::<Vec<_>>();
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        button(
            column((
                row((
                    card_name(title, Tone::Body),
                    stated(beside.0, beside.1, beside.2),
                ))
                .gap(gap)
                .align(AlignItems::Center),
                column(lines).gap(0.0),
            ))
            .gap(gap / 2.0)
            .align(AlignItems::FlexStart),
        )
        .rules(card_button)
        .tagged(world_menu::Asks(thing))
        .on_activate(act)
    })
}
