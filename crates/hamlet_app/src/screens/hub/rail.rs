//! The Hub's rail: the realm as its chain of command. The realm and
//! the Hand first, then what waits for the chief, then each hamlet as
//! the tree of its hires under its mayor, then the city's scholars,
//! the library and those that left. A row keeps its place as its
//! agent works and rests; only its mark changes.

use std::collections::{HashMap, HashSet};

use bevy::app::{App, PostUpdate, Update};
use bevy::color::Color;
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Has, With};
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
    Dragging, FynixSystems, OverrideCursor, ScopedExt, ViewExt,
    derived, each, every_frame, keyed, resource, style,
};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::realm::{
    Activity, Busy, HamletId, Identity, Kind, Left, Lived, LivesIn,
    Mayor, MergedInto, Parent, Purpose, Scholar, Seen, Shown, Skills,
    name_of, shown_of,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing, bring_into_view, select};
use hamlet_ui::styles::{
    ICON, STATUS_PAD, note, note_rules, scroll_area, status_icon_at,
    text_input, transparent,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::{Edge, Variant, Wash};
use hamlet_ui::widgets::ellipsis::{cut, one_line};
use hamlet_ui::widgets::hint::search;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::widgets::tabs::{self, Spread};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::{Showing, show};
use crate::age::Age;
use crate::convo::{Convo, talk_to, talk_to_hand};
use crate::scholar_tag::{self, ScholarTag};
use crate::standing::{Standing, need_you};
use crate::status::now_ms;
use crate::words;
use crate::world_menu::Asks;

/// The width of the rail at first, the least it is dragged to, so a
/// row keeps its name, and the most.
const WIDE: f32 = 248.0;
const NARROWEST: f32 = 200.0;
const WIDEST: f32 = 520.0;
/// The least the rail leaves the page beside it.
const PAGE: f32 = 280.0;

/// The most that left a rail shows.
const LEFT_SHOWN: usize = 6;

/// The most letters of the words a row's tooltip quotes.
const LINE_SHOWN: usize = 90;

/// The most letters of a name its row's tooltip leaves out; a longer
/// one may be cut at the rail's edge, so the tooltip has it whole.
const NAME_IN_TIP: usize = 24;

/// The least height of a row.
const ROW: f32 = 28.0;

/// The height of a worker's row and the side of its icon, smaller
/// than a hire's, so the hires stand out of their workers.
const WORKER_ROW: f32 = 24.0;
const WORKER_ICON: f32 = 12.0;

/// How opaque the icon of one that left is.
const DIMMED: f32 = 0.6;

/// The column at the rail's edge that holds every chevron.
const CHEVRON: f32 = 14.0;

/// How far each step down a tree is set in: a chevron's room and a
/// little more, so a guide under a chevron leads to the next one.
const STEP_IN: f32 = 18.0;

/// The deepest a row is set in; deeper rows line up with it.
const DEEPEST: u8 = 6;

/// The room between an elbow's end and the row it leads to.
const GAP: f32 = 4.0;

/// The room inside a row's ends, and over and under its words.
const ROW_INSET: f32 = 8.0;
const ROW_DOWN: f32 = 6.0;

/// The room over a section's header inside its block.
const BLOCK_PAD: f32 = 4.0;

/// The space over each part of the rail: what waits, the tabs and
/// each hamlet, so that the parts stand apart and evenly.
const PART_GAP: f32 = 16.0;

/// The space over the first part under the tabs, less than
/// [`PART_GAP`] by what the search row already leaves under it.
const UNDER_SEARCH: f32 = 12.0;

/// The frames a row asked to be brought into view is waited for, as
/// the folds over it open and it is built.
const REVEAL_FRAMES: u8 = 10;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Rail>()
        .init_resource::<Width>()
        .init_resource::<Tab>()
        .init_resource::<Reveal>()
        .init_resource::<Parts>()
        .init_resource::<Minute>()
        .add_observer(grab)
        .add_observer(drag)
        .add_observer(drop)
        .add_systems(Update, (read_search, tabs::mark::<Tab>, tick))
        .add_systems(
            Update,
            gather
                .run_if(is_stale)
                .after(read_search)
                .before(FynixSystems::Mount),
        )
        .add_systems(PostUpdate, reveal.after(UiSystems::Layout));
}

/// The minute it is, counted from 1970, on which the rows' ages are
/// read again.
#[derive(Resource, Default, PartialEq)]
struct Minute(u64);

fn tick(mut minute: ResMut<Minute>) {
    minute.set_if_neq(Minute(now_ms() / 60_000));
}

/// The row to bring into view once it is laid out, and the frames
/// left to wait for it.
#[derive(Resource, Default)]
struct Reveal(Option<(Target, u8)>);

/// On a row of the rail, or a header that opens a page: what it
/// leads to.
#[derive(Component, Clone, Copy)]
struct RailRow(Target);

/// Turns the rail to the row of `thing`, chosen in the world: the tab
/// that lists it, the folds over it opened and the row brought into
/// view, unless it is in view already.
pub(super) fn follow(world: &mut World, thing: Thing) {
    let Some(target) = target_of(world, thing) else {
        return;
    };
    let tab = match target {
        Target::Agent(_) | Target::Gone(_) | Target::Hamlet(_) => {
            Some(Tab::Hamlets)
        }
        Target::Scholar(_) => Some(Tab::City),
        Target::Realm
        | Target::Hand
        | Target::Library
        | Target::Fold(_) => None,
    };
    if let Some(tab) = tab
        && let Some(mut now) = world.get_resource_mut::<Tab>()
        && *now != tab
    {
        *now = tab;
    }
    if let Target::Agent(agent) = target {
        open_over(world, agent);
    }
    let holds = |section: &Section| {
        section.opens == Some(target)
            || section.lines.iter().any(|line| line.target == target)
    };
    let shut = sections(world)
        .into_iter()
        .find(|section| section.folded && holds(section))
        .map(|section| section.group);
    let mut rail = world.resource_mut::<Rail>();
    if let Some(group) = shut {
        rail.folded.retain(|folded| *folded != group);
    }
    world.resource_mut::<Reveal>().0 = Some((target, REVEAL_FRAMES));
}

/// The row of the rail `thing` has, if it has one.
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
            agent(hamlet_client::realm::first_mayor(world)?)
        }
        Thing::Hand => Target::Hand,
        Thing::Common(hamlet) => Target::Hamlet(hamlet),
        Thing::Scholar(scholar) => Target::Scholar(scholar),
        Thing::Ruin(agent) => Target::Gone(agent),
        Thing::Library => Target::Library,
        _ => return None,
    })
}

/// Opens the leads and the groups of workers `agent` is folded
/// under.
fn open_over(world: &mut World, agent: Entity) {
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
    let mut rail = world.resource_mut::<Rail>();
    for (parent, worker) in over {
        match worker {
            true => {
                let group = Group::Workers(parent);
                if !rail.opened.contains(&group) {
                    rail.opened.push(group);
                }
            }
            false => {
                rail.folded.retain(|g| *g != Group::Lead(parent));
            }
        }
    }
}

/// Scrolls the rail as little as shows the row asked for, once it is
/// laid out. A row in view already leaves it where it is.
fn reveal(
    mut asked: ResMut<Reveal>,
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
    let Some((target, left)) = asked.0 else {
        return;
    };
    let laid = rows.iter().find(|(_, row, node, _)| {
        row.0 == target && node.size().y > 0.0
    });
    let Some((row, _, node, place)) = laid else {
        asked.0 = left.checked_sub(1).map(|left| (target, left));
        return;
    };
    asked.0 = None;
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

/// The rail's search and what is folded.
#[derive(Resource, PartialEq, Default)]
pub struct Rail {
    search: String,
    /// The parts folded to their header. A hamlet with something
    /// waiting opens whatever this says.
    folded: Vec<Group>,
    /// The groups of workers opened; each is folded until it is.
    opened: Vec<Group>,
    /// The hamlets left out of it; one with something waiting is
    /// shown whatever this says.
    hidden: Vec<Entity>,
    /// Whether the list of hamlets to show is open.
    filtering: bool,
}

/// How wide the chief dragged the rail.
#[derive(Resource)]
struct Width(f32);

impl Default for Width {
    fn default() -> Self {
        Self(WIDE)
    }
}

/// The part of the realm the rail lists under the realm's own rows
/// and what waits, one at a time.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// The hamlets, each the tree under its mayor, and the hires
    /// that left.
    #[default]
    Hamlets,
    /// The scholars, those at the harbor and those that left.
    City,
}

impl Rail {
    /// Folds `group` to its header, or opens it.
    fn toggle(&mut self, group: Group) {
        // Workers are kept in what is opened, as they start folded.
        let kept = match group {
            Group::Workers(_) => &mut self.opened,
            _ => &mut self.folded,
        };
        match kept.iter().position(|g| *g == group) {
            Some(at) => {
                kept.remove(at);
            }
            None => kept.push(group),
        }
    }
}

/// The root of the Hub, whose width bounds the rail's.
#[derive(Component)]
pub(super) struct Frame;

/// The line between the rail and the page, dragged to size the rail.
#[derive(Component)]
pub(super) struct Divider;

/// The rail itself, as laid out.
#[derive(Component)]
struct Body;

/// The line between the rail and the page: dragged, it sizes the
/// rail, between [`NARROWEST`] and [`WIDEST`].
pub(super) fn divider() -> impl View {
    views::divider(Axis::Vertical)
        .rules(revealed_line)
        .tagged(Divider)
}

/// The divider is a hairline, in the accent while held or hovered.
fn revealed_line(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.fill(t.palette.gold_6))
        .hovered(|s| s.fill(|t: &HamletTheme| t.palette.gold_4))
        .when::<Dragging>(|s| {
            s.fill(|t: &HamletTheme| t.palette.gold_4)
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

/// Sizes the rail so that the line is where the pointer is, however
/// fast it moves. Every desk has a Hub of its own, so the rail
/// measured is the one beside the line dragged.
fn drag(
    drag: On<PointerDrag>,
    dividers: Query<&UiGlobalTransform, With<Divider>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Frame>>,
    bodies: Query<(&ComputedNode, &UiGlobalTransform), With<Body>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut width: ResMut<Width>,
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
    let Ok((node, place)) = frames.get(frame) else {
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

/// The root of the search field.
#[derive(Component)]
struct Search;

fn read_search(
    root: Query<&Children, With<Search>>,
    texts: Query<&EditableText>,
    mut rail: ResMut<Rail>,
) {
    let Some(text) = root
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != rail.search.as_str() {
        rail.search = text.value().to_string();
    }
}

/// A part of the rail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Group {
    /// The realm and the Hand, with the Hand's own workers.
    Top,
    Needs,
    Hamlet(Entity),
    /// The hires under a lead, which fold into its row.
    Lead(Entity),
    /// The workers an agent started, or those a chief started in a
    /// hamlet, which fold into one row.
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
    Scholar(Entity),
    Library,
    Gone(Entity),
    /// The group it is folded or opened.
    Fold(Group),
}

impl Target {
    /// The thing whose menu a right click on its row opens, as in the
    /// world.
    fn thing(self) -> Option<Thing> {
        match self {
            Target::Hand => Some(Thing::Hand),
            Target::Agent(agent) => Some(Thing::Agent(agent)),
            Target::Hamlet(hamlet) => Some(Thing::Common(hamlet)),
            Target::Scholar(scholar) => Some(Thing::Scholar(scholar)),
            Target::Library => Some(Thing::Library),
            Target::Gone(agent) => Some(Thing::Ruin(agent)),
            Target::Realm | Target::Fold(_) => None,
        }
    }
}

/// A row of the rail: one line of its name, its tag, its age and its
/// mark.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Line {
    pub target: Target,
    pub glyph: Glyph,
    pub name: String,
    /// The scholar of the city its agent is of.
    pub tag: Option<ScholarTag>,
    /// What its tooltip says of it beside what it is, such as what
    /// its agent is doing or what it waits for.
    pub detail: String,
    /// When what its age counts from happened, in milliseconds; 0
    /// for none. The age is worked out as it is drawn, so the rows
    /// are not built anew each minute.
    pub since: u64,
    pub mark: Option<Standing>,
    /// What the search also finds it by: its address.
    pub address: String,
    /// How far under another agent it is set in.
    pub depth: u8,
    /// Whether no row after it is set in under the same agent.
    pub last: bool,
    /// For each level between the top and its own, whether the guide
    /// of the agent there carries on past this row.
    pub rails: Vec<bool>,
    pub current: bool,
    /// For a lead: the fold of the rows under it, and whether it is
    /// folded.
    pub fold: Option<(Group, bool)>,
    /// Whether it is a worker's, or the row its group folds into:
    /// shorter and lighter than a hire's, so the hires stand out of
    /// their workers.
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
    mayor: bool,
    mark: Option<Standing>,
    /// Its address, and what it goes by: its nickname, or its
    /// scholar's name.
    address: String,
    called: String,
    tag: Option<ScholarTag>,
    /// What it is doing, while it works.
    doing: String,
    since: u64,
}

/// How long ago `then` was at `now`, short, or nothing for a time
/// never set.
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

/// The last thing the chief said to the agent, and when the chat
/// was last added to.
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

/// What an agent with `address` is called in the rail: what it is
/// shown by, else its address.
fn called(address: &str, shown: Option<&Shown>) -> String {
    shown.map_or_else(|| address.to_string(), |shown| shown.0.clone())
}

/// What a row of the hamlet's page says under an agent's name: its
/// work while it works, else the chief's last words to it, else what
/// it was asked, and nothing where its mark already says the same.
/// Then when its chat was last added to.
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
        (Has<Mayor>, Has<Left>),
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
                (mayor, _),
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
                    mayor,
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
                    tag: ScholarTag::of(world, entity),
                    doing,
                    since: last.max(lived.since),
                }
            },
        )
        .collect()
}

/// A place in a tree: an agent, or the workers one started, which
/// fold into a row of their own under the hires beside them.
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

/// The hires and the workers under each agent, by its id, each by
/// name.
type Under<'a> =
    HashMap<protocol::AgentId, (Vec<&'a Agent>, Vec<&'a Agent>)>;

/// Who is under each of `some`, among `some`.
fn under<'a>(some: &[&'a Agent]) -> Under<'a> {
    let mut under = Under::new();
    for agent in some {
        let Some(parent) = agent.parent else {
            continue;
        };
        let (hires, workers) = under.entry(parent).or_default();
        match agent.is_worker() {
            true => workers.push(*agent),
            false => hires.push(*agent),
        }
    }
    for (hires, workers) in under.values_mut() {
        hires.sort_by_cached_key(|agent| agent.called.to_lowercase());
        workers
            .sort_by_cached_key(|agent| agent.called.to_lowercase());
    }
    under
}

/// `root` and everyone `under` it, each with its depth below `root`:
/// hires by name, then the workers each agent started in a group
/// under it.
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
        let Some((hires, workers)) = under.get(&agent.id) else {
            return;
        };
        let depth = depth.saturating_add(1);
        for child in hires {
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

/// The rows of `family`, each set in as deep as it is in the tree,
/// knowing whether it is the last under the agent above it and which
/// guides above it carry on past it. A group of workers is one row,
/// with a row for each of them under it while `open` says it is.
fn family_lines(
    world: &World,
    family: &[(Node<'_>, u8)],
    open: &dyn Fn(Group) -> bool,
) -> Vec<Line> {
    // Each row with the depth it has in the tree.
    let mut rows = Vec::<(Line, u8)>::new();
    for (node, depth) in family {
        let depth = *depth;
        match node {
            Node::Agent(agent) => {
                let line = agent_line(world, agent, depth, false);
                rows.push((line, depth));
            }
            Node::Workers { group, workers } => {
                let shown = open(*group);
                rows.push((
                    workers_line(*group, workers, shown, depth),
                    depth,
                ));
                if shown {
                    let under = depth.saturating_add(1);
                    for worker in workers {
                        let line =
                            agent_line(world, worker, under, false);
                        rows.push((line, under));
                    }
                }
            }
        }
    }
    // Whether each row is the last at its depth before a row set in
    // less, found from the end.
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
            // The guides of the levels between the top and its own.
            line.rails =
                more.get(1..level).unwrap_or_default().to_vec();
            more.resize(level + 1, false);
            more[level] = !last;
            line
        })
        .collect()
}

/// The row a group of `workers` folds into: how many there are, and
/// while it is folded the most pressing of their marks.
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
        current: false,
        fold: Some((group, !open)),
        worker: true,
    }
}

fn kind_glyph(agent: &Agent) -> Glyph {
    match agent.kind {
        protocol::AgentKind::Hand => Glyph::Tower,
        protocol::AgentKind::Hire if agent.mayor => Glyph::Mayor,
        protocol::AgentKind::Hire => Glyph::Hire,
        protocol::AgentKind::Worker => Glyph::Worker,
    }
}

/// Whether the Hub is turned to `target`.
fn is_current(world: &World, target: Target) -> bool {
    let showing = world.get_resource::<Showing>().copied();
    let seated = world.get_resource::<Convo>().map(|c| c.agent);
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
        (Target::Scholar(a), Some(Showing::Scholar(b)))
        | (Target::Gone(a), Some(Showing::Gone(b)))
        | (Target::Hamlet(a), Some(Showing::Hamlet(b))) => a == b,
        (Target::Library, Some(Showing::Library))
        | (Target::Realm, Some(Showing::Realm)) => true,
        _ => false,
    }
}

fn agent_line(
    world: &World,
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
        current: is_current(world, target),
        fold: None,
        worker: agent.kind == protocol::AgentKind::Worker,
    }
}

/// Lets each lead in `lines` stand for the rows set in under it:
/// while folded they are left out and its mark becomes the most
/// pressing of theirs. What waits under it is still in the rows of
/// what needs the chief.
fn fold_leads(lines: Vec<Line>, folded: &[Group]) -> Vec<Line> {
    let mut lines = lines;
    // From the end, the rows not yet under a row before them, each
    // with the most pressing mark of it and the rows under it.
    let mut after = Vec::<(u8, Standing)>::new();
    for at in (0..lines.len()).rev() {
        let depth = lines[at].depth;
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
        let group = Group::Lead(agent);
        let shut = folded.contains(&group);
        let line = &mut lines[at];
        line.fold = Some((group, shut));
        // Open, each row shows its own state; folded, the lead
        // stands for all of them.
        if shut {
            line.mark = Some(mark);
        }
    }
    // Leave out what folded leads hold.
    let mut kept = Vec::with_capacity(lines.len());
    let mut hidden_below: Option<u8> = None;
    for line in lines {
        if let Some(depth) = hidden_below {
            if line.depth > depth {
                continue;
            }
            hidden_below = None;
        }
        if matches!(line.fold, Some((_, true))) {
            hidden_below = Some(line.depth);
        }
        kept.push(line);
    }
    kept
}

/// A row that leads to a page.
fn page_line(
    world: &World,
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
        current: is_current(world, target),
        fold: None,
        worker: false,
    }
}

/// Whether `line` matches what is typed in the search: its name, its
/// address or its scholar's name.
fn matches(line: &Line, search: &str) -> bool {
    search.is_empty()
        || line.name.to_lowercase().contains(search)
        || line.address.to_lowercase().contains(search)
        || line.tag.as_ref().is_some_and(|tag| {
            tag.name.to_lowercase().contains(search)
        })
}

/// What waits for the chief, one row for each who asks: how long its
/// oldest wait has waited, with its words and how many more in its
/// tooltip; and how many waits there are in all.
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
    // The row of each who asks, by its address.
    let mut rows = HashMap::<&str, usize>::new();
    let mut counts = Vec::<usize>::new();
    for wait in waits {
        // A newcomer is about the scholar at the harbor, not about
        // whoever made it, and leads to its page.
        if let protocol::WaitDetail::Newcomer { scholar } =
            &wait.detail
            && let Some(entity) =
                hamlet_client::realm::scholar(world, *scholar)
        {
            let name = world
                .get::<Scholar>(entity)
                .map(|s| s.0.name.clone())
                .unwrap_or_default();
            lines.push(Line {
                target: Target::Scholar(entity),
                glyph: Glyph::Scholar,
                name: format!("{name} · harbor"),
                tag: None,
                detail: "Wants to move into the city".into(),
                since: wait.since,
                mark: Some(Standing::Waits),
                address: name,
                depth: 0,
                last: true,
                rails: Vec::new(),
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
            // What it goes by; its address, hamlet and all, is the
            // row's tooltip.
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

/// `text` with its first letter a capital, as every row's line
/// starts.
fn capitalised(text: &str) -> String {
    let mut letters = text.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// The city's scholars, the harbor, the library and those that left.
fn society(world: &World, sections: &mut Vec<Section>) {
    let mut city = Vec::new();
    let mut harbor = Vec::new();
    let mut gone = Vec::new();
    // A newcomer waits for the chief only while the chief holds the
    // door; under the Hand or an open door nothing waits on them.
    let chief_holds_door = world
        .get_resource::<hamlet_client::realm::Settings>()
        .is_some_and(|s| s.0.newcomers == protocol::Newcomers::Chief);
    if let Some(mut query) = world.try_query::<(Entity, &Scholar)>() {
        let mut found =
            query.iter(world).collect::<Vec<(Entity, &Scholar)>>();
        found.sort_by_key(|(_, scholar)| {
            scholar.0.name.to_lowercase()
        });
        for (entity, scholar) in found {
            let scholar = &scholar.0;
            // Where it is hired, by hamlet.
            let mut hamlets = scholar
                .hires
                .iter()
                .filter_map(|hire| {
                    hamlet_client::realm::agent(world, *hire)
                })
                .filter(|hire| world.get::<Left>(*hire).is_none())
                .filter_map(|hire| world.get::<LivesIn>(hire))
                .map(|lives| name_of(world, lives.0))
                .collect::<Vec<_>>();
            hamlets.sort();
            hamlets.dedup();
            let (title, mark) = match (&scholar.left, scholar.arrival)
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
                    ("Not hired".into(), None)
                }
                (None, protocol::Arrival::Housed) => {
                    (format!("In {}", hamlets.join(", ")), None)
                }
                // A blank hire's is not of the city: the hire is in
                // its hamlet's tree.
                (None, protocol::Arrival::Apart) => continue,
            };
            let line = page_line(
                world,
                Target::Scholar(entity),
                Glyph::Scholar,
                &scholar.name,
                title,
                mark,
            );
            match (&scholar.left, scholar.arrival) {
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
                Target::Gone(entity),
                Glyph::Hire,
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

/// The rail's rows in their groups. A group with no row is left out;
/// while searching every group is open and keeps only what matches.
pub(super) fn sections(world: &World) -> Vec<Section> {
    let (search, folded, opened, hidden) =
        world.get_resource::<Rail>().map_or_else(
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
    let tab =
        world.get_resource::<Tab>().copied().unwrap_or_default();
    let all = agents(world);
    let mut sections = Vec::new();

    // The realm, with how many wait for the chief in it, and the
    // Hand with what it started outside any hamlet.
    let waiting = world
        .try_query::<&Waiting>()
        .map_or(0, |mut waits| waits.iter(world).count());
    let mut top = vec![page_line(
        world,
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
        top.extend(family_lines(world, &family, &open));
    }
    // The library is the realm's, beside the Hand.
    let shelf =
        world.get_resource::<Skills>().map_or(0, |s| s.0.len());
    top.push(page_line(
        world,
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

    // Each hamlet as the tree under its mayor.
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
        roots.sort_by_key(|agent| {
            (!agent.mayor, agent.called.clone())
        });
        // The workers a chief started in the hamlet answer to no one
        // in it, and fold into one row after its tree.
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
        let mut lines = family_lines(world, &family, &open);
        let group = Group::Hamlet(hamlet);
        let mark = Some(Standing::of_all(
            lines.iter().filter_map(|line| line.mark),
        ));
        // A search finds rows a lead would fold away.
        if search.is_empty() {
            lines = fold_leads(lines, &folded);
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
            current: is_current(world, Target::Hamlet(hamlet)),
        });
    }

    society(world, &mut sections);
    for section in &mut sections {
        // Every part with a header folds, but never over a wait.
        if matches!(
            section.group,
            Group::City | Group::Harbor | Group::Left
        ) {
            section.folded = folded.contains(&section.group)
                && !section
                    .lines
                    .iter()
                    .any(|l| l.mark == Some(Standing::Waits));
        }
        if !search.is_empty() {
            section.folded = false;
            section.lines.retain(|line| matches(line, &search));
        }
        // Of those that left, the tab's own: hires on the hamlets',
        // scholars on the city's.
        if section.group == Group::Left {
            section.lines.retain(|line| {
                matches!(line.target, Target::Scholar(_))
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
            | Group::Lead(_)
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

/// The tabs between the hamlets and the city.
const TABS: [(Tab, Glyph, &str); 2] = [
    (Tab::Hamlets, Glyph::Hamlet, "Hamlets"),
    (Tab::City, Glyph::Scholar, "City"),
];

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

/// The search over the rail's rows, with a word in it while it is
/// empty, and at its end the filter of the hamlets shown and a way to
/// make what the tab lists.
fn search_row() -> impl View {
    let field = search(
        text_field(String::new(), |_, _| {})
            .rules(text_input)
            .tagged(Search),
        "Search",
    );
    // Every tab's tools are built once and only shown on their tab,
    // so that a change of tab does not make them anew.
    let on = |shown: Tab, view: AnyView| {
        column((view,)).display(resource::<Tab, _>(move |tab| {
            if *tab == shown {
                Display::Flex
            } else {
                Display::None
            }
        }))
    };
    let tools = row((
        on(
            Tab::Hamlets,
            tool(
                Glyph::Sliders,
                "Choose which hamlets are shown",
                |w| {
                    let mut rail = w.resource_mut::<Rail>();
                    rail.filtering = !rail.filtering;
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
                "Make a scholar",
                crate::world_menu::ask_new_scholar,
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

/// A switch for each hamlet that shows or hides it, while the filter
/// is open on the hamlets' tab.
fn filter_list() -> impl View {
    keyed(
        derived(|world: &World| {
            let rail = world.resource::<Rail>();
            let open = rail.filtering
                && world.get_resource::<Tab>().copied()
                    == Some(Tab::Hamlets);
            let names = match open {
                true => hamlet_names(world),
                false => Vec::new(),
            };
            (open, rail.hidden.clone(), names)
        })
        .when(every_frame()),
        |(open, hidden, names)| {
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
                                let hidden = &mut world
                                    .resource_mut::<Rail>()
                                    .hidden;
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

/// The tabs between the hamlets and the city, in the style of the
/// panels' tabs, the search under them, and the hamlets to show.
fn tabs_and_search() -> impl View {
    with_cx(|cx| {
        column((
            tabs::tipped_strip(
                &TABS,
                &[words::HAMLET, words::CITY],
                Spread::Shared,
                |world, tab| {
                    *world.resource_mut::<Tab>() = tab;
                },
            ),
            search_row(),
            filter_list(),
        ))
        .gap(cx.theme().space.gap)
    })
}

/// What pressing a row does.
fn go(world: &mut World, target: Target) {
    match target {
        Target::Realm => show(world, Showing::Realm),
        Target::Hand => {
            talk_to_hand(world);
            type_in_thread(world);
        }
        // Its thread, and its house brought into view.
        Target::Agent(agent) => {
            talk_to(world, agent);
            type_in_thread(world);
            if world.contains_resource::<Selected>() {
                let home = crate::convo::home_of(world, agent);
                bring_into_view(world, home);
            }
        }
        Target::Hamlet(hamlet) => {
            show(world, Showing::Hamlet(hamlet));
            look_at(world, Thing::Common(hamlet));
        }
        // A scholar's house, or its place on the pier, and the
        // library are selected and brought into view in the world
        // too.
        Target::Scholar(scholar) => {
            show(world, Showing::Scholar(scholar));
            look_at(world, Thing::Scholar(scholar));
        }
        Target::Library => {
            show(world, Showing::Library);
            look_at(world, Thing::Library);
        }
        Target::Gone(agent) => show(world, Showing::Gone(agent)),
        Target::Fold(group) => {
            world.resource_mut::<Rail>().toggle(group);
        }
    }
}

/// Asks for the focus to go to the chat's field, so that what a chief
/// types after choosing an agent in the rail goes to it.
fn type_in_thread(world: &mut World) {
    if let Some(mut draft) =
        world.get_resource_mut::<crate::screens::chat::Draft>()
    {
        draft.focus = true;
    }
}

/// Chooses `thing` in the world and brings it into view, where there
/// is a world.
fn look_at(world: &mut World, thing: Thing) {
    if world.contains_resource::<Selected>() {
        select(world, Some(thing));
        bring_into_view(world, thing);
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

/// `row` on a frame that is washed with light while `held`, and less
/// under the pointer: the row's own gradient is its glow, which would
/// take the wash back.
fn washed(shown: impl View + 'static, held: bool) -> impl View {
    with_cx(move |cx| {
        row((shown,))
            .grow(1.0)
            .min_width(px(0.0))
            .radius(cx.theme().space.round)
            .tagged(Wash::new(held, Edge::Left))
    })
}

/// The lines in front of a nested row `height` tall, on a grid of
/// [`STEP_IN`]: `rails` for the levels above, each with the guide of
/// an agent that has more under it after this row, then the elbow
/// from its parent, cut short when it is `last`.
pub(crate) fn guides(
    rails: Vec<bool>,
    last: bool,
    height: f32,
) -> impl View {
    with_cx(move |cx| {
        let colour = cx.theme().palette.gold_6;
        let half = height / 2.0;
        // A column of the grid, with a guide down its middle where
        // `on` says, over its top and bottom halves.
        let column_of = |top: bool, bottom: bool, across: bool| {
            let bar = |on: bool| {
                frame().width(px(1.0)).height(px(half)).fill(if on {
                    colour
                } else {
                    Color::NONE
                })
            };
            row((
                frame().width(px(CHEVRON / 2.0)),
                column((bar(top), bar(bottom))).gap(0.0),
                column((frame()
                    .width(px(STEP_IN - CHEVRON / 2.0 - 1.0))
                    .height(px(1.0))
                    .fill(if across {
                        colour
                    } else {
                        Color::NONE
                    }),))
                .height(px(height))
                .justify(JustifyContent::Center),
            ))
            .gap(0.0)
            .boxed()
        };
        let mut parts = rails
            .iter()
            .map(|on| column_of(*on, *on, false))
            .collect::<Vec<_>>();
        parts.push(column_of(true, !last, true));
        row(parts).gap(0.0)
    })
}

/// The mark at the end of a row or a header, in its place whether or
/// not there is one, so that marks line up from row to row.
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

/// The chevron that folds `group`, pointing down while open and to
/// the right while folded, in the rail's one column of chevrons.
fn chevron(group: Group, shut: bool, says: &'static str) -> AnyView {
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
        world.resource_mut::<Rail>().toggle(group);
    })
    .boxed()
}

/// The least height of `line`'s row.
fn row_height(line: &Line) -> f32 {
    if line.worker { WORKER_ROW } else { ROW }
}

/// A row of one line: its icon with its scholar's badge, its name cut
/// at the rail's edge, how long ago and its mark at the end. What it
/// is doing or waits for is in its tooltip. A `dimmed` row, of those
/// that left, is set in grey.
fn expanded(line: &Line, dimmed: bool) -> AnyView {
    let (target, since, worker) =
        (line.target, line.since, line.worker);
    let name_tone = if line.current {
        Tone::Accent
    } else if dimmed || worker {
        Tone::Dim
    } else {
        Tone::Body
    };
    let name = column((one_line(
        label(line.name.clone())
            .tone(name_tone)
            .bold(!(worker || dimmed)),
    ),))
    .min_width(px(0.0))
    .overflow(Overflow::clip());
    let mut words = vec![name.boxed(), frame().grow(1.0).boxed()];
    words.push(
        label(resource::<Minute, _>(move |_| age(now_ms(), since)))
            .wrap(false)
            .rules(note_rules)
            .boxed(),
    );
    words.push(mark_icon(line.mark));
    // The scholar's badge sits on the icon's top-left corner.
    let mut icon = vec![
        glyph(line.glyph, if worker { WORKER_ICON } else { ICON })
            .tone(Tone::Dim)
            .opacity(if dimmed { DIMMED } else { 1.0 })
            .boxed(),
    ];
    icon.extend(line.tag.as_ref().map(scholar_tag::badge));
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
        .on_activate(move |world| go(world, target));
    // The right button opens its menu, as on the thing in the world.
    let pressed = match target.thing() {
        Some(thing) => pressed.tagged(Asks(thing)).boxed(),
        None => pressed.boxed(),
    };
    let shown = washed(pressed, line.current).boxed();
    let shown = match tip_of(line) {
        Some(tip) => shown.tooltip(move || note(tip.clone())).boxed(),
        None => shown.boxed(),
    };
    // The chevron of a lead, outside the row's press so that folding
    // does not open the thread, then the step in of a row under one.
    let fold = match line.fold {
        Some((group, shut)) => chevron(
            group,
            shut,
            match (group, shut) {
                (Group::Workers(_), true) => "Show the workers",
                (Group::Workers(_), false) => "Fold the workers",
                (_, true) => "Show the hires under it",
                (_, false) => "Fold the hires under it",
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
        .tagged(RailRow(target))
        .boxed()
}

/// The tree's lines over a row, on a grid of [`STEP_IN`]: for each
/// level above it, the guide of an agent that has more under it after
/// this row, then the elbow from its own lead, which runs on down
/// while more follow. Every guide runs under the middle of its
/// agent's chevron, so the tree lines up however deep it goes, and
/// an elbow stops short of the row it leads to, on the middle of its
/// icon.
fn tree_lines(line: &Line) -> Vec<AnyView> {
    if line.depth == 0 {
        return Vec::new();
    }
    // Where the guide of the agent at `level` runs.
    let guide =
        |level: usize| level as f32 * STEP_IN + CHEVRON / 2.0 - 0.5;
    let lead = usize::from(line.depth) - 1;
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
                    .fill(cx.theme().palette.gold_6)
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
    // The guides of the levels above that carry on past the row.
    let mut lines = line
        .rails
        .iter()
        .enumerate()
        .filter(|(_, on)| **on)
        .map(|(level, _)| over(guide(level), 0.0, 1.0, None))
        .collect::<Vec<_>>();
    // The elbow from its lead, stopping short of its own chevron, or
    // of the row's edge where it has no chevron, so it never runs
    // into the row.
    let start = f32::from(line.depth) * STEP_IN;
    let end = match line.fold {
        Some(_) => start - GAP,
        None => start + CHEVRON - GAP,
    };
    let x = guide(lead);
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
        Group::Top | Group::Lead(_) | Group::Workers(_) => None,
    }
}

/// What a row's tooltip says, a line each: a long name in full, the
/// address it goes by where that is not its name, what it is doing or
/// waits for, and what it is in plain words.
fn tip_of(line: &Line) -> Option<String> {
    let meaning = match line.target {
        Target::Realm => Some(words::REALM),
        Target::Library => Some(words::LIBRARY),
        Target::Hand => Some(words::HAND),
        Target::Agent(_) | Target::Gone(_) => match line.glyph {
            Glyph::Mayor => Some(words::MAYOR),
            Glyph::Worker => Some(words::WORKER),
            _ => Some(words::HIRE),
        },
        Target::Fold(Group::Workers(_)) => Some(words::WORKER),
        Target::Scholar(_) => Some(words::SCHOLAR),
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

/// A section's header, set apart from the rows under it in the
/// display face: a chevron that folds it, then its title, its count
/// and its state, pressed whole to open its page where it has one.
fn header(section: &Section, above: f32) -> AnyView {
    let group = section.group;
    let fold = match group {
        Group::Needs => {
            frame().width(px(CHEVRON)).shrink(0.0).boxed()
        }
        _ => chevron(group, section.folded, "Fold or open"),
    };
    // Those that left are headed in grey, not in the gold of what
    // can be pressed.
    let tone = if section.current {
        Tone::Accent
    } else if group == Group::Left {
        Tone::Dim
    } else {
        Tone::Faint
    };
    let mut words = vec![
        crate::overview::part_title(section.title.clone(), tone)
            .boxed(),
        label(section.count.to_string()).tone(Tone::Dim).boxed(),
    ];
    if matches!(group, Group::Hamlet(_)) {
        words.push(frame().grow(1.0).boxed());
        words.push(mark_icon(section.mark));
    }
    let words = row(words).gap(6.0).align(AlignItems::Center);
    let words = match group_meaning(group) {
        Some(meaning) => {
            row((words.tooltip(move || note(meaning)).boxed(),))
        }
        None => row((words.boxed(),)),
    };
    match section.opens {
        // Filled under the pointer as a row is, outside the chevron's
        // press so that folding does not open the page; its mark in
        // the column of the rows' marks.
        Some(target) => row((
            fold,
            washed(
                {
                    let pressed = button(words.grow(1.0))
                        .grow(1.0)
                        .min_width(px(0.0))
                        .rules(row_rules)
                        .tagged(Variant::Default)
                        .on_activate(move |world| go(world, target));
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
        .tagged(RailRow(target))
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

/// The rail's sections in its three places, worked out once an update
/// for the views to read.
#[derive(Resource, Default, PartialEq)]
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

/// Whether anything [`sections`] reads changed since this last ran:
/// the realm, a chat's transcript, the rail itself or what the Hub
/// is turned to.
fn is_stale(
    seen: Option<Res<Seen>>,
    rail: Res<Rail>,
    tab: Res<Tab>,
    showing: Option<Res<Showing>>,
    convo: Option<Res<Convo>>,
    transcripts: Query<(), Changed<Transcript>>,
    mut unwatched: RemovedComponents<Transcript>,
) -> bool {
    // Read through each time, so that a removal is seen only once.
    let unwatched = unwatched.read().count() > 0;
    let changed = |is: Option<bool>| is.unwrap_or(false);
    unwatched
        || !transcripts.is_empty()
        || changed(seen.map(|seen| seen.is_changed()))
        || rail.is_changed()
        || tab.is_changed()
        || changed(showing.map(|showing| showing.is_changed()))
        || changed(convo.map(|convo| convo.is_changed()))
}

/// Keeps [`Parts`] up to date, changed only when a section is.
fn gather(world: &World, mut commands: Commands) {
    // The realm's page lists what waits in full.
    let on_realm =
        world.get_resource::<Showing>() == Some(&Showing::Realm);
    let mut parts = Parts::default();
    for section in sections(world) {
        match section.group {
            Group::Top => parts.over.push(section),
            Group::Needs if !on_realm => parts.over.push(section),
            Group::Needs => {}
            Group::Left => parts.left.push(section),
            _ => parts.under.push(section),
        }
    }
    if world.get_resource::<Parts>() != Some(&parts) {
        commands.insert_resource(parts);
    }
}

/// The sections of `place`, the first header `first_above` under what
/// is over it. A section is built once and kept while it is in
/// `place`; within it, only a header or a row that changed is built
/// again.
fn sections_view(place: Place, first_above: f32) -> impl View {
    each(
        resource::<Parts, _>(move |parts| {
            place(parts)
                .iter()
                .enumerate()
                .map(|(at, section)| {
                    // The room over a header is kept outside its
                    // block, so a highlighted block starts at its
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
        move |&(group, above)| section_view(place, group, above),
    )
    .within(hamlet_ui::styles::bare())
}

/// The section `group` of `place`: its header and its rows.
fn section_view(place: Place, group: Group, above: f32) -> AnyView {
    let find = move |parts: &Parts| {
        place(parts)
            .iter()
            .find(|section| section.group == group)
            .cloned()
    };
    let head = keyed(
        resource::<Parts, _>(move |parts| {
            find(parts).map(|section| Section {
                title: section.title,
                lines: Vec::new(),
                ..section
            })
        }),
        |head| match head {
            // The title of the hamlet whose page is shown is washed
            // as a selected row is.
            Some(head) if !head.title.is_empty() => {
                header(head, BLOCK_PAD)
            }
            _ => column(()).boxed(),
        },
    )
    .within(hamlet_ui::styles::bare());
    let dimmed = group == Group::Left;
    let rows = each(
        resource::<Parts, _>(move |parts| {
            find(parts)
                .filter(|section| !section.folded)
                .map_or_else(Vec::new, |section| section.lines)
        }),
        Line::clone,
        move |line| expanded(line, dimmed),
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

/// The rail down the Hub's left side: the realm's own rows and what
/// waits, then the tabs and the search, then the part of the realm
/// the tab chooses. Only the headers and rows that change are built
/// anew; the tabs and the search between them are not, so typing
/// keeps its place.
pub(super) fn view() -> impl View {
    let body = column((
        sections_view(|parts| &parts.over, PART_GAP),
        column((tabs_and_search(),))
            .padding(UiRect::top(px(PART_GAP))),
        // Those that left, at the foot of the rail however short the
        // rest is.
        column((
            sections_view(|parts| &parts.under, UNDER_SEARCH),
            frame().grow(1.0),
            sections_view(|parts| &parts.left, PART_GAP),
        ))
        .gap(0.0)
        .grow(1.0),
    ))
    .gap(0.0)
    .grow(1.0);
    with_cx(move |cx| {
        column((scroll_area((body,)),))
            .gap(cx.theme().space.gap)
            .width(resource::<Width, _>(|width| px(width.0)))
            .min_width(px(NARROWEST))
            // Its width is its own: it neither grows into the page's
            // room nor shrinks, and only stretches down.
            .shrink(0.0)
            .height(percent(100.0))
            .min_height(px(0.0))
            .tagged(Body)
    })
}

#[cfg(test)]
mod tests {
    use hamlet_client::realm::named;

    use super::*;
    use crate::testing::panel_app;

    fn names(app: &App, group: fn(&Group) -> bool) -> Vec<String> {
        sections(app.world())
            .into_iter()
            .filter(|section| group(&section.group))
            .flat_map(|section| section.lines)
            .map(|line| line.name)
            .collect()
    }

    #[test]
    fn a_hire_keeps_its_place_in_its_hamlet_and_what_waits_is_on_top()
    {
        let mut app = panel_app();
        app.add_plugins(plugin);
        app.update();
        let ada = named(app.world_mut(), "hire#2").unwrap();
        let in_hamlet = |g: &Group| matches!(g, Group::Hamlet(_));
        assert_eq!(names(&app, in_hamlet), ["ada"]);
        assert!(
            names(&app, |g| *g == Group::Top)
                .contains(&"The Hand".into())
        );

        // Working does not move it.
        app.world_mut().entity_mut(ada).insert(Busy);
        assert_eq!(names(&app, in_hamlet), ["ada"]);

        let mut state = hamlet_testing::realm::state();
        state.waits.push(hamlet_testing::realm::wait("ask"));
        state.waits.push(hamlet_testing::realm::wait("again"));
        state.revision += 1;
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert_eq!(names(&app, |g| *g == Group::Needs), ["ada"]);
        // One row for who asks, counted as every wait, as the
        // realm's page and the status bar count them.
        let needs = sections(app.world())
            .into_iter()
            .find(|section| section.group == Group::Needs)
            .unwrap();
        assert_eq!(needs.count, 2);
        assert_eq!(names(&app, in_hamlet), ["ada"]);
    }

    #[test]
    fn choosing_a_row_builds_again_only_the_rows_it_marks() {
        let mut app = panel_app();
        app.add_plugins(plugin)
            .init_resource::<Showing>()
            .init_resource::<Selected>();
        app.update();
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), view());
        app.update();
        let rows = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<(Entity, &RailRow)>()
                .iter(world)
                .map(|(row, target)| (target.0, row))
                .collect::<Vec<_>>()
        };
        let before = rows(&mut app);

        let ada = named(app.world_mut(), "hire#2").unwrap();
        crate::convo::seat(app.world_mut(), ada);
        *app.world_mut().resource_mut::<Showing>() = Showing::Thread;
        app.update();

        // The realm's row was marked before.
        let marks = [Target::Realm, Target::Agent(ada)];
        let after = rows(&mut app);
        assert_eq!(before.len(), after.len());
        for (target, row) in after {
            let kept = before.contains(&(target, row));
            assert_eq!(kept, !marks.contains(&target), "{target:?}");
        }
    }

    #[test]
    fn a_hire_chosen_in_the_world_is_shown_and_marked_in_the_rail() {
        let mut app = panel_app();
        app.add_plugins(plugin)
            .init_resource::<Showing>()
            .init_resource::<Selected>();
        app.update();
        let ada = named(app.world_mut(), "hire#2").unwrap();
        let hamlet = app.world().get::<LivesIn>(ada).unwrap().0;
        *app.world_mut().resource_mut::<Tab>() = Tab::City;
        app.world_mut().resource_mut::<Rail>().folded =
            vec![Group::Hamlet(hamlet)];

        app.world_mut().resource_mut::<Selected>().0 =
            Some(Thing::Agent(ada));
        crate::hub::follow_selection(app.world_mut());

        assert_eq!(*app.world().resource::<Tab>(), Tab::Hamlets);
        let section = sections(app.world())
            .into_iter()
            .find(|section| section.group == Group::Hamlet(hamlet))
            .unwrap();
        assert!(!section.folded);
        let row = section
            .lines
            .iter()
            .find(|line| line.target == Target::Agent(ada))
            .unwrap();
        assert!(row.current);
    }

    #[test]
    fn a_hire_of_a_city_scholar_is_tagged_and_found_by_its_scholar() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state.agents[1].nickname = Some("Fix the login".into());
        app.world_mut().resource_mut::<Rail>().search = "ada".into();
        let mut found = |state: &protocol::State| {
            let mut state = state.clone();
            state.revision += 1;
            hamlet_client::receive(
                app.world_mut(),
                protocol::ServerMessage::State(state),
            );
            app.update();
            sections(app.world())
                .into_iter()
                .filter(|s| matches!(s.group, Group::Hamlet(_)))
                .flat_map(|section| section.lines)
                .map(|line| (line.name, line.tag.map(|tag| tag.name)))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            found(&state),
            [("Fix the login".to_string(), Some("ada".to_string()))]
        );

        // A blank agent's scholar is its own, not the city's.
        state.revision += 1;
        state.scholars[0].arrival = protocol::Arrival::Apart;
        assert!(found(&state).is_empty());
    }
}
