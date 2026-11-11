//! The Hub's rail: the realm as its chain of command. The realm and
//! the Hand first, then what waits for the chief, then each hamlet as
//! the tree of its hires under its mayor, then the city's scholars,
//! the library and those that left. A row keeps its place as its
//! agent works and rests; only its mark changes.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, ComputedNode, Display, JustifyContent, Overflow,
    PositionType, UiGlobalTransform, UiRect, Val, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{Motion, SurfaceTokens, Tone};
use bevy_fynix::views::{
    self, Axis, BehaviorExt, FrameProps, TooltipExt, button, column,
    frame, label, row, text_field,
};
use bevy_fynix::{
    Dragging, OverrideCursor, ScopedExt, ViewExt, derived,
    every_frame, keyed, resource, style,
};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::realm::{
    Activity, Busy, HamletId, Identity, Kind, Left, Lived, LivesIn,
    Mayor, MergedInto, Parent, Purpose, Scholar, Shown, Skills,
    name_of, shown_of,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing, bring_into_view, select};
use hamlet_ui::styles::{
    ICON, STATUS_PAD, description_rules, note, note_rules, quoted,
    scroll_area, status_icon_at, text_input, transparent,
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
use crate::standing::{Standing, need_you};
use crate::status::now_ms;

/// The width of the rail at first, the least it is dragged to, so a
/// row keeps its name, and the most.
const WIDE: f32 = 248.0;
const NARROWEST: f32 = 200.0;
const WIDEST: f32 = 520.0;
/// The least the rail leaves the page beside it.
const PAGE: f32 = 280.0;

/// The most that left a rail shows.
const LEFT_SHOWN: usize = 6;

/// The most letters of a line a row holds; the rail's width cuts it
/// shorter.
const LINE_SHOWN: usize = 90;

/// The most letters of a row's second line, which wraps rather than
/// being cut at the rail's edge.
const TITLE_SHOWN: usize = 56;

/// The least height of a row, so that a row of one line is as easy
/// to press as one of two.
const ROW: f32 = 32.0;

/// The height of a worker's row and the side of its icon, smaller
/// than a hire's, so the hires stand out of their workers.
const WORKER_ROW: f32 = 26.0;
const WORKER_ICON: f32 = 12.0;

/// How opaque the icon of one that left is.
const DIMMED: f32 = 0.6;

/// The least height of `line`'s row.
fn row_height(line: &Line) -> f32 {
    if line.worker { WORKER_ROW } else { ROW }
}

/// The column at the rail's edge that holds every chevron.
const CHEVRON: f32 = 14.0;

/// How far each step down a tree is set in: a chevron's room and a
/// little more, so a guide under a chevron leads to the next one.
const STEP_IN: f32 = 18.0;

/// The deepest a row is set in; deeper rows line up with it.
const DEEPEST: u8 = 6;

/// The room between an elbow's end and the row it leads to.
const GAP: f32 = 2.0;

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

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Rail>()
        .init_resource::<Tab>()
        .add_observer(grab)
        .add_observer(drag)
        .add_observer(drop)
        .add_systems(Update, (read_search, tabs::mark::<Tab>));
}

/// The rail's search, what is folded, and its width.
#[derive(Resource, PartialEq)]
pub struct Rail {
    search: String,
    /// The parts folded to their header. A hamlet with something
    /// waiting opens whatever this says.
    folded: Vec<Group>,
    /// How wide the chief dragged it.
    width: f32,
    /// The hamlets left out of it; one with something waiting is
    /// shown whatever this says.
    hidden: Vec<Entity>,
    /// Whether the list of hamlets to show is open.
    filtering: bool,
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

impl Default for Rail {
    fn default() -> Self {
        Self {
            search: String::new(),
            folded: Vec::new(),
            width: WIDE,
            hidden: Vec::new(),
            filtering: false,
        }
    }
}

impl Rail {
    /// Shows the rail's rows at its first width.
    pub fn show_rows(&mut self) {
        self.width = WIDE;
    }

    /// Folds `group` to its header, or opens it.
    fn toggle(&mut self, group: Group) {
        match self.folded.iter().position(|g| *g == group) {
            Some(at) => {
                self.folded.remove(at);
            }
            None => self.folded.push(group),
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
        .transition(Motion::Interact)
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
    mut rail: ResMut<Rail>,
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
    rail.width = (at.x - left - past).clamp(NARROWEST, most);
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
}

/// A row of the rail.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Line {
    pub target: Target,
    pub glyph: Glyph,
    pub name: String,
    pub title: String,
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
    /// Whether it is a worker's: one short line, lighter than a
    /// hire's, so the hires stand out of their workers.
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
    title: String,
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
    let reference = protocol::ChatRef {
        kind: None,
        from: None,
        with: id,
        past: None,
    };
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

/// What an agent's row says under its name: its work while it works,
/// else the chief's last words to it, else what it was asked, and
/// nothing where its mark already says the same. Then when its chat
/// was last added to.
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
                let (title, last) = under_name(world, entity);
                Agent {
                    entity,
                    id: id.0,
                    kind: kind.0,
                    parent: parent.map(|parent| parent.0),
                    hamlet: lives.map(|lives| lives.0),
                    mayor,
                    mark: Some(Standing::of(world, entity)),
                    called: called(&address, shown),
                    address,
                    title,
                    since: last.max(lived.since),
                }
            },
        )
        .collect()
}

/// `root` and everyone under it among `all`, each with its depth
/// below `root`: hires before workers, each kind by name.
fn tree<'a>(
    root: &'a Agent,
    all: &[&'a Agent],
) -> Vec<(&'a Agent, u8)> {
    fn walk<'a>(
        agent: &'a Agent,
        all: &[&'a Agent],
        depth: u8,
        seen: &mut Vec<protocol::AgentId>,
        out: &mut Vec<(&'a Agent, u8)>,
    ) {
        if seen.contains(&agent.id) {
            return;
        }
        seen.push(agent.id);
        out.push((agent, depth));
        let mut under = all
            .iter()
            .copied()
            .filter(|other| other.parent == Some(agent.id))
            .collect::<Vec<_>>();
        under.sort_by_key(|other| {
            (
                other.kind == protocol::AgentKind::Worker,
                other.called.to_lowercase(),
            )
        });
        for child in under {
            walk(child, all, depth.saturating_add(1), seen, out);
        }
    }
    let mut out = Vec::new();
    walk(root, all, 0, &mut Vec::new(), &mut out);
    out
}

/// The rows of `family`, each set in as deep as it is in the tree,
/// knowing whether it is the last under the agent above it and which
/// guides above it carry on past it.
fn family_lines(world: &World, family: &[(&Agent, u8)]) -> Vec<Line> {
    // Whether the agent last seen at each depth has more after it.
    let mut more = Vec::<bool>::new();
    family
        .iter()
        .enumerate()
        .map(|(at, (member, depth))| {
            let last = family.get(at + 1..).is_none_or(|after| {
                after
                    .iter()
                    .take_while(|(_, d)| d >= depth)
                    .all(|(_, d)| d > depth)
            });
            let depth = (*depth).min(DEEPEST);
            let level = usize::from(depth);
            let mut line = agent_line(world, member, depth, last);
            // The guides of the levels between the top and its own.
            line.rails =
                more.get(1..level).unwrap_or_default().to_vec();
            more.resize(level + 1, false);
            more[level] = !last;
            line
        })
        .collect()
}

fn kind_glyph(agent: &Agent) -> Glyph {
    match agent.kind {
        protocol::AgentKind::Hand => Glyph::Tower,
        protocol::AgentKind::Hire if agent.mayor => Glyph::Mayor,
        protocol::AgentKind::Hire => Glyph::Hire,
        protocol::AgentKind::Worker => Glyph::Robot,
    }
}

/// Whether the Hub is turned to `target`.
fn is_current(world: &World, target: Target) -> bool {
    let showing = world.get_resource::<Showing>().copied();
    let seated = world.get_resource::<Convo>().map(|c| c.agent);
    match (target, showing) {
        (
            Target::Hand | Target::Agent(_),
            Some(
                Showing::Thread | Showing::Sessions | Showing::Prompt,
            ),
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
        title: agent.title.clone(),
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
    for at in 0..lines.len() {
        let depth = lines[at].depth;
        let under = lines[at + 1..]
            .iter()
            .take_while(|line| line.depth > depth)
            .count();
        let Target::Agent(agent) = lines[at].target else {
            continue;
        };
        if under == 0 {
            continue;
        }
        let range = &lines[at + 1..at + 1 + under];
        let group = Group::Lead(agent);
        let shut = folded.contains(&group);
        // Open, each row shows its own state; folded, the lead
        // stands for all of them.
        let mark = Some(Standing::of_all(
            std::iter::once(lines[at].mark)
                .chain(range.iter().map(|line| line.mark))
                .flatten(),
        ));
        let line = &mut lines[at];
        line.fold = Some((group, shut));
        if shut {
            line.mark = mark;
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
    title: String,
    mark: Option<Standing>,
) -> Line {
    Line {
        target,
        glyph,
        name: name.into(),
        title,
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

/// Whether `line` matches what is typed in the search.
fn matches(line: &Line, search: &str) -> bool {
    search.is_empty()
        || line.name.to_lowercase().contains(search)
        || line.address.to_lowercase().contains(search)
        || line.title.to_lowercase().contains(search)
}

/// What waits for the chief, one row for each who asks: the words of
/// its oldest wait, how long it has waited, and how many more; and
/// how many waits there are in all.
fn needs(world: &World, all: &[Agent]) -> (Vec<Line>, usize) {
    let Some(mut query) = world.try_query::<&Waiting>() else {
        return (Vec::new(), 0);
    };
    let mut waits = query.iter(world).collect::<Vec<_>>();
    let total = waits.len();
    waits.sort_by_key(|wait| wait.since);
    let mut lines = Vec::<Line>::new();
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
                title: "Wants to move into the city".into(),
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
        if let Some(at) =
            lines.iter().position(|line| line.address == wait.by)
        {
            counts[at] += 1;
            continue;
        }
        let line = wait.line(LINE_SHOWN);
        let said = line
            .strip_prefix(wait.shown.as_str())
            .unwrap_or(&line)
            .trim_start();
        let asker = all.iter().find(|agent| agent.address == wait.by);
        let (target, glyph, name) = match asker {
            Some(agent)
                if agent.kind == protocol::AgentKind::Hand =>
            {
                (Target::Hand, Glyph::Tower, "The Hand".to_string())
            }
            // What it goes by; its address, hamlet and all, is the
            // row's tooltip.
            Some(agent) => (
                Target::Agent(agent.entity),
                kind_glyph(agent),
                agent.called.clone(),
            ),
            // The realm's page has every wait in full.
            None => (Target::Realm, Glyph::Bell, wait.shown.clone()),
        };
        lines.push(Line {
            target,
            glyph,
            name,
            title: capitalised(said),
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
            line.title = format!("{count} waits · {}", line.title);
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
    let (search, folded, hidden) =
        world.get_resource::<Rail>().map_or_else(
            || (String::new(), Vec::new(), Vec::new()),
            |rail| {
                (
                    rail.search.trim().to_lowercase(),
                    rail.folded.clone(),
                    rail.hidden.clone(),
                )
            },
        );
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
        top.extend(family_lines(world, &tree(hand, &outside)));
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
    for (hamlet, name) in hamlet_names(world) {
        let here = all
            .iter()
            .filter(|agent| agent.hamlet == Some(hamlet))
            .collect::<Vec<_>>();
        let mut roots = here
            .iter()
            .copied()
            .filter(|agent| {
                agent.parent.is_none_or(|parent| {
                    !here.iter().any(|a| a.id == parent)
                })
            })
            .collect::<Vec<_>>();
        roots.sort_by_key(|agent| {
            (!agent.mayor, agent.called.clone())
        });
        let mut lines = Vec::new();
        for root in roots {
            lines.extend(family_lines(world, &tree(root, &here)));
        }
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
            | Group::Lead(_) => true,
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
            tool(Glyph::Plus, "Found a hamlet", |w| {
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
            (open, rail.hidden.clone(), hamlet_names(world))
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
            tabs::strip(&TABS, Spread::Shared, |world, tab| {
                *world.resource_mut::<Tab>() = tab;
            }),
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
        Target::Hand => talk_to_hand(world),
        // Its thread, and its house brought into view.
        Target::Agent(agent) => {
            talk_to(world, agent);
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
        .transition(Motion::Interact)
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

/// A row: its icon, then its name with how long ago and its mark at
/// the end, over what it is doing, which runs the row's whole width
/// and wraps rather than being cut. A worker's row is one short line
/// with what it is doing beside its name, cut at the rail's edge.
/// A `dimmed` row, of those that left, is set in grey.
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
    let title = cut(&line.title, TITLE_SHOWN);
    let clipped = |shown: AnyView| {
        column((shown,))
            .min_width(px(0.0))
            .overflow(Overflow::clip())
    };
    let name = clipped(
        one_line(
            label(line.name.clone())
                .tone(name_tone)
                .bold(!(worker || dimmed)),
        )
        .boxed(),
    );
    let words = if worker {
        row((
            name,
            clipped(one_line(label(title.clone())).boxed())
                .grow(1.0)
                .rules(note_rules),
        ))
        .gap(6.0)
        .align(AlignItems::Center)
        .grow(1.0)
        .min_width(px(0.0))
        .boxed()
    } else {
        name.grow(1.0).boxed()
    };
    // The icon on the middle of the name's line.
    let head = row((
        glyph(line.glyph, if worker { WORKER_ICON } else { ICON })
            .tone(Tone::Dim)
            .opacity(if dimmed { DIMMED } else { 1.0 }),
        row((
            words,
            label(
                derived(move |_: &World| age(now_ms(), since))
                    .when(every_frame()),
            )
            .wrap(false)
            .rules(note_rules),
            mark_icon(line.mark),
        ))
        .gap(6.0)
        .align(AlignItems::Center)
        .grow(1.0)
        .min_width(px(0.0)),
    ))
    .gap(8.0)
    .align(AlignItems::Center);
    // Under it, from the row's own edge rather than the name's, what
    // it is doing. A row with nothing more to say is one line.
    let doing = column((if title.contains('`') {
        quoted(&title)
    } else {
        label(title.clone()).boxed()
    },))
    .min_width(px(0.0))
    .display(if title.is_empty() || worker {
        Display::None
    } else {
        Display::Flex
    })
    .rules(description_rules);
    let content = column((head, doing))
        .gap(0.0)
        .justify(JustifyContent::Center)
        .min_height(px(row_height(line) - 2.0 * ROW_DOWN))
        .width(percent(100.0));
    let address = line.address.clone();
    let shown = washed(
        button(content)
            .grow(1.0)
            .min_width(px(0.0))
            .rules(row_rules)
            .tagged(Variant::Default)
            .on_activate(move |world| go(world, target)),
        line.current,
    )
    .boxed();
    let shown = if address.is_empty() || address == line.name {
        shown.boxed()
    } else {
        shown.tooltip(move || note(address.clone())).boxed()
    };
    // The chevron of a lead, outside the row's press so that folding
    // does not open the thread, then the step in of a row under one.
    let fold = match line.fold {
        Some((group, shut)) => chevron(
            group,
            shut,
            if shut {
                "Show the hires under it"
            } else {
                "Fold the hires under it"
            },
        ),
        None => frame().width(px(CHEVRON)).shrink(0.0).boxed(),
    };
    let step = frame()
        .width(px(f32::from(line.depth) * STEP_IN))
        .shrink(0.0);
    let mut parts = vec![step.boxed(), fold, shown];
    parts.extend(tree_lines(line, title.is_empty() || worker));
    row(parts)
        .gap(0.0)
        .align(AlignItems::Center)
        .width(percent(100.0))
        .boxed()
}

/// The tree's lines over a row, on a grid of [`STEP_IN`]: for each
/// level above it, the guide of an agent that has more under it after
/// this row, then the elbow from its own lead, which runs on down
/// while more follow. Every guide runs under the middle of its
/// agent's chevron, so the tree lines up however deep it goes, and
/// an elbow stops short of the row it leads to. A row of `one_line`
/// has its icon on its middle, one of more on its first.
fn tree_lines(line: &Line, one_line: bool) -> Vec<AnyView> {
    if line.depth == 0 {
        return Vec::new();
    }
    // Where the guide of the agent at `level` runs.
    let guide =
        |level: usize| level as f32 * STEP_IN + CHEVRON / 2.0 - 0.5;
    let lead = usize::from(line.depth) - 1;
    // Where the elbow meets the row, and where it stops.
    let elbow = ROW_DOWN
        + if one_line {
            (row_height(line) - 2.0 * ROW_DOWN) / 2.0
        } else {
            2.0 + ICON / 2.0
        };
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
    // of its icon where it has no chevron.
    let start = f32::from(line.depth) * STEP_IN;
    let end = match line.fold {
        Some(_) => start - GAP,
        None => start + CHEVRON + ROW_INSET - GAP,
    };
    let x = guide(lead);
    lines.push(over(x, 0.0, 1.0, Some(elbow + 1.0)));
    lines.push(over(x, elbow, end - x, Some(1.0)));
    if !line.last {
        lines.push(over(x, elbow, 1.0, None));
    }
    lines
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
    match section.opens {
        // Filled under the pointer as a row is, outside the chevron's
        // press so that folding does not open the page; its mark in
        // the column of the rows' marks.
        Some(target) => row((
            fold,
            washed(
                button(words.grow(1.0))
                    .grow(1.0)
                    .min_width(px(0.0))
                    .rules(row_rules)
                    .tagged(Variant::Default)
                    .on_activate(move |world| go(world, target)),
                section.current
                    && matches!(section.group, Group::Hamlet(_)),
            ),
        ))
        .gap(0.0)
        .align(AlignItems::Center)
        .padding(UiRect::top(px((above - ROW_DOWN).max(0.0))))
        .width(percent(100.0))
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

/// `sections` one under another, the first header `first_above` under
/// what is over it.
fn sections_view(sections: &[Section], first_above: f32) -> AnyView {
    let mut parts = Vec::new();
    for section in sections {
        let mut block = Vec::new();
        // The room over a header is kept outside its block, so a lit
        // block starts at its header.
        let mut above = 0.0;
        if !section.title.is_empty() {
            above = if parts.is_empty() {
                first_above
            } else {
                PART_GAP
            };
            // The title of the hamlet whose page is shown is washed
            // as a selected row is.
            block.push(header(section, BLOCK_PAD));
            above = (above - BLOCK_PAD).max(0.0);
        }
        let lines = if section.folded {
            &[][..]
        } else {
            &section.lines[..]
        };
        let dimmed = section.group == Group::Left;
        block.extend(lines.iter().map(|line| expanded(line, dimmed)));
        parts.push(
            with_cx(move |cx| {
                column(block)
                    .gap(0.0)
                    .margin(UiRect::top(px(above)))
                    .radius(cx.theme().space.round)
            })
            .boxed(),
        );
    }
    column(parts).gap(0.0).boxed()
}

/// Whether `section` is of the realm's own, over the tabs: the realm,
/// the Hand, the library and what waits.
fn is_over_tabs(section: &Section) -> bool {
    matches!(section.group, Group::Top | Group::Needs)
}

/// The rail down the Hub's left side: the realm's own rows and what
/// waits, then the tabs and the search, then the part of the realm
/// the tab chooses. The rows are built anew as they change; the tabs
/// and the search between them are not, so typing keeps its place.
pub(super) fn view() -> impl View {
    let over = keyed(
        derived(|world: &World| {
            // The realm's page lists what waits in full.
            let on_realm = world.get_resource::<Showing>()
                == Some(&Showing::Realm);
            sections(world)
                .into_iter()
                .filter(is_over_tabs)
                .filter(|section| {
                    !(on_realm && section.group == Group::Needs)
                })
                .collect::<Vec<_>>()
        })
        .when(every_frame()),
        |sections| sections_view(sections, PART_GAP),
    )
    .within(hamlet_ui::styles::bare());
    let under = keyed(
        derived(|world: &World| {
            sections(world)
                .into_iter()
                .filter(|section| !is_over_tabs(section))
                .collect::<Vec<_>>()
        })
        .when(every_frame()),
        |sections| {
            // Those that left, at the foot of the rail however short
            // the rest is.
            let (left, rest): (Vec<_>, Vec<_>) = sections
                .iter()
                .cloned()
                .partition(|section| section.group == Group::Left);
            column((
                sections_view(&rest, UNDER_SEARCH),
                frame().grow(1.0),
                sections_view(&left, PART_GAP),
            ))
            .gap(0.0)
            .grow(1.0)
            .boxed()
        },
    )
    .within(hamlet_ui::styles::bare().grow(1.0));
    let body = column((
        over,
        column((tabs_and_search(),))
            .padding(UiRect::top(px(PART_GAP))),
        under,
    ))
    .gap(0.0)
    .grow(1.0);
    with_cx(move |cx| {
        column((scroll_area((body,)),))
            .gap(cx.theme().space.gap)
            .width(resource::<Rail, _>(|rail| px(rail.width)))
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
        let ada = named(app.world_mut(), "home#0").unwrap();
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
}
