//! The hamlets and their agents as rows of the Hub's lists.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::Has;
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::ScopedExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, row,
};
use hamlet_client::realm::{
    Account, Activity, AgentNumber, HamletId, Identity, Left,
    LivesIn, Mayor, MergedInto, Parent, Purpose, Treasury,
};
use hamlet_protocol::client::AgentId;
use hamlet_realm::pick::Thing;
use hamlet_ui::styles::{card_button, card_name, status_label};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use super::Naming;
use crate::roll::Roll;
use crate::standing::{Standing, need_you};
use crate::world_menu;

/// A check that fires when the agents or the hamlets changed,
/// something began or ended its wait, or a hamlet's naming did.
pub(crate) fn stale()
-> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut seen = None::<(Option<Roll>, Option<Entity>)>;
    move |world: &World| {
        let roll = world.get_resource::<Roll>().copied();
        let naming = world.get_resource::<Naming>().and_then(|n| n.0);
        let now = (roll, naming);
        let same = seen == Some(now);
        seen = Some(now);
        !same
    }
}

/// A hamlet in the overview.
#[derive(Clone, PartialEq)]
pub(super) struct HamletRow {
    pub hamlet: Entity,
    /// The id commands name it by.
    pub id: String,
    pub title: String,
    pub account: String,
    /// How much of its budget is spent, from 0 to 1, when it has
    /// one.
    pub spent: Option<f32>,
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
    let (state, words) = match (waiting, working) {
        (0, 0) => (Standing::Rests, format!("{count}, all at rest")),
        (0, n) => (Standing::Works, format!("{count}, {n} at work")),
        (w, _) => {
            let words = format!("{count}, {w} {}", need_you(w));
            (Standing::Waits, words)
        }
    };
    (state.glyph(), words, state.tone())
}

/// An agent of a hamlet, living or gone.
#[derive(Clone, PartialEq)]
pub(super) struct AgentRow {
    pub agent: Entity,
    pub number: u64,
    /// What it is shown by.
    pub name: String,
    pub mayor: bool,
    pub purpose: String,
    /// Its work, or the way it left.
    pub doing: String,
    pub standing: Standing,
    pub gone: bool,
    pub account: String,
    /// How many leads there are between it and the top of its
    /// hamlet's tree.
    pub depth: usize,
}

type Told<'a> = (
    Entity,
    &'a AgentNumber,
    &'a LivesIn,
    Option<&'a Purpose>,
    Option<&'a Activity>,
    Option<&'a Account>,
    Option<&'a MergedInto>,
    (Has<Mayor>, Has<Left>),
    &'a Identity,
    Option<&'a Parent>,
);

/// `rows` in the order of their tree: each lead followed by its
/// hires, the living before those that are gone.
fn in_tree(
    rows: Vec<AgentRow>,
    parents: &[(AgentId, Option<AgentId>)],
    ids: &[(Entity, AgentId)],
) -> Vec<AgentRow> {
    let id_of = |row: &AgentRow| {
        ids.iter()
            .find(|(entity, _)| *entity == row.agent)
            .map(|(_, id)| *id)
    };
    let parent_of = |id: AgentId| {
        parents
            .iter()
            .find(|(of, _)| *of == id)
            .and_then(|(_, parent)| *parent)
    };
    let here =
        |id: AgentId| rows.iter().any(|row| id_of(row) == Some(id));
    let mut ordered = Vec::new();
    let mut pending = rows
        .iter()
        .filter(|row| {
            id_of(row)
                .and_then(parent_of)
                .is_none_or(|parent| !here(parent))
        })
        .cloned()
        .map(|row| (row, 0))
        .collect::<Vec<_>>();
    pending.reverse();
    while let Some((mut row, depth)) = pending.pop() {
        let id = id_of(&row);
        row.depth = depth;
        ordered.push(row);
        let mut below = rows
            .iter()
            .filter(|other| {
                id.is_some()
                    && id_of(other).and_then(parent_of) == id
                    && !ordered
                        .iter()
                        .any(|done| done.agent == other.agent)
            })
            .cloned()
            .map(|row| (row, depth + 1))
            .collect::<Vec<_>>();
        below.reverse();
        pending.extend(below);
    }
    // Whatever no lead reaches goes last.
    for row in rows {
        if !ordered.iter().any(|done| done.agent == row.agent) {
            ordered.push(row);
        }
    }
    ordered
}

/// Everyone of `hamlet`, or of every hamlet: the mayor, the living
/// by their numbers, then those that are gone.
pub(super) fn agent_rows(
    world: &World,
    hamlet: Option<Entity>,
) -> Vec<AgentRow> {
    let Some(mut agents) = world.try_query::<Told>() else {
        return Vec::new();
    };
    let named =
        |agent: Entity| hamlet_client::realm::shown_of(world, agent);
    let mut ids = Vec::new();
    let mut parents = Vec::new();
    let mut rows = agents
        .iter(world)
        .filter(|told| hamlet.is_none_or(|h| told.2.0 == h))
        .map(
            |(
                agent,
                number,
                _,
                purpose,
                activity,
                account,
                into,
                (mayor, gone),
                id,
                parent,
            )| {
                ids.push((agent, id.0));
                parents.push((id.0, parent.map(|parent| parent.0)));
                let standing = Standing::of(world, agent);
                let doing = match (gone, into) {
                    (true, Some(host)) => {
                        format!("Merged into {}", named(host.0))
                    }
                    (true, None) => "Removed".to_string(),
                    // Its standing says the rest.
                    _ if standing != Standing::Works => String::new(),
                    _ => activity.cloned().unwrap_or_default().words,
                };
                AgentRow {
                    agent,
                    number: number.0,
                    name: named(agent),
                    mayor,
                    purpose: purpose
                        .map(|p| {
                            crate::hover::short(
                                &p.0,
                                crate::hover::PURPOSE_SHOWN,
                            )
                        })
                        .unwrap_or_default(),
                    doing,
                    standing,
                    gone,
                    account: account
                        .map(crate::models::account_words)
                        .unwrap_or_default(),
                    depth: 0,
                }
            },
        )
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| (row.gone, !row.mayor, row.number));
    let (gone, living) = rows.into_iter().partition(|row| row.gone);
    let mut rows = in_tree(living, &parents, &ids);
    rows.extend(in_tree(gone, &parents, &ids));
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
    let mut rows = hamlets
        .iter(world)
        .map(|(hamlet, id, name, treasury)| HamletRow {
            hamlet,
            id: id.0.clone(),
            title: name.as_str().to_string(),
            account: treasury
                .map(|t| t.0.summary.clone())
                .unwrap_or_default(),
            spent: treasury.and_then(|t| {
                t.0.caps.micros.filter(|cap| *cap > 0).map(|cap| {
                    (t.0.spent.micros as f32 / cap as f32)
                        .clamp(0.0, 1.0)
                })
            }),
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.title.to_lowercase());
    rows
}

/// A row that can be pressed: a title with a word of status beside
/// it, over `lines`. The right button opens the menu of `thing`.
pub(super) fn pressable(
    thing: Thing,
    title: String,
    beside: (Glyph, String, Tone),
    lines: Vec<AnyView>,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        button(
            column((
                row((
                    card_name(title, Tone::Body),
                    status_label(beside.0, beside.1, beside.2),
                ))
                .gap(gap)
                .align(AlignItems::Center),
                column(lines).gap(0.0),
            ))
            .gap(gap / 2.0)
            .align(AlignItems::FlexStart),
        )
        .rules(card_button)
        .tagged(Variant::Default)
        .tagged(world_menu::Asks(thing))
        .on_activate(act)
    })
}
