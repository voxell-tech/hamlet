//! The Hub's rail: the realm as its chain of command.

use std::collections::{BTreeSet, HashMap, HashSet};

use bevy::app::{App, PostUpdate, Update};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Has, Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, ComputedNode, Display, JustifyContent, Overflow,
    PositionType, ScrollPosition, UiGlobalTransform, UiRect,
    UiSystems, Val, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::tokens::{MotionTokens, SurfaceTokens, Tone};
use bevy_fynix::views::{
    self, Axis, BehaviorExt, FrameProps, TooltipExt, button, column,
    frame, label, row, text_field,
};
use bevy_fynix::{
    Binding, Dragging, FynixSystems, OverrideCursor, ScopedExt,
    Signal, ViewExt, component, derived, each, every_frame, keyed,
    resource, style,
};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::realm::{
    Activity, Busy, HamletId, Identity, Kind, Lead, Left, Lived,
    LivesIn, MergedInto, Parent, Profile, Purpose, Seen, Shown,
    Skills, name_of, shown_of,
};
use hamlet_client::waits::Waiting;
use hamlet_core::age::Age;
use hamlet_core::clock::now_ms;
use hamlet_core::pick::{BringIntoView, Selected, Thing};
use hamlet_core::standing::{Standing, need_you};
use hamlet_core::words;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    CHEVRON, ICON, STATUS_PAD, STEP_IN, note, scroll_area,
    status_icon_at, text_input, transparent,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::{Edge, Variant, Wash};
use hamlet_ui::widgets::ellipsis::{cut, one_line};
use hamlet_ui::widgets::hint::search;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::widgets::tabs::{self, Spread};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::{RailState, Showing, look_at, show};
use crate::convo::{Convo, seat, talk_to_hand};
use crate::profile_tag::{self, ProfileTag};
use crate::world_menu::Asks;

/// The width of the rail at first, the least and the most.
const WIDE: f32 = 248.0;
const NARROWEST: f32 = 200.0;
const WIDEST: f32 = 520.0;
/// The least the rail leaves the page beside it.
const PAGE: f32 = 280.0;

/// The most that left a rail shows.
const LEFT_SHOWN: usize = 6;

/// The most letters of the words a row's tooltip quotes.
const LINE_SHOWN: usize = 90;

/// The longest name a row's tooltip leaves out.
const NAME_IN_TIP: usize = 24;

/// The least height of a row.
const ROW: f32 = 28.0;

/// The height of a worker's row and the side of its icon.
const WORKER_ROW: f32 = 24.0;
const WORKER_ICON: f32 = 12.0;

/// The size of the time since a row last moved, at its end.
const AGE_TEXT: f32 = 12.0;

/// How opaque the icon of one that left is.
const DIMMED: f32 = 0.6;

/// The deepest a row is set in; deeper rows line up with it.
const DEEPEST: u8 = 6;

/// The room between an elbow's end and the row it leads to.
const GAP: f32 = 4.0;

/// The room inside a row's ends, and over and under its words.
const ROW_INSET: f32 = 8.0;
const ROW_DOWN: f32 = 6.0;

/// The room over a section's header inside its block.
const BLOCK_PAD: f32 = 4.0;

/// The space over each part of the rail.
const PART_GAP: f32 = 16.0;

/// The space over the first part under the search row.
const UNDER_SEARCH: f32 = 12.0;

/// The frames a row asked to be brought into view is waited for.
const REVEAL_FRAMES: u8 = 10;

/// The width of the collapsed rail.
const STRIP: f32 = 32.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Minute>()
        .init_resource::<Stale>()
        .add_observer(grab)
        .add_observer(drag)
        .add_observer(drop)
        .add_systems(
            Update,
            (read_search, tabs::mark_on::<Tab>, tick),
        )
        .add_systems(
            Update,
            (note_stale, gather.run_if(Stale::is_any))
                .chain()
                .after(read_search)
                .before(FynixSystems::Mount),
        )
        .add_systems(PostUpdate, reveal.after(UiSystems::Layout));
}

/// Gives `hub` its rail.
pub(super) fn equip(
    world: &mut World,
    hub: Entity,
    state: RailState,
) {
    let mut hub = world.entity_mut(hub);
    hub.insert((
        Rail::default(),
        Width::default(),
        Tab::default(),
        Reveal::default(),
        Parts::default(),
    ));
    if state == RailState::Collapsed {
        hub.insert(Collapsed);
    }
}

/// The panel ids of the Hubs whose rails are collapsed.
pub(super) fn collapsed(world: &mut World) -> BTreeSet<String> {
    world
        .query_filtered::<&super::Hub, With<Collapsed>>()
        .iter(world)
        .map(|hub| hub.0.clone())
        .collect()
}

/// Turns `hub`'s rail to `tab`.
pub(super) fn turn(world: &mut World, hub: Entity, tab: Tab) {
    if let Some(mut now) = world.get_mut::<Tab>(hub)
        && *now != tab
    {
        *now = tab;
    }
}

/// The minute since 1970 the rows' ages are read at.
#[derive(Resource, Default, PartialEq)]
struct Minute(u64);

fn tick(mut minute: ResMut<Minute>) {
    minute.set_if_neq(Minute(now_ms() / 60_000));
}

/// The row to bring into view and the frames left to wait for it.
#[derive(Component, Default)]
struct Reveal(Option<(Target, u8)>);

/// On a Hub whose rail is collapsed to a strip.
#[derive(Component)]
struct Collapsed;

/// What a row or header of a Hub's rail leads to.
#[derive(Component, Clone, Copy)]
struct RailRow {
    hub: Entity,
    target: Target,
}

/// Turns `hub`'s rail to `thing`'s row and brings it into view.
pub(super) fn follow(world: &mut World, hub: Entity, thing: Thing) {
    let Some(target) = target_of(world, thing) else {
        return;
    };
    let tab = match target {
        Target::Agent(_) | Target::Gone(_) | Target::Hamlet(_) => {
            Some(Tab::Hamlets)
        }
        Target::Profile(_) => Some(Tab::City),
        Target::Realm
        | Target::Hand
        | Target::Library
        | Target::Fold(_) => None,
    };
    if let Some(tab) = tab {
        turn(world, hub, tab);
    }
    if let Target::Agent(agent) = target {
        open_over(world, hub, agent);
    }
    let holds = |section: &Section| {
        section.opens == Some(target)
            || section.lines.iter().any(|line| line.target == target)
    };
    let shut = sections(world, hub)
        .into_iter()
        .find(|section| section.folded && holds(section))
        .map(|section| section.group);
    if let Some(group) = shut
        && let Some(mut rail) = world.get_mut::<Rail>(hub)
    {
        rail.open(group);
    }
    if let Some(mut reveal) = world.get_mut::<Reveal>(hub) {
        reveal.0 = Some((target, REVEAL_FRAMES));
    }
}

fn target_of(world: &World, thing: Thing) -> Option<Target> {
    let agent = |agent: Entity| match world
        .get::<Kind>(agent)
        .map(|kind| kind.0)
    {
        Some(protocol::AgentKind::Hand) => Target::Hand,
        _ => Target::Agent(agent),
    };
    Some(match thing {
        Thing::Agent(one) | Thing::House(one) => agent(one),
        Thing::Keeper | Thing::Cottage => {
            agent(hamlet_client::realm::first_lead(world)?)
        }
        Thing::Hand => Target::Hand,
        Thing::Common(hamlet) => Target::Hamlet(hamlet),
        Thing::Profile(profile) => Target::Profile(profile),
        Thing::Ruin(agent) => Target::Gone(agent),
        Thing::Library => Target::Library,
        _ => return None,
    })
}

/// Opens the folds `agent` is under in `hub`'s rail.
fn open_over(world: &mut World, hub: Entity, agent: Entity) {
    let mut over = Vec::new();
    let mut child = agent;
    while let Some(parent) = world
        .get::<Parent>(child)
        .and_then(|parent| {
            hamlet_client::realm::agent(world, parent.0)
        })
        .filter(|parent| !over.iter().any(|(at, _)| at == parent))
    {
        let worker = world.get::<Kind>(child).map(|kind| kind.0)
            == Some(protocol::AgentKind::Worker);
        over.push((parent, worker));
        child = parent;
    }
    let Some(mut rail) = world.get_mut::<Rail>(hub) else {
        return;
    };
    for (parent, worker) in over {
        rail.open(match worker {
            true => Group::Workers(parent),
            false => Group::Parent(parent),
        });
    }
}

/// Scrolls each Hub's rail just enough to show the row asked for.
fn reveal(
    mut hubs: Query<(Entity, &mut Reveal)>,
    rows: Query<(
        Entity,
        &RailRow,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    parents: Query<&ChildOf>,
    mut areas: Query<(
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
        &mut ScrollGoal,
    )>,
) {
    for (hub, mut asked) in &mut hubs {
        let Some((target, left)) = asked.0 else {
            continue;
        };
        let laid = rows.iter().find(|(_, row, node, _)| {
            row.hub == hub
                && row.target == target
                && node.size().y > 0.0
        });
        let Some((row, _, node, place)) = laid else {
            asked.0 = left.checked_sub(1).map(|left| (target, left));
            continue;
        };
        asked.0 = None;
        scroll_to(row, node, place, &parents, &mut areas);
    }
}

/// Scrolls the area around `row` just enough to show it.
fn scroll_to(
    row: Entity,
    node: &ComputedNode,
    place: &UiGlobalTransform,
    parents: &Query<&ChildOf>,
    areas: &mut Query<(
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
        &mut ScrollGoal,
    )>,
) {
    let Some((area, at, mut scroll, mut goal)) = parents
        .iter_ancestors(row)
        .find(|up| areas.contains(*up))
        .and_then(|up| areas.get_mut(up).ok())
    else {
        return;
    };
    let top = |node: &ComputedNode, at: &UiGlobalTransform| {
        at.translation.y - node.size().y / 2.0
    };
    let (row_top, area_top) = (top(node, place), top(area, at));
    let (row_end, area_end) =
        (row_top + node.size().y, area_top + area.size().y);
    let shift = if row_top < area_top {
        row_top - area_top
    } else if row_end > area_end {
        (row_end - area_end).min(row_top - area_top)
    } else {
        return;
    };
    *goal = ScrollGoal::default();
    scroll.y =
        (scroll.y + shift * area.inverse_scale_factor()).max(0.0);
}

/// A Hub's rail's search and folds.
#[derive(Component, PartialEq, Default)]
pub struct Rail {
    search: String,
    folded: Vec<Group>,
    opened: Vec<Group>,
    hidden: Vec<Entity>,
    /// Whether the list of hamlets to show is open.
    filtering: bool,
}

/// How wide the chief dragged a Hub's rail.
#[derive(Component)]
struct Width(f32);

impl Default for Width {
    fn default() -> Self {
        Self(WIDE)
    }
}

/// The part of the realm a Hub's rail lists under the tabs.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Hamlets,
    City,
}

/// Whether `group` starts folded, kept in `opened` once opened.
fn starts_folded(group: Group) -> bool {
    matches!(group, Group::Workers(_) | Group::Left)
}

impl Rail {
    fn toggle(&mut self, group: Group) {
        let kept = match starts_folded(group) {
            true => &mut self.opened,
            false => &mut self.folded,
        };
        match kept.iter().position(|g| *g == group) {
            Some(at) => {
                kept.remove(at);
            }
            None => kept.push(group),
        }
    }

    /// Opens `group` if it is folded.
    fn open(&mut self, group: Group) {
        if starts_folded(group) {
            if !self.opened.contains(&group) {
                self.opened.push(group);
            }
        } else {
            self.folded.retain(|folded| *folded != group);
        }
    }
}

/// The root of a Hub, whose width bounds the rail's.
#[derive(Component)]
pub(super) struct Frame(pub Entity);

/// The line between the rail and the page, dragged to size the rail.
#[derive(Component)]
pub(super) struct Divider;

#[derive(Component)]
struct Body;

/// The line beside `hub`'s rail, none while the rail is collapsed.
pub(super) fn divider(hub: Entity) -> impl View {
    keyed(
        component::<Collapsed, _>(hub, |collapsed| {
            collapsed.is_some()
        }),
        |collapsed| match collapsed {
            true => frame().boxed(),
            false => views::divider(Axis::Vertical)
                .rules(revealed_line)
                .tagged(Divider)
                .boxed(),
        },
    )
    .within(row(()).gap(0.0).shrink(0.0).height(percent(100.0)))
}

/// The divider is a hairline, in the accent while held or hovered.
fn revealed_line(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.fill(t.palette.hairline))
        .hovered(|s| s.fill(|t: &HamletTheme| t.palette.accent_fill))
        .when::<Dragging>(|s| {
            s.fill(|t: &HamletTheme| t.palette.accent_fill)
        })
        .transition(HamletTheme::interact)
        .apply(cx);
}

fn grab(
    start: On<PointerDragStart>,
    dividers: Query<(), With<Divider>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if dividers.contains(start.entity) {
        cursor.0 = Some(SystemCursorIcon::EwResize);
    }
}

/// Sizes the rail so the dragged line is at the pointer.
fn drag(
    drag: On<PointerDrag>,
    dividers: Query<&UiGlobalTransform, With<Divider>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform, &Frame)>,
    bodies: Query<(&ComputedNode, &UiGlobalTransform), With<Body>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut widths: Query<&mut Width>,
) {
    let Ok(line) = dividers.get(drag.entity) else {
        return;
    };
    let at = drag.pointer.position;
    let Some(frame) = parents
        .iter_ancestors(drag.entity)
        .find(|above| frames.contains(*above))
    else {
        return;
    };
    // The Hub starts where the rail does, in logical pixels.
    let Ok((node, place, hub)) = frames.get(frame) else {
        return;
    };
    let Ok(mut width) = widths.get_mut(hub.0) else {
        return;
    };
    let Some((body, body_at)) = children
        .iter_descendants(frame)
        .find_map(|kid| bodies.get(kid).ok())
    else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let left = (place.translation.x - node.size().x * 0.5) * scale;
    // The line stands the gap past the rail's edge, as last laid out.
    let edge = body_at.translation.x + body.size().x * 0.5;
    let past = (line.translation.x - edge) * scale;
    let most =
        (node.size().x * scale - PAGE).clamp(NARROWEST, WIDEST);
    width.0 = (at.x - left - past).clamp(NARROWEST, most);
}

fn drop(
    end: On<PointerDragEnd>,
    dividers: Query<(), With<Divider>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if dividers.contains(end.entity) {
        cursor.0 = None;
    }
}

/// The root of a Hub's search field.
#[derive(Component)]
struct Search(Entity);

fn read_search(
    roots: Query<(&Search, &Children)>,
    texts: Query<&EditableText>,
    mut rails: Query<&mut Rail>,
) {
    for (search, children) in &roots {
        let Some(text) =
            children.iter().find_map(|child| texts.get(*child).ok())
        else {
            continue;
        };
        let Ok(mut rail) = rails.get_mut(search.0) else {
            continue;
        };
        if text.value() != rail.search.as_str() {
            rail.search = text.value().to_string();
        }
    }
}

/// A part of the rail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Group {
    /// The realm and the Hand, with the Hand's own workers.
    Top,
    Needs,
    Hamlet(Entity),
    /// The agents under a parent, which fold into its row.
    Parent(Entity),
    /// The workers an agent or a chief started, folded into one row.
    Workers(Entity),
    City,
    /// Newcomers waiting to be let in, and those turned away.
    Harbor,
    Left,
}

/// What a row or a header leads to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Target {
    Realm,
    Hand,
    Agent(Entity),
    Hamlet(Entity),
    Profile(Entity),
    Library,
    Gone(Entity),
    Fold(Group),
}

impl Target {
    /// The thing whose menu a right click on its row opens.
    fn thing(self) -> Option<Thing> {
        match self {
            Target::Hand => Some(Thing::Hand),
            Target::Agent(agent) => Some(Thing::Agent(agent)),
            Target::Hamlet(hamlet) => Some(Thing::Common(hamlet)),
            Target::Profile(profile) => Some(Thing::Profile(profile)),
            Target::Library => Some(Thing::Library),
            Target::Gone(agent) => Some(Thing::Ruin(agent)),
            Target::Realm | Target::Fold(_) => None,
        }
    }
}

/// A row of the rail.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Line {
    pub target: Target,
    pub glyph: Glyph,
    pub name: String,
    /// The profile of the city its agent is of.
    pub tag: Option<ProfileTag>,
    /// What its tooltip says beside what it is.
    pub detail: String,
    /// When its age counts from, in milliseconds; 0 for none.
    pub since: u64,
    pub mark: Option<Standing>,
    /// What the search also finds it by: its address.
    pub address: String,
    /// How far under another agent it is set in. The lead's agents
    /// are set in as far as the lead.
    pub depth: u8,
    /// How many agents it is under, which folds go by.
    pub level: u8,
    /// Whether no row after it is set in under the same agent.
    pub last: bool,
    /// For each level above, whether its guide runs past this row.
    pub rails: Vec<bool>,
    pub current: bool,
    /// For a parent, the fold of the rows under it.
    pub fold: Option<(Group, bool)>,
    /// Whether it is a worker's, or its group's.
    pub worker: bool,
}

/// A group, its header and its rows.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Section {
    pub group: Group,
    pub title: String,
    /// Where pressing its title leads, if anywhere.
    pub opens: Option<Target>,
    /// The state of all of it, shown on its header.
    pub mark: Option<Standing>,
    pub count: usize,
    pub folded: bool,
    pub lines: Vec<Line>,
    /// Whether the page its title opens is the one shown.
    pub current: bool,
}

/// An agent as the rail reads it.
struct Agent {
    entity: Entity,
    id: protocol::AgentId,
    kind: protocol::AgentKind,
    parent: Option<protocol::AgentId>,
    hamlet: Option<Entity>,
    lead: bool,
    mark: Option<Standing>,
    /// Its address, and its name or its profile's name.
    address: String,
    called: String,
    tag: Option<ProfileTag>,
    /// What it is doing, while it works.
    doing: String,
    since: u64,
}

/// How long ago `then` was at `now`, short.
fn age(now: u64, then: u64) -> String {
    if then == 0 {
        return String::new();
    }
    Age::of(now, then).short()
}

/// The first line of `text` that has words, cut at `most` letters.
fn first_line(text: &str, most: usize) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty());
    cut(line.unwrap_or(""), most)
}

/// The chief's last words to the agent and its chat's last change.
fn thread(
    world: &World,
    id: protocol::AgentId,
) -> (Option<String>, u64) {
    let reference = protocol::ChatRef { with: id };
    let Some(chat) = hamlet_client::chats::find(world, &reference)
    else {
        return (None, 0);
    };
    let Some(transcript) = world.get::<Transcript>(chat) else {
        return (None, 0);
    };
    let items = transcript.items();
    let said =
        items.iter().rev().find_map(|item| match &item.entry {
            Entry::Chief { text, .. } => {
                let (_, own) =
                    crate::screens::chat::reply::own_words(text);
                Some(first_line(&own, LINE_SHOWN))
            }
            _ => None,
        });
    (said, items.last().map_or(0, |item| item.at))
}

/// What an agent with `address` is called in the rail.
fn called(address: &str, shown: Option<&Shown>) -> String {
    shown.map_or_else(|| address.to_string(), |shown| shown.0.clone())
}

/// The line under an agent's name and its chat's last change.
pub(crate) fn under_name(
    world: &World,
    agent: Entity,
) -> (String, u64) {
    let (said, last) = world
        .get::<Identity>(agent)
        .map_or((None, 0), |id| thread(world, id.0));
    let words = world
        .get::<Activity>(agent)
        .map(|activity| activity.words.clone())
        .unwrap_or_default();
    let title = world
        .get::<Busy>(agent)
        .map(|_| words.clone())
        .or(said.map(|said| format!("You: {said}")))
        .or_else(|| {
            world
                .get::<Purpose>(agent)
                .map(|p| first_line(&p.0, LINE_SHOWN))
                .filter(|line| !line.is_empty())
        })
        .unwrap_or(words);
    if title.eq_ignore_ascii_case(Standing::of(world, agent).word()) {
        (String::new(), last)
    } else {
        (title, last)
    }
}

fn agents(world: &World) -> Vec<Agent> {
    // How many waits each address raised, counted once for every row.
    let mut raised = HashMap::<&str, usize>::new();
    if let Some(mut waits) = world.try_query::<&Waiting>() {
        for wait in waits.iter(world) {
            if !matches!(
                wait.detail,
                protocol::WaitDetail::Newcomer { .. }
            ) {
                *raised.entry(wait.by.as_str()).or_default() += 1;
            }
        }
    }
    let Some(mut query) = world.try_query::<(
        Entity,
        &Identity,
        &Kind,
        Option<&Parent>,
        Option<&LivesIn>,
        &Lived,
        Option<&Shown>,
        (Has<Lead>, Has<Left>),
    )>() else {
        return Vec::new();
    };
    query
        .iter(world)
        .filter(|(.., (_, left))| !left)
        .map(
            |(
                entity,
                id,
                kind,
                parent,
                lives,
                lived,
                shown,
                (lead, _),
            )| {
                let address = name_of(world, entity);
                let last = world
                    .get::<Identity>(entity)
                    .map_or(0, |id| thread(world, id.0).1);
                let doing = world
                    .get::<Busy>(entity)
                    .and_then(|_| world.get::<Activity>(entity))
                    .map(|activity| activity.words.clone())
                    .unwrap_or_default();
                Agent {
                    entity,
                    id: id.0,
                    kind: kind.0,
                    parent: parent.map(|parent| parent.0),
                    hamlet: lives.map(|lives| lives.0),
                    lead,
                    mark: Some(Standing::raised(
                        world,
                        entity,
                        raised
                            .get(address.as_str())
                            .copied()
                            .unwrap_or(0),
                    )),
                    called: called(&address, shown),
                    address,
                    tag: ProfileTag::of(world, entity),
                    doing,
                    since: last.max(lived.since),
                }
            },
        )
        .collect()
}

/// A place in a tree: an agent, or the workers one started.
enum Node<'a> {
    Agent(&'a Agent),
    Workers {
        group: Group,
        workers: Vec<&'a Agent>,
    },
}

impl Agent {
    fn is_worker(&self) -> bool {
        self.kind == protocol::AgentKind::Worker
    }
}

/// The agents and the workers under each agent, by name.
type Under<'a> =
    HashMap<protocol::AgentId, (Vec<&'a Agent>, Vec<&'a Agent>)>;

/// Who is under each of `some`, among `some`.
fn under<'a>(some: &[&'a Agent]) -> Under<'a> {
    let mut under = Under::new();
    for agent in some {
        let Some(parent) = agent.parent else {
            continue;
        };
        let (agents, workers) = under.entry(parent).or_default();
        match agent.is_worker() {
            true => workers.push(*agent),
            false => agents.push(*agent),
        }
    }
    for (agents, workers) in under.values_mut() {
        agents
            .sort_by_cached_key(|agent| agent.called.to_lowercase());
        workers
            .sort_by_cached_key(|agent| agent.called.to_lowercase());
    }
    under
}

/// `root` and everyone `under` it, each with its depth below `root`.
fn tree<'a>(
    root: &'a Agent,
    under: &Under<'a>,
) -> Vec<(Node<'a>, u8)> {
    fn walk<'a>(
        agent: &'a Agent,
        under: &Under<'a>,
        depth: u8,
        seen: &mut HashSet<protocol::AgentId>,
        out: &mut Vec<(Node<'a>, u8)>,
    ) {
        if !seen.insert(agent.id) {
            return;
        }
        out.push((Node::Agent(agent), depth));
        let Some((agents, workers)) = under.get(&agent.id) else {
            return;
        };
        let depth = depth.saturating_add(1);
        for child in agents {
            walk(child, under, depth, seen, out);
        }
        if !workers.is_empty() {
            let group = Group::Workers(agent.entity);
            let workers = workers.clone();
            out.push((Node::Workers { group, workers }, depth));
        }
    }
    let mut out = Vec::new();
    walk(root, under, 0, &mut HashSet::new(), &mut out);
    out
}

/// The rows of `family` in `hub`'s rail, each set in as deep as it
/// is in the tree.
fn family_lines(
    world: &World,
    hub: Entity,
    family: &[(Node<'_>, u8)],
    open: &dyn Fn(Group) -> bool,
) -> Vec<Line> {
    let mut rows = Vec::<(Line, u8)>::new();
    let mut lifted = false;
    for (node, depth) in family {
        let depth = *depth;
        if depth == 0 {
            lifted = matches!(node, Node::Agent(agent)
                if agent.lead || agent.kind == protocol::AgentKind::Hand);
        }
        let set_in = |level: u8| {
            if lifted {
                level.saturating_sub(1)
            } else {
                level
            }
        };
        match node {
            Node::Agent(agent) => {
                let mut line =
                    agent_line(world, hub, agent, depth, false);
                line.level = depth;
                rows.push((line, set_in(depth)));
            }
            Node::Workers { group, workers } => {
                let shown = open(*group);
                let mut line =
                    workers_line(*group, workers, shown, depth);
                line.level = depth;
                rows.push((line, set_in(depth)));
                if shown {
                    let under = depth.saturating_add(1);
                    for worker in workers {
                        let mut line = agent_line(
                            world, hub, worker, under, false,
                        );
                        line.level = under;
                        rows.push((line, set_in(under)));
                    }
                }
            }
        }
    }
    // Whether each row is the last at its depth, from the end.
    let mut lasts = vec![true; rows.len()];
    let mut follows = Vec::<bool>::new();
    for (at, (_, depth)) in rows.iter().enumerate().rev() {
        let level = usize::from(*depth);
        follows.resize(level + 1, false);
        lasts[at] = !follows[level];
        follows[level] = true;
    }
    // Whether the agent last seen at each depth has more after it.
    let mut more = Vec::<bool>::new();
    rows.into_iter()
        .zip(lasts)
        .map(|((mut line, depth), last)| {
            let depth = depth.min(DEEPEST);
            let level = usize::from(depth);
            line.depth = depth;
            line.last = last;
            line.rails =
                more.get(1..level).unwrap_or_default().to_vec();
            more.resize(level + 1, false);
            more[level] = !last;
            line
        })
        .collect()
}

/// The row a group of `workers` folds into.
fn workers_line(
    group: Group,
    workers: &[&Agent],
    open: bool,
    depth: u8,
) -> Line {
    let count = workers.len();
    Line {
        target: Target::Fold(group),
        glyph: Glyph::Worker,
        name: match count {
            1 => "1 worker".into(),
            n => format!("{n} workers"),
        },
        tag: None,
        detail: String::new(),
        since: 0,
        mark: (!open).then(|| {
            Standing::of_all(workers.iter().filter_map(|w| w.mark))
        }),
        address: workers
            .iter()
            .map(|worker| worker.address.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        depth,
        last: true,
        rails: Vec::new(),
        level: 0,
        current: false,
        fold: Some((group, !open)),
        worker: true,
    }
}

fn kind_glyph(agent: &Agent) -> Glyph {
    match agent.kind {
        protocol::AgentKind::Hand => Glyph::Tower,
        protocol::AgentKind::Hamlet if agent.lead => Glyph::Lead,
        protocol::AgentKind::Hamlet => Glyph::HamletAgent,
        protocol::AgentKind::Worker => Glyph::Worker,
    }
}

/// Whether `target` is what `hub` shows.
fn is_current(world: &World, hub: Entity, target: Target) -> bool {
    let showing = super::showing(world, hub);
    let seated = crate::convo::of(world, hub).map(|c| c.agent);
    match (target, showing) {
        (
            Target::Hand | Target::Agent(_),
            Some(Showing::Thread | Showing::Prompt),
        ) => match target {
            Target::Agent(agent) => seated == Some(agent),
            _ => seated.is_some_and(|agent| {
                world.get::<Kind>(agent).map(|kind| kind.0)
                    == Some(protocol::AgentKind::Hand)
            }),
        },
        (Target::Profile(a), Some(Showing::Profile(b)))
        | (Target::Gone(a), Some(Showing::Gone(b)))
        | (Target::Hamlet(a), Some(Showing::Hamlet(b))) => a == b,
        (Target::Library, Some(Showing::Library))
        | (Target::Realm, Some(Showing::Realm)) => true,
        _ => false,
    }
}

fn agent_line(
    world: &World,
    hub: Entity,
    agent: &Agent,
    depth: u8,
    last: bool,
) -> Line {
    let hand = agent.kind == protocol::AgentKind::Hand;
    let target = if hand {
        Target::Hand
    } else {
        Target::Agent(agent.entity)
    };
    Line {
        target,
        glyph: kind_glyph(agent),
        name: if hand {
            "The Hand".into()
        } else {
            agent.called.clone()
        },
        tag: agent.tag.clone(),
        detail: agent.doing.clone(),
        since: agent.since,
        mark: agent.mark,
        address: agent.address.clone(),
        depth,
        last,
        rails: Vec::new(),
        level: 0,
        current: is_current(world, hub, target),
        fold: None,
        worker: agent.kind == protocol::AgentKind::Worker,
    }
}

/// `lines` without the rows under each folded parent.
fn fold_parents(lines: Vec<Line>, folded: &[Group]) -> Vec<Line> {
    let mut lines = lines;
    // From the end, each open subtree with its most pressing mark.
    let mut after = Vec::<(u8, Standing)>::new();
    for at in (0..lines.len()).rev() {
        let depth = lines[at].level;
        let mut under = false;
        let mut mark = Standing::of_all(lines[at].mark);
        while let Some(&(below, of)) = after.last()
            && below > depth
        {
            after.pop();
            under = true;
            mark = Standing::of_all([mark, of]);
        }
        after.push((depth, mark));
        let Target::Agent(agent) = lines[at].target else {
            continue;
        };
        if !under {
            continue;
        }
        let group = Group::Parent(agent);
        let shut = folded.contains(&group);
        let line = &mut lines[at];
        line.fold = Some((group, shut));
        if shut {
            line.mark = Some(mark);
        }
    }
    let mut kept = Vec::with_capacity(lines.len());
    let mut hidden_below: Option<u8> = None;
    for line in lines {
        if let Some(depth) = hidden_below {
            if line.level > depth {
                continue;
            }
            hidden_below = None;
        }
        if matches!(line.fold, Some((_, true))) {
            hidden_below = Some(line.level);
        }
        kept.push(line);
    }
    kept
}

/// A row of `hub`'s rail that leads to a page.
fn page_line(
    world: &World,
    hub: Entity,
    target: Target,
    glyph: Glyph,
    name: &str,
    detail: String,
    mark: Option<Standing>,
) -> Line {
    Line {
        target,
        glyph,
        name: name.into(),
        tag: None,
        detail,
        since: 0,
        mark,
        address: String::new(),
        depth: 0,
        last: true,
        rails: Vec::new(),
        level: 0,
        current: is_current(world, hub, target),
        fold: None,
        worker: false,
    }
}

fn matches(line: &Line, search: &str) -> bool {
    search.is_empty()
        || line.name.to_lowercase().contains(search)
        || line.address.to_lowercase().contains(search)
        || line.tag.as_ref().is_some_and(|tag| {
            tag.name.to_lowercase().contains(search)
        })
}

/// A row for each who waits on the chief, and the count of all waits.
fn needs(world: &World, all: &[Agent]) -> (Vec<Line>, usize) {
    let Some(mut query) = world.try_query::<&Waiting>() else {
        return (Vec::new(), 0);
    };
    let mut waits = query.iter(world).collect::<Vec<_>>();
    let total = waits.len();
    waits.sort_by_key(|wait| wait.since);
    let by_address = all
        .iter()
        .map(|agent| (agent.address.as_str(), agent))
        .collect::<HashMap<_, _>>();
    let mut lines = Vec::<Line>::new();
    let mut rows = HashMap::<&str, usize>::new();
    let mut counts = Vec::<usize>::new();
    for wait in waits {
        // A newcomer leads to the profile at the harbor.
        if let protocol::WaitDetail::Newcomer { profile } =
            &wait.detail
            && let Some(entity) =
                hamlet_client::realm::profile(world, *profile)
        {
            let name = world
                .get::<Profile>(entity)
                .map(|s| s.0.name.clone())
                .unwrap_or_default();
            lines.push(Line {
                target: Target::Profile(entity),
                glyph: Glyph::Profile,
                name: format!("{name} · harbor"),
                tag: None,
                detail: "Wants to move into the city".into(),
                since: wait.since,
                mark: Some(Standing::Waits),
                address: name,
                depth: 0,
                last: true,
                rails: Vec::new(),
                level: 0,
                current: false,
                fold: None,
                worker: false,
            });
            counts.push(1);
            continue;
        }
        if let Some(&at) = rows.get(wait.by.as_str()) {
            counts[at] += 1;
            continue;
        }
        rows.insert(wait.by.as_str(), lines.len());
        let line = wait.line(LINE_SHOWN);
        let said = line
            .strip_prefix(wait.shown.as_str())
            .unwrap_or(&line)
            .trim_start();
        let (target, glyph, name, tag) = match by_address
            .get(wait.by.as_str())
        {
            Some(agent)
                if agent.kind == protocol::AgentKind::Hand =>
            {
                (
                    Target::Hand,
                    Glyph::Tower,
                    "The Hand".to_string(),
                    None,
                )
            }
            Some(agent) => (
                Target::Agent(agent.entity),
                kind_glyph(agent),
                agent.called.clone(),
                agent.tag.clone(),
            ),
            // The realm's page has every wait in full.
            None => {
                (Target::Realm, Glyph::Bell, wait.shown.clone(), None)
            }
        };
        lines.push(Line {
            target,
            glyph,
            name,
            tag,
            detail: capitalised(said),
            since: wait.since,
            mark: Some(Standing::Waits),
            address: wait.by.clone(),
            depth: 0,
            last: true,
            rails: Vec::new(),
            level: 0,
            current: false,
            fold: None,
            // What waits is never set lighter.
            worker: false,
        });
        counts.push(1);
    }
    for (line, count) in lines.iter_mut().zip(counts) {
        if count > 1 {
            line.detail = format!("{count} waits · {}", line.detail);
        }
    }
    (lines, total)
}

fn capitalised(text: &str) -> String {
    let mut letters = text.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// The city's profiles, the harbor, the library and those that left,
/// in `hub`'s rail.
fn society(world: &World, hub: Entity, sections: &mut Vec<Section>) {
    let mut city = Vec::new();
    let mut harbor = Vec::new();
    let mut gone = Vec::new();
    // A newcomer waits for the chief only while the chief holds the
    // door.
    let chief_holds_door = world
        .get_resource::<hamlet_client::realm::Settings>()
        .is_some_and(|s| s.0.newcomers == protocol::Newcomers::Chief);
    if let Some(mut query) = world.try_query::<(Entity, &Profile)>() {
        let mut found =
            query.iter(world).collect::<Vec<(Entity, &Profile)>>();
        found.sort_by_key(|(_, profile)| {
            profile.0.name.to_lowercase()
        });
        for (entity, profile) in found {
            let profile = &profile.0;
            // Where its agents work, by hamlet.
            let mut hamlets = profile
                .agents
                .iter()
                .filter_map(|agent| {
                    hamlet_client::realm::agent(world, *agent)
                })
                .filter(|agent| world.get::<Left>(*agent).is_none())
                .filter_map(|agent| world.get::<LivesIn>(agent))
                .map(|lives| name_of(world, lives.0))
                .collect::<Vec<_>>();
            hamlets.sort();
            hamlets.dedup();
            let (title, mark) = match (&profile.left, profile.arrival)
            {
                (Some(_), _) => {
                    ("Left the city".into(), Some(Standing::Gone))
                }
                (None, protocol::Arrival::Waiting) => (
                    "Waits to be let in".into(),
                    if chief_holds_door {
                        Some(Standing::Waits)
                    } else {
                        None
                    },
                ),
                (None, protocol::Arrival::Turned) => {
                    ("Turned away".into(), Some(Standing::Turned))
                }
                (None, protocol::Arrival::Housed)
                    if hamlets.is_empty() =>
                {
                    ("Not at work".into(), None)
                }
                (None, protocol::Arrival::Housed) => {
                    (format!("In {}", hamlets.join(", ")), None)
                }
                // A blank agent's profile is not the city's.
                (None, protocol::Arrival::Apart) => continue,
            };
            let line = page_line(
                world,
                hub,
                Target::Profile(entity),
                Glyph::Profile,
                &profile.name,
                title,
                mark,
            );
            match (&profile.left, profile.arrival) {
                (Some(_), protocol::Arrival::Apart) => {}
                (Some(_), _) => gone.push(line),
                (None, protocol::Arrival::Housed) => city.push(line),
                (None, _) => harbor.push(line),
            }
        }
    }
    for (group, title, lines) in [
        (Group::City, "City", city),
        (Group::Harbor, "At the harbor", harbor),
    ] {
        sections.push(Section {
            group,
            title: title.into(),
            opens: None,
            mark: None,
            count: lines.len(),
            folded: false,
            lines,
            current: false,
        });
    }
    if let Some(mut query) = world.try_query::<(
        Entity,
        &Lived,
        Option<&MergedInto>,
        &Kind,
    )>() {
        let mut left = query
            .iter(world)
            .filter(|(entity, ..)| {
                world.get::<Left>(*entity).is_some()
            })
            .filter(|(_, _, _, kind)| {
                kind.0 != protocol::AgentKind::Worker
            })
            .collect::<Vec<_>>();
        left.sort_by_key(|(_, lived, ..)| {
            std::cmp::Reverse(lived.until.unwrap_or(0))
        });
        for (entity, _, into, _) in left.into_iter().take(LEFT_SHOWN)
        {
            let address = name_of(world, entity);
            let mut line = page_line(
                world,
                hub,
                Target::Gone(entity),
                Glyph::HamletAgent,
                &shown_of(world, entity),
                match into {
                    Some(host) => {
                        format!(
                            "Merged into {}",
                            shown_of(world, host.0)
                        )
                    }
                    None => "Ended".into(),
                },
                Some(if into.is_some() {
                    Standing::Merged
                } else {
                    Standing::Gone
                }),
            );
            line.address = address;
            gone.push(line);
        }
    }
    let count = gone.len();
    sections.push(Section {
        group: Group::Left,
        title: "Left".into(),
        opens: None,
        mark: None,
        count,
        folded: false,
        lines: gone,
        current: false,
    });
}

/// The rows of `hub`'s rail in their groups.
pub(super) fn sections(world: &World, hub: Entity) -> Vec<Section> {
    let (search, folded, opened, hidden) =
        world.get::<Rail>(hub).map_or_else(
            || (String::new(), Vec::new(), Vec::new(), Vec::new()),
            |rail| {
                (
                    rail.search.trim().to_lowercase(),
                    rail.folded.clone(),
                    rail.opened.clone(),
                    rail.hidden.clone(),
                )
            },
        );
    // Workers are folded until opened, but a search finds them.
    let open =
        |group: Group| !search.is_empty() || opened.contains(&group);
    let tab = world.get::<Tab>(hub).copied().unwrap_or_default();
    let all = agents(world);
    let mut sections = Vec::new();

    let waiting = world
        .try_query::<&Waiting>()
        .map_or(0, |mut waits| waits.iter(world).count());
    let mut top = vec![page_line(
        world,
        hub,
        Target::Realm,
        Glyph::Globe,
        "Realm",
        match waiting {
            0 => String::new(),
            n => format!("{n} {}", need_you(n)),
        },
        (waiting > 0).then_some(Standing::Waits),
    )];
    if let Some(hand) =
        all.iter().find(|a| a.kind == protocol::AgentKind::Hand)
    {
        let outside = all
            .iter()
            .filter(|agent| agent.hamlet.is_none())
            .collect::<Vec<_>>();
        let family = tree(hand, &under(&outside));
        top.extend(family_lines(world, hub, &family, &open));
    }
    let shelf =
        world.get_resource::<Skills>().map_or(0, |s| s.0.len());
    top.push(page_line(
        world,
        hub,
        Target::Library,
        Glyph::Book,
        "Library",
        match shelf {
            1 => "1 skill on its shelves".into(),
            n => format!("{n} skills on its shelves"),
        },
        None,
    ));
    sections.push(Section {
        group: Group::Top,
        title: String::new(),
        opens: None,
        mark: None,
        count: 0,
        folded: false,
        lines: top,
        current: false,
    });

    let (needs, count) = needs(world, &all);
    sections.push(Section {
        group: Group::Needs,
        title: "Needs you".into(),
        opens: Some(Target::Realm),
        mark: None,
        count,
        folded: false,
        lines: needs,
        current: false,
    });

    let mut by_hamlet = HashMap::<Entity, Vec<&Agent>>::new();
    for agent in &all {
        if let Some(hamlet) = agent.hamlet {
            by_hamlet.entry(hamlet).or_default().push(agent);
        }
    }
    for (hamlet, name) in hamlet_names(world) {
        let here = by_hamlet.remove(&hamlet).unwrap_or_default();
        let ids =
            here.iter().map(|agent| agent.id).collect::<HashSet<_>>();
        let mut roots = here
            .iter()
            .copied()
            .filter(|agent| {
                agent
                    .parent
                    .is_none_or(|parent| !ids.contains(&parent))
            })
            .collect::<Vec<_>>();
        roots
            .sort_by_key(|agent| (!agent.lead, agent.called.clone()));
        // The workers a chief started, after the tree.
        let (chiefs, roots): (Vec<_>, Vec<_>) =
            roots.into_iter().partition(|agent| agent.is_worker());
        let under = under(&here);
        let mut family = Vec::new();
        for root in roots {
            family.extend(tree(root, &under));
        }
        if !chiefs.is_empty() {
            let group = Group::Workers(hamlet);
            family.push((
                Node::Workers {
                    group,
                    workers: chiefs,
                },
                0,
            ));
        }
        let mut lines = family_lines(world, hub, &family, &open);
        let group = Group::Hamlet(hamlet);
        let mark = Some(Standing::of_all(
            lines.iter().filter_map(|line| line.mark),
        ));
        // A search finds rows a parent would fold away.
        if search.is_empty() {
            lines = fold_parents(lines, &folded);
        }
        sections.push(Section {
            group,
            title: name,
            opens: Some(Target::Hamlet(hamlet)),
            mark,
            count: here.len(),
            folded: folded.contains(&group)
                && mark != Some(Standing::Waits),
            lines,
            current: is_current(world, hub, Target::Hamlet(hamlet)),
        });
    }

    society(world, hub, &mut sections);
    for section in &mut sections {
        // Every part with a header folds, but never over a wait.
        if matches!(
            section.group,
            Group::City | Group::Harbor | Group::Left
        ) {
            let shut = match starts_folded(section.group) {
                true => !opened.contains(&section.group),
                false => folded.contains(&section.group),
            };
            section.folded = shut
                && !section
                    .lines
                    .iter()
                    .any(|l| l.mark == Some(Standing::Waits));
        }
        if !search.is_empty() {
            section.folded = false;
            section.lines.retain(|line| matches(line, &search));
        }
        if section.group == Group::Left {
            section.lines.retain(|line| {
                matches!(line.target, Target::Profile(_))
                    == (tab == Tab::City)
            });
            section.count = section.lines.len();
        }
    }
    sections.retain(|section| {
        let shown = match section.group {
            Group::Top
            | Group::Needs
            | Group::Left
            | Group::Parent(_)
            | Group::Workers(_) => true,
            Group::Hamlet(hamlet) => {
                tab == Tab::Hamlets
                    && (!hidden.contains(&hamlet)
                        || section.mark == Some(Standing::Waits))
            }
            Group::City | Group::Harbor => tab == Tab::City,
        };
        shown && !section.lines.is_empty()
    });
    sections
}

/// Every hamlet with its name, in name order.
fn hamlet_names(world: &World) -> Vec<(Entity, String)> {
    let mut found = world
        .try_query::<(Entity, &HamletId)>()
        .map(|mut query| {
            query
                .iter(world)
                .map(|(hamlet, _)| (hamlet, name_of(world, hamlet)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    found.sort_by_key(|(_, name)| name.to_lowercase());
    found
}

/// The tabs of `hub`'s rail between the hamlets and the city.
fn tabs_of(hub: Entity) -> [((Entity, Tab), Glyph, &'static str); 2] {
    [
        ((hub, Tab::Hamlets), Glyph::Hamlet, "Hamlets"),
        ((hub, Tab::City), Glyph::Profile, "City"),
    ]
}

/// An icon button of the search row.
fn tool(
    which: Glyph,
    says: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    button(glyph(which, ICON))
        .padding(UiRect::all(px(4.0)))
        .rules(views::tint)
        .tooltip(move || note(says))
        .on_activate(act)
        .boxed()
}

/// The search over `hub`'s rail's rows, with the tab's tools at its
/// end.
fn search_row(hub: Entity) -> impl View {
    // Built again when the rail expands, with the search it had.
    let field = AnyView::new(move |cx: &mut Cx| {
        let now = cx
            .world
            .get::<Rail>(hub)
            .map(|rail| rail.search.clone())
            .unwrap_or_default();
        cx.build(search(
            text_field(Binding::new(now, |_, _| {}))
                .rules(text_input)
                .tagged(Search(hub)),
            "Search",
        ))
    });
    // Built once, shown only on their tab.
    let on = |shown: Tab, view: AnyView| {
        column((view,)).display(component::<Tab, _>(
            hub,
            move |tab| {
                if tab == Some(&shown) {
                    Display::Flex
                } else {
                    Display::None
                }
            },
        ))
    };
    let tools = row((
        on(
            Tab::Hamlets,
            tool(
                Glyph::Sliders,
                "Choose which hamlets are shown",
                move |w| {
                    if let Some(mut rail) = w.get_mut::<Rail>(hub) {
                        rail.filtering = !rail.filtering;
                    }
                },
            ),
        ),
        on(
            Tab::Hamlets,
            tool(Glyph::Plus, "Create a hamlet", |w| {
                crate::world_menu::ask_found(w);
            }),
        ),
        on(
            Tab::City,
            tool(
                Glyph::Plus,
                "Make a profile",
                crate::world_menu::ask_new_profile,
            ),
        ),
    ))
    .gap(2.0);
    with_cx(move |cx| {
        row((field, tools))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
    })
}

/// A switch for each hamlet that shows or hides it in `hub`'s rail.
fn filter_list(hub: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let Some(rail) = world.get::<Rail>(hub) else {
                return (false, Vec::new(), Vec::new());
            };
            let open = rail.filtering
                && world.get::<Tab>(hub) == Some(&Tab::Hamlets);
            let names = match open {
                true => hamlet_names(world),
                false => Vec::new(),
            };
            (open, rail.hidden.clone(), names)
        })
        .when(every_frame()),
        move |(open, hidden, names)| {
            if !*open {
                return frame().boxed();
            }
            column(
                names
                    .iter()
                    .cloned()
                    .map(|(hamlet, name)| {
                        labelled_switch(
                            name,
                            !hidden.contains(&hamlet),
                            move |world, show| {
                                let Some(mut rail) =
                                    world.get_mut::<Rail>(hub)
                                else {
                                    return;
                                };
                                let hidden = &mut rail.hidden;
                                hidden.retain(|h| *h != hamlet);
                                if !show {
                                    hidden.push(hamlet);
                                }
                            },
                        )
                        .boxed()
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(6.0)
            .padding(UiRect::axes(px(CHEVRON), px(4.0)))
            .boxed()
        },
    )
    .within(hamlet_ui::styles::bare())
}

/// The tabs, the search under them, and the hamlets to show.
fn tabs_and_search(hub: Entity) -> impl View {
    with_cx(move |cx| {
        column((
            tabs::tipped_strip(
                &tabs_of(hub),
                &[words::HAMLET, words::CITY],
                Spread::Shared,
                |world, (hub, tab)| turn(world, hub, tab),
            ),
            search_row(hub),
            filter_list(hub),
        ))
        .gap(cx.theme().space.gap)
    })
}

/// What pressing a row of `hub`'s rail does. Only the Hub that
/// follows the world's selection selects in the world.
fn go(world: &mut World, hub: Entity, target: Target) {
    let follows = super::is_following(world, hub);
    match target {
        Target::Realm => show(world, hub, Showing::Realm),
        Target::Hand if follows => {
            talk_to_hand(world);
            crate::chat::focus(world, hub);
        }
        Target::Hand => {
            if let Some(hand) = hamlet_client::realm::hand(world) {
                seat(world, hub, hand);
                show(world, hub, Showing::Thread);
                crate::chat::focus(world, hub);
            }
        }
        // Its thread, and its house brought into view.
        Target::Agent(agent) => {
            super::talk_in(world, hub, agent);
            crate::chat::focus(world, hub);
            if follows && world.contains_resource::<Selected>() {
                let home = crate::convo::home_of(world, agent);
                world.trigger(BringIntoView(home));
            }
        }
        Target::Hamlet(hamlet) => {
            show(world, hub, Showing::Hamlet(hamlet));
            look_at(world, hub, Thing::Common(hamlet));
        }
        // Also selected in the world.
        Target::Profile(profile) => {
            show(world, hub, Showing::Profile(profile));
            look_at(world, hub, Thing::Profile(profile));
        }
        Target::Library => {
            show(world, hub, Showing::Library);
            look_at(world, hub, Thing::Library);
        }
        Target::Gone(agent) => show(world, hub, Showing::Gone(agent)),
        Target::Fold(group) => {
            if let Some(mut rail) = world.get_mut::<Rail>(hub) {
                rail.toggle(group);
            }
        }
    }
}

fn row_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(transparent(t.palette.button))
                .border(t.space.border)
                .border_color(transparent(t.palette.accent))
                .radius(t.space.round)
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::Stretch)
                .padding(UiRect::axes(px(ROW_INSET), px(ROW_DOWN)))
        })
        .pressed(|s| s.frame(|f, t| f.fill(t.pressed())))
        .transition(HamletTheme::interact)
        .apply(cx);
}

/// `shown` on a frame highlighted while `held` or hovered.
fn washed(shown: impl View + 'static, held: bool) -> impl View {
    with_cx(move |cx| {
        row((shown,))
            .grow(1.0)
            .min_width(px(0.0))
            .radius(cx.theme().space.round)
            .tagged(Wash::new(held, Edge::Left))
    })
}

/// The mark at a row's end, its room kept when empty.
fn mark_icon(mark: Option<Standing>) -> AnyView {
    match mark {
        Some(standing) => status_icon_at(
            standing.glyph(),
            standing.word(),
            standing.kind(),
            ICON,
        )
        .boxed(),
        None => frame().width(px(ICON + STATUS_PAD.0 * 2.0)).boxed(),
    }
}

/// The chevron that folds `group` in `hub`'s rail.
fn chevron(
    hub: Entity,
    group: Group,
    shut: bool,
    says: &'static str,
) -> AnyView {
    button(
        glyph(Glyph::Chevron, 10.0)
            .tone(if group == Group::Left {
                Tone::Dim
            } else {
                Tone::Faint
            })
            .rotation(if shut { 90.0 } else { 180.0 }),
    )
    .width(px(CHEVRON))
    .shrink(0.0)
    .padding(UiRect::all(px(0.0)))
    .rules(views::tint)
    .tooltip(move || note(says))
    .on_activate(move |world| {
        if let Some(mut rail) = world.get_mut::<Rail>(hub) {
            rail.toggle(group);
        }
    })
    .boxed()
}

/// The least height of `line`'s row.
fn row_height(line: &Line) -> f32 {
    if line.worker { WORKER_ROW } else { ROW }
}

/// A row of one line in `hub`'s rail. A `dimmed` row, of those that
/// left, is grey.
fn expanded(hub: Entity, line: &Line, dimmed: bool) -> AnyView {
    let (target, since, worker) =
        (line.target, line.since, line.worker);
    let name_tone = if line.current {
        Tone::Accent
    } else if dimmed || worker {
        Tone::Dim
    } else {
        Tone::Body
    };
    let name =
        column((one_line(label(line.name.clone()).tone(name_tone)),))
            .min_width(px(0.0))
            .overflow(Overflow::clip());
    let mut words = vec![name.boxed(), frame().grow(1.0).boxed()];
    words.push(
        label(resource::<Minute, _>(move |_| age(now_ms(), since)))
            .wrap(false)
            .size(AGE_TEXT)
            .tone(Tone::Dim)
            .boxed(),
    );
    words.push(mark_icon(line.mark));
    // The profile's badge sits on the icon's top-left corner.
    let mut icon = vec![
        glyph(line.glyph, if worker { WORKER_ICON } else { ICON })
            .tone(Tone::Dim)
            .opacity(if dimmed { DIMMED } else { 1.0 })
            .boxed(),
    ];
    icon.extend(line.tag.as_ref().map(profile_tag::badge));
    let content = row((
        row(icon).shrink(0.0),
        row(words)
            .gap(6.0)
            .align(AlignItems::Center)
            .grow(1.0)
            .min_width(px(0.0)),
    ))
    .gap(if worker { 6.0 } else { 8.0 })
    .align(AlignItems::Center)
    .min_height(px(row_height(line) - 2.0 * ROW_DOWN))
    .width(percent(100.0));
    let pressed = button(content)
        .grow(1.0)
        .min_width(px(0.0))
        .rules(row_rules)
        .tagged(Variant::Default)
        .on_activate(move |world| go(world, hub, target));
    let pressed = match target.thing() {
        Some(thing) => pressed.tagged(Asks(thing)).boxed(),
        None => pressed.boxed(),
    };
    let shown = washed(pressed, line.current).boxed();
    let shown = match tip_of(line) {
        Some(tip) => shown.tooltip(move || note(tip.clone())).boxed(),
        None => shown.boxed(),
    };
    // Outside the row's press, so folding does not open the thread.
    let fold = match line.fold {
        Some((group, shut)) => chevron(
            hub,
            group,
            shut,
            match (group, shut) {
                (Group::Workers(_), true) => "Show the workers",
                (Group::Workers(_), false) => "Fold the workers",
                (_, true) => "Show the agents under it",
                (_, false) => "Fold the agents under it",
            },
        ),
        None => frame().width(px(CHEVRON)).shrink(0.0).boxed(),
    };
    let step = frame()
        .width(px(f32::from(line.depth) * STEP_IN))
        .shrink(0.0);
    let mut parts = vec![step.boxed(), fold, shown];
    parts.extend(tree_lines(line));
    row(parts)
        .gap(0.0)
        .align(AlignItems::Center)
        .width(percent(100.0))
        .tagged(RailRow { hub, target })
        .boxed()
}

/// The guides of the levels above a row, then its elbow.
fn tree_lines(line: &Line) -> Vec<AnyView> {
    if line.depth == 0 {
        return Vec::new();
    }
    // Where the guide of the agent at `level` runs.
    let guide =
        |level: usize| level as f32 * STEP_IN + CHEVRON / 2.0 - 0.5;
    let parent = usize::from(line.depth) - 1;
    // Where the elbow meets the row, and where it stops.
    let elbow = row_height(line) / 2.0;
    // A line laid over the row, `left` and `top` in from its edges;
    // one with no height runs to its foot.
    let over =
        |left: f32, top: f32, width: f32, height: Option<f32>| {
            with_cx(move |cx| {
                frame()
                    .width(px(width))
                    .height(height.map_or(Val::Auto, px))
                    .fill(cx.theme().palette.hairline)
                    .position(PositionType::Absolute)
                    .inset(UiRect::new(
                        px(left),
                        Val::Auto,
                        px(top),
                        if height.is_some() {
                            Val::Auto
                        } else {
                            px(0.0)
                        },
                    ))
            })
            .boxed()
        };
    let mut lines = line
        .rails
        .iter()
        .enumerate()
        .filter(|(_, on)| **on)
        .map(|(level, _)| over(guide(level), 0.0, 1.0, None))
        .collect::<Vec<_>>();
    // Stops short of its own chevron, or of the row's edge.
    let start = f32::from(line.depth) * STEP_IN;
    let end = match line.fold {
        Some(_) => start - GAP,
        None => start + CHEVRON - GAP,
    };
    let x = guide(parent);
    lines.push(over(x, 0.0, 1.0, Some(elbow + 1.0)));
    lines.push(over(x, elbow, end - x, Some(1.0)));
    if !line.last {
        lines.push(over(x, elbow, 1.0, None));
    }
    lines
}

/// What a group's header names, in plain words.
fn group_meaning(group: Group) -> Option<&'static str> {
    match group {
        Group::Needs => Some(words::NEEDS),
        Group::Hamlet(_) => Some(words::HAMLET),
        Group::City => Some(words::CITY),
        Group::Harbor => Some(words::HARBOR),
        Group::Left => Some(words::LEFT),
        Group::Top | Group::Parent(_) | Group::Workers(_) => None,
    }
}

/// What a row's tooltip says, a line each.
fn tip_of(line: &Line) -> Option<String> {
    let meaning = match line.target {
        Target::Realm => Some(words::REALM),
        Target::Library => Some(words::LIBRARY),
        Target::Hand => Some(words::HAND),
        Target::Agent(_) | Target::Gone(_) => match line.glyph {
            Glyph::Lead => Some(words::LEAD),
            Glyph::Worker => Some(words::WORKER),
            _ => Some(words::AGENT),
        },
        Target::Fold(Group::Workers(_)) => Some(words::WORKER),
        Target::Profile(_) => Some(words::PROFILE),
        Target::Hamlet(_) | Target::Fold(_) => None,
    };
    let name = (line.name.chars().count() > NAME_IN_TIP)
        .then(|| line.name.clone());
    let address = (!line.address.is_empty()
        && line.address != line.name
        && line.target != Target::Hand)
        .then(|| line.address.clone());
    let detail =
        (!line.detail.is_empty()).then(|| line.detail.clone());
    let lines = [name, address, detail, meaning.map(str::to_string)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// A section's header in `hub`'s rail, pressed to open its page where
/// it has one.
fn header(hub: Entity, section: &Section, above: f32) -> AnyView {
    let group = section.group;
    let fold = match group {
        Group::Needs => {
            frame().width(px(CHEVRON)).shrink(0.0).boxed()
        }
        _ => chevron(hub, group, section.folded, "Fold or open"),
    };
    let tone = if section.current {
        Tone::Accent
    } else if group == Group::Left {
        Tone::Dim
    } else {
        Tone::Faint
    };
    let hamlet = matches!(group, Group::Hamlet(_));
    let title = section.title.clone();
    let mut words = vec![
        with_cx(move |cx| {
            label(title.clone())
                .size(cx.theme().text.heading)
                .bold(hamlet)
                .tone(tone)
                .wrap(false)
        })
        .boxed(),
    ];
    if hamlet {
        words.push(frame().grow(1.0).boxed());
        words.push(mark_icon(section.mark));
    } else {
        words.push(
            label(section.count.to_string()).tone(Tone::Dim).boxed(),
        );
    }
    let words = row(words).gap(6.0).align(AlignItems::Center);
    let words = match group_meaning(group) {
        Some(meaning) => {
            row((words.tooltip(move || note(meaning)).boxed(),))
        }
        None => row((words.boxed(),)),
    };
    match section.opens {
        // Outside the chevron's press, so folding does not open the
        // page.
        Some(target) => row((
            fold,
            washed(
                {
                    let pressed = button(words.grow(1.0))
                        .grow(1.0)
                        .min_width(px(0.0))
                        .rules(row_rules)
                        .tagged(Variant::Default)
                        .on_activate(move |world| {
                            go(world, hub, target);
                        });
                    match target.thing() {
                        Some(thing) => {
                            pressed.tagged(Asks(thing)).boxed()
                        }
                        None => pressed.boxed(),
                    }
                },
                section.current
                    && matches!(section.group, Group::Hamlet(_)),
            ),
        ))
        .gap(0.0)
        .align(AlignItems::Center)
        .padding(UiRect::top(px((above - ROW_DOWN).max(0.0))))
        .width(percent(100.0))
        .tagged(RailRow { hub, target })
        .boxed(),
        None => row((fold, words))
            .gap(6.0)
            .align(AlignItems::Center)
            .padding(UiRect::new(
                px(0.0),
                px(ROW_INSET),
                px(above),
                px(ROW_DOWN),
            ))
            .width(percent(100.0))
            .boxed(),
    }
}

/// The rail's sections in its three places.
#[derive(Component, Default, PartialEq)]
struct Parts {
    /// The realm's own and what waits, over the tabs.
    over: Vec<Section>,
    /// What the tab chooses, under the search.
    under: Vec<Section>,
    /// Those that left, at the foot of the rail.
    left: Vec<Section>,
}

/// Which of [`Parts`]' places a part of the rail is in.
type Place = fn(&Parts) -> &[Section];

/// The rails whose [`sections`] may have changed: every one, or those
/// of these Hubs.
#[derive(Resource, Default)]
struct Stale {
    all: bool,
    hubs: Vec<Entity>,
}

impl Stale {
    fn is_any(stale: Res<Stale>) -> bool {
        stale.all || !stale.hubs.is_empty()
    }
}

/// Notes what [`sections`] reads that changed since this last ran:
/// the realm's for every rail, a Hub's own for its rail alone.
fn note_stale(
    seen: Option<Res<Seen>>,
    hubs: Query<
        Entity,
        (
            With<super::Hub>,
            Or<(
                Changed<Rail>,
                Changed<Tab>,
                Changed<Showing>,
                Changed<Convo>,
            )>,
        ),
    >,
    transcripts: Query<(), Changed<Transcript>>,
    mut unwatched: RemovedComponents<Transcript>,
    mut stale: ResMut<Stale>,
) {
    // Read through each time, so that a removal is seen only once.
    let unwatched = unwatched.read().count() > 0;
    let changed = |is: Option<bool>| is.unwrap_or(false);
    if unwatched
        || !transcripts.is_empty()
        || changed(seen.map(|seen| seen.is_changed()))
    {
        stale.all = true;
    }
    stale.hubs.extend(hubs.iter());
}

/// Keeps the [`Parts`] of each Hub whose rail is stale up to date,
/// changed only when a section is.
fn gather(world: &World, mut commands: Commands) {
    let Some(mut hubs) =
        world.try_query_filtered::<Entity, With<super::Hub>>()
    else {
        return;
    };
    let stale = world.resource::<Stale>();
    for hub in hubs.iter(world) {
        if !stale.all && !stale.hubs.contains(&hub) {
            continue;
        }
        // The realm's page lists what waits in full.
        let on_realm =
            super::showing(world, hub) == Some(Showing::Realm);
        let mut parts = Parts::default();
        for section in sections(world, hub) {
            match section.group {
                Group::Top => parts.over.push(section),
                Group::Needs if !on_realm => parts.over.push(section),
                Group::Needs => {}
                Group::Left => parts.left.push(section),
                _ => parts.under.push(section),
            }
        }
        if world.get::<Parts>(hub) != Some(&parts) {
            commands.entity(hub).try_insert(parts);
        }
    }
    commands.queue(|world: &mut World| {
        *world.resource_mut::<Stale>() = Stale::default();
    });
}

/// `hub`'s [`Parts`], read through `read`.
fn parts<T>(
    hub: Entity,
    read: impl Fn(&Parts) -> T + Send + Sync + 'static,
) -> Signal<T>
where
    T: Default,
{
    component::<Parts, _>(hub, move |parts| {
        parts.map(&read).unwrap_or_default()
    })
}

/// The sections of `place` in `hub`'s rail, the first header
/// `first_above` down.
fn sections_view(
    hub: Entity,
    place: Place,
    first_above: f32,
) -> impl View {
    each(
        parts(hub, move |parts| {
            place(parts)
                .iter()
                .enumerate()
                .map(|(at, section)| {
                    // Outside the block, so a highlight starts at the
                    // header.
                    let above = match (section.title.is_empty(), at) {
                        (true, _) => 0.0,
                        (false, 0) => first_above - BLOCK_PAD,
                        (false, _) => PART_GAP - BLOCK_PAD,
                    };
                    (section.group, above.max(0.0))
                })
                .collect::<Vec<_>>()
        }),
        |part: &(Group, f32)| *part,
        move |&(group, above)| section_view(hub, place, group, above),
    )
    .within(hamlet_ui::styles::bare())
}

/// The section `group` of `place` in `hub`'s rail: its header and its
/// rows.
fn section_view(
    hub: Entity,
    place: Place,
    group: Group,
    above: f32,
) -> AnyView {
    let find = move |parts: &Parts| {
        place(parts)
            .iter()
            .find(|section| section.group == group)
            .cloned()
    };
    let head = keyed(
        parts(hub, move |parts| {
            find(parts).map(|section| Section {
                title: section.title,
                lines: Vec::new(),
                ..section
            })
        }),
        move |head| match head {
            Some(head) if !head.title.is_empty() => {
                header(hub, head, BLOCK_PAD)
            }
            _ => column(()).boxed(),
        },
    )
    .within(hamlet_ui::styles::bare());
    let dimmed = group == Group::Left;
    let rows = each(
        parts(hub, move |parts| {
            find(parts)
                .filter(|section| !section.folded)
                .map_or_else(Vec::new, |section| section.lines)
        }),
        Line::clone,
        move |line| expanded(hub, line, dimmed),
    )
    .within(hamlet_ui::styles::bare());
    with_cx(move |cx| {
        column((head, rows))
            .gap(0.0)
            .margin(UiRect::top(px(above)))
            .radius(cx.theme().space.round)
    })
    .boxed()
}

/// The rail down `hub`'s left side, or a strip of its rows' icons
/// while it is collapsed.
pub(super) fn view(hub: Entity) -> impl View {
    keyed(
        component::<Collapsed, _>(hub, |collapsed| {
            collapsed.is_some()
        }),
        move |collapsed| match collapsed {
            true => strip(hub).boxed(),
            false => full(hub).boxed(),
        },
    )
    .within(row(()).gap(0.0).shrink(0.0).height(percent(100.0)))
}

/// The rail with every row, its tools over them.
fn full(hub: Entity) -> impl View {
    let body = column((
        sections_view(hub, |parts| &parts.over, PART_GAP),
        column((tabs_and_search(hub),))
            .padding(UiRect::top(px(PART_GAP))),
        column((
            sections_view(hub, |parts| &parts.under, UNDER_SEARCH),
            frame().grow(1.0),
            sections_view(hub, |parts| &parts.left, PART_GAP),
        ))
        .gap(0.0)
        .grow(1.0),
    ))
    .gap(0.0)
    .grow(1.0);
    let tools =
        row((frame().grow(1.0), follow_toggle(hub), fold(hub)))
            .gap(2.0)
            .align(AlignItems::Center);
    with_cx(move |cx| {
        column((tools, scroll_area((body,))))
            .gap(cx.theme().space.gap)
            .width(component::<Width, _>(hub, |width| {
                px(width.map_or(WIDE, |width| width.0))
            }))
            .min_width(px(NARROWEST))
            .shrink(0.0)
            .height(percent(100.0))
            .min_height(px(0.0))
            .tagged(Body)
    })
}

/// The row a hamlet's header stands as in the collapsed rail, which
/// opens its page.
fn header_line(section: &Section) -> Option<Line> {
    let target @ Target::Hamlet(_) = section.opens? else {
        return None;
    };
    Some(Line {
        target,
        glyph: Glyph::Hamlet,
        name: section.title.clone(),
        tag: None,
        detail: String::new(),
        since: 0,
        mark: section.mark,
        address: String::new(),
        depth: 0,
        level: 0,
        last: true,
        rails: Vec::new(),
        current: section.current,
        fold: None,
        worker: false,
    })
}

/// The rows of `section` in the collapsed rail: its header's, then
/// those not folded away. A folded group stays while one in it waits.
fn strip_lines(section: &Section) -> Vec<Line> {
    let mut lines = Vec::from_iter(header_line(section));
    if !section.folded {
        lines.extend(
            section
                .lines
                .iter()
                .filter(|line| {
                    !matches!(line.target, Target::Fold(_))
                        || line.mark == Some(Standing::Waits)
                })
                .cloned(),
        );
    }
    lines
}

/// The collapsed rail: its tools over an icon for each row.
fn strip(hub: Entity) -> impl View {
    let rows = each(
        parts(hub, |parts| {
            parts
                .over
                .iter()
                .chain(&parts.under)
                .chain(&parts.left)
                .filter(|section| section.group != Group::Needs)
                .flat_map(strip_lines)
                .collect::<Vec<_>>()
        }),
        Line::clone,
        move |line| strip_row(hub, line),
    )
    .within(column(()).gap(2.0).align(AlignItems::Center));
    with_cx(move |cx| {
        column((
            column((fold(hub), follow_toggle(hub)))
                .gap(2.0)
                .align(AlignItems::Center),
            scroll_area((rows,)),
        ))
        .gap(cx.theme().space.gap)
        .width(px(STRIP))
        .shrink(0.0)
        .height(percent(100.0))
        .min_height(px(0.0))
    })
}

/// A row of the collapsed rail: its icon, or its mark while it waits.
fn strip_row(hub: Entity, line: &Line) -> AnyView {
    let target = line.target;
    let icon = match line.mark {
        Some(standing @ Standing::Waits) => status_icon_at(
            standing.glyph(),
            standing.word(),
            standing.kind(),
            ICON,
        )
        .boxed(),
        _ => glyph(line.glyph, ICON)
            .tone(if line.current {
                Tone::Accent
            } else {
                Tone::Dim
            })
            .boxed(),
    };
    let name = line.name.clone();
    let pressed = button(icon)
        .padding(UiRect::all(px(4.0)))
        .rules(views::tint)
        .tooltip(move || note(name.clone()))
        .on_activate(move |world| go(world, hub, target));
    let pressed = match target.thing() {
        Some(thing) => pressed.tagged(Asks(thing)).boxed(),
        None => pressed.boxed(),
    };
    column((pressed,)).tagged(RailRow { hub, target }).boxed()
}

/// The press that collapses `hub`'s rail, or expands it.
fn fold(hub: Entity) -> AnyView {
    keyed(
        component::<Collapsed, _>(hub, |collapsed| {
            collapsed.is_some()
        }),
        move |collapsed| {
            let says = match collapsed {
                true => "Expand the rail",
                false => "Collapse the rail",
            };
            tool(Glyph::Sidebar, says, move |world| {
                let mut hub = world.entity_mut(hub);
                if hub.contains::<Collapsed>() {
                    hub.remove::<Collapsed>();
                } else {
                    hub.insert(Collapsed);
                }
            })
        },
    )
    .within(hamlet_ui::styles::bare())
    .boxed()
}

/// The press that makes `hub` follow the world's selection, lit on
/// the Hub that does.
fn follow_toggle(hub: Entity) -> AnyView {
    keyed(
        component::<super::Following, _>(hub, |following| {
            following.is_some()
        }),
        move |following| {
            let (tone, says) = match following {
                true => {
                    (Tone::Accent, "Follows the world's selection")
                }
                false => {
                    (Tone::Faint, "Follow the world's selection")
                }
            };
            button(glyph(Glyph::Aim, ICON).tone(tone))
                .padding(UiRect::all(px(4.0)))
                .rules(views::tint)
                .tooltip(move || note(says))
                .on_activate(move |world| super::follow(world, hub))
                .boxed()
        },
    )
    .within(hamlet_ui::styles::bare())
    .boxed()
}

#[cfg(test)]
mod tests {
    use hamlet_client::realm::named;

    use super::*;
    use crate::testing::panel_app;

    fn names(
        app: &App,
        hub: Entity,
        group: fn(&Group) -> bool,
    ) -> Vec<String> {
        sections(app.world(), hub)
            .into_iter()
            .filter(|section| group(&section.group))
            .flat_map(|section| section.lines)
            .map(|line| line.name)
            .collect()
    }

    #[test]
    fn an_agent_keeps_its_place_in_its_hamlet_and_what_waits_is_on_top()
     {
        let mut app = panel_app();
        app.add_plugins(plugin);
        let hub = crate::testing::hub(&mut app);
        app.update();
        let ada = named(app.world_mut(), "agent#2").unwrap();
        let in_hamlet = |g: &Group| matches!(g, Group::Hamlet(_));
        assert_eq!(names(&app, hub, in_hamlet), ["ada"]);
        assert!(
            names(&app, hub, |g| *g == Group::Top)
                .contains(&"The Hand".into())
        );

        // Working does not move it.
        app.world_mut().entity_mut(ada).insert(Busy);
        assert_eq!(names(&app, hub, in_hamlet), ["ada"]);

        let mut state = hamlet_testing::realm::state();
        state.waits.push(hamlet_testing::realm::wait("ask"));
        state.waits.push(hamlet_testing::realm::wait("again"));
        state.revision += 1;
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert_eq!(names(&app, hub, |g| *g == Group::Needs), ["ada"]);
        // One row for who asks, counted as every wait, as the
        // realm's page counts them.
        let needs = sections(app.world(), hub)
            .into_iter()
            .find(|section| section.group == Group::Needs)
            .unwrap();
        assert_eq!(needs.count, 2);
        assert_eq!(names(&app, hub, in_hamlet), ["ada"]);
    }

    #[test]
    fn a_hub_opened_later_gets_its_rows() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        crate::testing::hub(&mut app);
        app.update();
        let second = crate::hub::spawn(
            app.world_mut(),
            "hub-2".into(),
            RailState::Expanded,
        );
        app.update();
        let parts = app.world().get::<Parts>(second).unwrap();
        assert!(!parts.over.is_empty());
    }
}
