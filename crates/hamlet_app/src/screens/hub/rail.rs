//! The Hub's rail: the realm as its chain of command. Home and the
//! Hand first, then what waits for the chief, then each hamlet as the
//! tree of its hires under its mayor, then the city's scholars, the
//! library and those that left. A row keeps its place as its agent
//! works and rests; only its mark changes.

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
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
    AlignItems, ComputedNode, JustifyContent, Overflow,
    UiGlobalTransform, UiRect, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{Motion, Tone};
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
    Activity, Busy, Faulted, HamletId, Identity, Kind, Left, Lived,
    LivesIn, Mayor, MergedInto, Nickname, Parent, Purpose, Scholar,
    Skills, name_of,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing, focus_on, select};
use hamlet_ui::styles::{
    Fills, ICON, caption, hairline, note, scroll_area, text_input,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::{Showing, show};
use crate::convo::{Convo, talk_to, talk_to_hand};
use crate::dock::{PETITIONS, show_tab};
use crate::status::now_ms;

/// The width of the rail with its rows at first, and with only their
/// icons.
const WIDE: f32 = 248.0;
const ICONS: f32 = 46.0;

/// The width under which a dragged rail folds to its icons.
const FOLDS: f32 = 140.0;
/// The most the rail is wide, and the least it leaves the page.
const WIDEST: f32 = 520.0;
const PAGE: f32 = 280.0;

/// The most that left a rail shows.
const LEFT_SHOWN: usize = 6;

/// The most letters of a line a row holds; the rail's width cuts it
/// shorter.
const LINE_SHOWN: usize = 90;

/// How tall a row is, so that the lines of a branch meet.
const ROW: f32 = 40.0;

/// The room before a row for a lead's chevron.
const CHEVRON: f32 = 14.0;

/// How far each step down a tree is set in.
const STEP_IN: f32 = 12.0;
/// The deepest a row is set in; deeper rows line up with it.
const DEEPEST: u8 = 6;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Rail>()
        .add_observer(grab)
        .add_observer(drag)
        .add_observer(drop)
        .add_systems(Update, read_search);
}

/// The rail's search, what is folded, and its width.
#[derive(Resource, PartialEq)]
pub struct Rail {
    search: String,
    /// The parts folded to their header. A hamlet with something
    /// waiting opens whatever this says.
    folded: Vec<Group>,
    /// How wide the chief dragged it; under [`FOLDS`] it shows only
    /// icons.
    width: f32,
    /// The width it unfolds to.
    unfolded: f32,
}

impl Default for Rail {
    fn default() -> Self {
        Self {
            search: String::new(),
            // The city is a long list the chief seldom needs.
            folded: vec![Group::City],
            width: WIDE,
            unfolded: WIDE,
        }
    }
}

impl Rail {
    /// Shows the rail's rows at its first width.
    pub fn show_rows(&mut self) {
        self.width = WIDE;
    }

    fn is_icons(&self) -> bool {
        self.width < FOLDS
    }

    /// How wide the rail is drawn.
    fn shown(&self) -> f32 {
        if self.is_icons() { ICONS } else { self.width }
    }

    /// Folds the rail to its icons, or opens it to where it was.
    fn fold(&mut self) {
        if self.is_icons() {
            self.width = self.unfolded;
        } else {
            self.unfolded = self.width;
            self.width = ICONS;
        }
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

/// The line between the rail and the page: dragged, it sizes the
/// rail, which folds to its icons when dragged narrow enough.
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

/// Sizes the rail to where the pointer is, so the line stays under
/// it however fast it moves.
fn drag(
    drag: On<PointerDrag>,
    dividers: Query<(), With<Divider>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Frame>>,
    mut rail: ResMut<Rail>,
) {
    if !dividers.contains(drag.entity) {
        return;
    }
    let at = drag.pointer.position;
    // The Hub starts where the rail does, in logical pixels.
    let Some((node, place)) = frames.iter().next() else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let left = (place.translation.x - node.size().x * 0.5) * scale;
    let most = (node.size().x * scale - PAGE).clamp(FOLDS, WIDEST);
    let width = at.x - left;
    rail.width = width.clamp(ICONS, most);
}

fn drop(
    end: On<PointerDragEnd>,
    dividers: Query<(), With<Divider>>,
    mut rail: ResMut<Rail>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if !dividers.contains(end.entity) {
        return;
    }
    cursor.0 = None;
    if !rail.is_icons() {
        rail.unfolded = rail.width;
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
    Home,
    Hand,
    Agent(Entity),
    Hamlet(Entity),
    Scholar(Entity),
    Library,
    Gone(Entity),
    Petitions,
}

/// The state shown at a row's end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mark {
    Waits,
    Works,
    Rests,
    Faulted,
    Gone,
    /// Nothing to say: a page, not someone.
    None,
}

impl Mark {
    fn shown(self) -> Option<(Glyph, Tone)> {
        match self {
            Self::Waits => Some((Glyph::Bell, Tone::Critical)),
            Self::Works => Some((Glyph::Bolt, Tone::Accent)),
            Self::Rests => Some((Glyph::Moon, Tone::Faint)),
            Self::Faulted => Some((Glyph::Stop, Tone::Critical)),
            Self::Gone => Some((Glyph::Gone, Tone::Faint)),
            Self::None => None,
        }
    }

    /// The mark of a group of rows: the most pressing of theirs.
    fn of_all(marks: impl Iterator<Item = Mark>) -> Mark {
        let marks = marks.collect::<Vec<_>>();
        [Mark::Waits, Mark::Faulted, Mark::Works]
            .into_iter()
            .find(|mark| marks.contains(mark))
            .unwrap_or(Mark::Rests)
    }
}

/// A row of the rail.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Line {
    pub target: Target,
    pub glyph: Glyph,
    pub name: String,
    pub title: String,
    pub age: String,
    pub mark: Mark,
    /// What the search also finds it by: its address.
    pub address: String,
    /// How far under another agent it is nested.
    pub depth: u8,
    /// Whether no row after it is nested under the same agent.
    pub last: bool,
    pub current: bool,
    /// For a lead: the fold of the rows under it, and whether it is
    /// folded.
    pub fold: Option<(Group, bool)>,
}

/// A group, its header and its rows.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Section {
    pub group: Group,
    pub title: String,
    /// Where pressing its title leads, if anywhere.
    pub opens: Option<Target>,
    /// The state of all of it, shown on its header.
    pub mark: Mark,
    pub count: usize,
    pub folded: bool,
    pub lines: Vec<Line>,
}

/// An agent as the rail reads it.
struct Agent {
    entity: Entity,
    id: protocol::AgentId,
    kind: protocol::AgentKind,
    parent: Option<protocol::AgentId>,
    hamlet: Option<Entity>,
    mayor: bool,
    mark: Mark,
    /// Its address, and what it goes by: its nickname, or its
    /// address without its hamlet.
    address: String,
    called: String,
    title: String,
    age: String,
}

/// The seconds between `then` and `now`, in the shortest word.
fn age(now: u64, then: u64) -> String {
    if then == 0 {
        return String::new();
    }
    match now.saturating_sub(then) / 1000 {
        0..=59 => "now".into(),
        s @ 60..=3599 => format!("{}m", s / 60),
        s @ 3600..=86399 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}

/// The first line of `text` that has words, cut at `most` letters.
fn first_line(text: &str, most: usize) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    match line.char_indices().nth(most) {
        Some((cut, _)) => format!("{}...", &line[..cut]),
        None => line.to_string(),
    }
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
                Some(first_line(text, LINE_SHOWN))
            }
            _ => None,
        });
    (said, items.last().map_or(0, |item| item.at))
}

/// What an agent with `address` is called in the rail: `nickname`,
/// else its address without its hamlet.
fn called(address: &str, nickname: Option<&Nickname>) -> String {
    match nickname {
        Some(nickname) => nickname.0.clone(),
        None => hamlet_realm::pick::short(address).to_string(),
    }
}

fn agents(world: &World) -> Vec<Agent> {
    let Some(mut query) = world.try_query::<(
        Entity,
        &Identity,
        &Kind,
        Option<&Parent>,
        Option<&LivesIn>,
        &Activity,
        &Lived,
        Option<&Purpose>,
        Option<&Nickname>,
        (Has<Busy>, Has<Faulted>, Has<Mayor>, Has<Left>),
    )>() else {
        return Vec::new();
    };
    let waiting = world
        .try_query::<&Waiting>()
        .map(|mut waits| {
            waits
                .iter(world)
                .map(|wait| wait.by.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let now = now_ms();
    query
        .iter(world)
        .filter(|(.., (_, _, _, left))| !left)
        .map(
            |(
                entity,
                id,
                kind,
                parent,
                lives,
                activity,
                lived,
                purpose,
                nickname,
                (busy, faulted, mayor, _),
            )| {
                let address = name_of(world, entity);
                let (said, last) = thread(world, id.0);
                let mark = if waiting.contains(&address) {
                    Mark::Waits
                } else if faulted {
                    Mark::Faulted
                } else if busy {
                    Mark::Works
                } else {
                    Mark::Rests
                };
                // Its work while it works, else what it was asked.
                let title = if busy {
                    Some(activity.words.clone())
                } else {
                    None
                }
                .or(said.map(|said| format!("You: {said}")))
                .or_else(|| {
                    purpose
                        .map(|p| first_line(&p.0, LINE_SHOWN))
                        .filter(|line| !line.is_empty())
                })
                .unwrap_or_else(|| activity.words.clone());
                Agent {
                    entity,
                    id: id.0,
                    kind: kind.0,
                    parent: parent.map(|parent| parent.0),
                    hamlet: lives.map(|lives| lives.0),
                    mayor,
                    mark,
                    called: called(&address, nickname),
                    address,
                    title,
                    age: age(now, last.max(lived.since)),
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

/// The rows of `family`, each knowing whether it is the last under
/// the agent above it.
fn family_lines(
    world: &World,
    family: &[(&Agent, u8)],
    shift: u8,
) -> Vec<Line> {
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
            agent_line(world, member, depth + shift, last)
        })
        .collect()
}

fn kind_glyph(agent: &Agent) -> Glyph {
    match agent.kind {
        protocol::AgentKind::Hand => Glyph::Shield,
        protocol::AgentKind::Hire if agent.mayor => Glyph::Mayor,
        protocol::AgentKind::Hire => Glyph::Robot,
        protocol::AgentKind::Worker => Glyph::Chip,
    }
}

/// Whether the Hub is turned to `target`.
fn is_current(world: &World, target: Target) -> bool {
    let showing = world.get_resource::<Showing>().copied();
    let seated = world.get_resource::<Convo>().map(|c| c.agent);
    match (target, showing) {
        (Target::Hand | Target::Agent(_), Some(Showing::Thread)) => {
            match target {
                Target::Agent(agent) => seated == Some(agent),
                _ => seated.is_some_and(|agent| {
                    world.get::<Kind>(agent).map(|kind| kind.0)
                        == Some(protocol::AgentKind::Hand)
                }),
            }
        }
        (Target::Scholar(a), Some(Showing::Scholar(b)))
        | (Target::Gone(a), Some(Showing::Gone(b)))
        | (Target::Hamlet(a), Some(Showing::Hamlet(b))) => a == b,
        (Target::Library, Some(Showing::Library))
        | (Target::Home, Some(Showing::Home)) => true,
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
        age: agent.age.clone(),
        mark: agent.mark,
        address: agent.address.clone(),
        depth,
        last,
        current: is_current(world, target),
        fold: None,
    }
}

/// Lets each lead in `lines` stand for the rows under it: its mark
/// becomes the most pressing of theirs, and while folded they are
/// left out and it says how many there are. A lead with something
/// waiting under it stays open.
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
        // A mayor's rows fold with its hamlet's header, and a lead
        // with something waiting under it stays open.
        let range = &lines[at + 1..at + 1 + under];
        let waits = range.iter().any(|line| line.mark == Mark::Waits);
        if under == 0 || depth == 0 || waits {
            continue;
        }
        let group = Group::Lead(agent);
        let shut = folded.contains(&group);
        // Open, each row shows its own state; folded, the lead
        // stands for all of them.
        let mark = Mark::of_all(
            std::iter::once(lines[at].mark)
                .chain(range.iter().map(|line| line.mark)),
        );
        let line = &mut lines[at];
        line.fold = Some((group, shut));
        if shut {
            line.mark = mark;
            line.title = format!("{under} under · {}", line.title);
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
    mark: Mark,
) -> Line {
    Line {
        target,
        glyph,
        name: name.into(),
        title,
        age: String::new(),
        mark,
        address: String::new(),
        depth: 0,
        last: true,
        current: is_current(world, target),
        fold: None,
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
/// its oldest wait, how long it has waited, and how many more.
fn needs(world: &World, all: &[Agent]) -> Vec<Line> {
    let Some(mut query) = world.try_query::<&Waiting>() else {
        return Vec::new();
    };
    let mut waits = query.iter(world).collect::<Vec<_>>();
    waits.sort_by_key(|wait| wait.since);
    let now = now_ms();
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
                glyph: Glyph::Notes,
                name: format!("{name} · harbor"),
                title: "Wants to be let into the city".into(),
                age: age(now, wait.since),
                mark: Mark::Waits,
                address: name,
                depth: 0,
                last: true,
                current: false,
                fold: None,
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
            .strip_prefix(wait.by.as_str())
            .unwrap_or(&line)
            .trim_start();
        let asker = all.iter().find(|agent| agent.address == wait.by);
        let (target, glyph, name) = match asker {
            Some(agent)
                if agent.kind == protocol::AgentKind::Hand =>
            {
                (Target::Hand, Glyph::Shield, "The Hand".to_string())
            }
            // What it goes by, and the hamlet it is in.
            Some(agent) => (
                Target::Agent(agent.entity),
                kind_glyph(agent),
                match agent.hamlet {
                    Some(hamlet) => format!(
                        "{} · {}",
                        agent.called,
                        name_of(world, hamlet)
                    ),
                    None => agent.called.clone(),
                },
            ),
            None => (Target::Petitions, Glyph::Bell, wait.by.clone()),
        };
        lines.push(Line {
            target,
            glyph,
            name,
            title: said.to_string(),
            age: age(now, wait.since),
            mark: Mark::Waits,
            address: wait.by.clone(),
            depth: 0,
            last: true,
            current: false,
            fold: None,
        });
        counts.push(1);
    }
    for (line, count) in lines.iter_mut().zip(counts) {
        if count > 1 {
            line.title = format!("{count} waits · {}", line.title);
        }
    }
    lines
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
                (Some(_), _) => ("Left the city".into(), Mark::Gone),
                (None, protocol::Arrival::Waiting) => (
                    "Waits to be let in".into(),
                    if chief_holds_door {
                        Mark::Waits
                    } else {
                        Mark::None
                    },
                ),
                (None, protocol::Arrival::Turned) => {
                    ("Turned away".into(), Mark::Gone)
                }
                (None, protocol::Arrival::Housed)
                    if hamlets.is_empty() =>
                {
                    ("Not hired".into(), Mark::None)
                }
                (None, protocol::Arrival::Housed) => {
                    (format!("In {}", hamlets.join(", ")), Mark::None)
                }
            };
            let line = page_line(
                world,
                Target::Scholar(entity),
                Glyph::Notes,
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
        (Group::City, "CITY", city),
        (Group::Harbor, "AT THE HARBOR", harbor),
    ] {
        sections.push(Section {
            group,
            title: title.into(),
            opens: None,
            mark: Mark::None,
            count: lines.len(),
            folded: false,
            lines,
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
                Glyph::Robot,
                &address,
                match into {
                    Some(host) => {
                        format!(
                            "Merged into {}",
                            name_of(world, host.0)
                        )
                    }
                    None => "Ended".into(),
                },
                Mark::Gone,
            );
            line.address = address;
            gone.push(line);
        }
    }
    let count = gone.len();
    sections.push(Section {
        group: Group::Left,
        title: "LEFT".into(),
        opens: None,
        mark: Mark::None,
        count,
        folded: false,
        lines: gone,
    });
}

/// The rail's rows in their groups. A group with no row is left out;
/// while searching every group is open and keeps only what matches.
pub(super) fn sections(world: &World) -> Vec<Section> {
    let (search, folded) = world.get_resource::<Rail>().map_or_else(
        || (String::new(), Vec::new()),
        |rail| {
            (rail.search.trim().to_lowercase(), rail.folded.clone())
        },
    );
    let all = agents(world);
    let mut sections = Vec::new();

    // The realm, and the Hand with what it started outside any
    // hamlet.
    let mut top = vec![page_line(
        world,
        Target::Home,
        Glyph::Globe,
        "Realm",
        "Every hamlet at a glance".into(),
        Mark::None,
    )];
    if let Some(hand) =
        all.iter().find(|a| a.kind == protocol::AgentKind::Hand)
    {
        let outside = all
            .iter()
            .filter(|agent| agent.hamlet.is_none())
            .collect::<Vec<_>>();
        top.extend(family_lines(world, &tree(hand, &outside), 0));
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
        Mark::None,
    ));
    sections.push(Section {
        group: Group::Top,
        title: String::new(),
        opens: None,
        mark: Mark::None,
        count: 0,
        folded: false,
        lines: top,
    });

    let needs = needs(world, &all);
    sections.push(Section {
        group: Group::Needs,
        title: "NEEDS YOU".into(),
        opens: Some(Target::Petitions),
        mark: Mark::None,
        count: needs.len(),
        folded: false,
        lines: needs,
    });

    // Each hamlet as the tree under its mayor.
    let mut hamlets = world
        .try_query::<(Entity, &HamletId)>()
        .map(|mut query| {
            query
                .iter(world)
                .map(|(hamlet, _)| (hamlet, name_of(world, hamlet)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    hamlets.sort_by_key(|(_, name)| name.to_lowercase());
    for (hamlet, name) in hamlets {
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
            lines.extend(family_lines(world, &tree(root, &here), 0));
        }
        let group = Group::Hamlet(hamlet);
        let mark = Mark::of_all(lines.iter().map(|line| line.mark));
        // A search finds rows a lead would fold away.
        if search.is_empty() {
            lines = fold_leads(lines, &folded);
        }
        let hires = here
            .iter()
            .filter(|a| a.kind == protocol::AgentKind::Hire)
            .count();
        sections.push(Section {
            group,
            title: name,
            opens: Some(Target::Hamlet(hamlet)),
            mark,
            count: hires,
            folded: folded.contains(&group) && mark != Mark::Waits,
            lines,
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
                    .any(|l| l.mark == Mark::Waits);
        }
        if !search.is_empty() {
            section.folded = false;
            section.lines.retain(|line| matches(line, &search));
        }
    }
    sections.retain(|section| !section.lines.is_empty());
    sections
}

/// What pressing a row does.
fn go(world: &mut World, target: Target) {
    match target {
        Target::Home => show(world, Showing::Home),
        Target::Hand => talk_to_hand(world),
        Target::Agent(agent) => talk_to(world, agent),
        Target::Hamlet(hamlet) => {
            show(world, Showing::Hamlet(hamlet));
            look_at(world, Thing::Common(hamlet));
        }
        // A scholar's house, or its place on the pier, and the
        // library are chosen and looked at in the world too.
        Target::Scholar(scholar) => {
            show(world, Showing::Scholar(scholar));
            look_at(world, Thing::Scholar(scholar));
        }
        Target::Library => {
            show(world, Showing::Library);
            look_at(world, Thing::Library);
        }
        Target::Gone(agent) => show(world, Showing::Gone(agent)),
        Target::Petitions => show_tab(world, PETITIONS),
    }
}

/// Chooses `thing` in the world and brings the camera to it, where
/// there is a world.
fn look_at(world: &mut World, thing: Thing) {
    if world.contains_resource::<Selected>() {
        select(world, Some(thing));
        focus_on(world, thing, false);
    }
}

fn row_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, _| {
            f.fill(Color::NONE)
                .radius(4.0)
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::Stretch)
                .padding(UiRect::axes(px(6.0), px(0.0)))
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.button)))
        .pressed(|s| s.frame(|f, t| f.fill(t.palette.input)))
        .transition(Motion::Interact)
        .apply(cx);
}

fn current_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.button)
                .radius(4.0)
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::Stretch)
                .padding(UiRect::axes(px(6.0), px(0.0)))
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.button)))
        .transition(Motion::Interact)
        .apply(cx);
}

/// The lines that join a nested row to the agent above it.
fn branch(last: bool, depth: u8) -> impl View {
    with_cx(move |cx| {
        let line = cx.theme().palette.gold_6;
        let half = ROW / 2.0;
        let bar = |on: bool| {
            frame().width(px(1.0)).height(px(half)).fill(if on {
                line
            } else {
                Color::NONE
            })
        };
        let elbow = row((
            column((bar(true), bar(!last))).gap(0.0),
            column((frame()
                .width(px(7.0))
                .height(px(1.0))
                .fill(line),))
            .height(px(ROW))
            .justify(JustifyContent::Center),
        ))
        .gap(0.0);
        let steps = depth.min(DEEPEST).saturating_sub(1);
        row((frame().width(px(f32::from(steps) * STEP_IN)), elbow))
            .gap(0.0)
    })
}

fn mark_icon(mark: Mark) -> AnyView {
    match mark.shown() {
        Some((which, tone)) => glyph(which, ICON).tone(tone).boxed(),
        None => frame().width(px(ICON)).boxed(),
    }
}

fn expanded(line: &Line) -> AnyView {
    let target = line.target;
    let name_tone = if line.current {
        Tone::Accent
    } else {
        Tone::Body
    };
    let content = row((
        glyph(line.glyph, ICON).tone(Tone::Dim),
        column((
            label(line.name.clone())
                .tone(name_tone)
                .bold(true)
                .wrap(false),
            label(line.title.clone()).tone(Tone::Dim).wrap(false),
        ))
        .gap(0.0)
        .grow(1.0)
        .min_width(px(0.0))
        .overflow(Overflow::clip()),
        // How long ago, beside its state.
        row((
            label(line.age.clone()).tone(Tone::Faint).wrap(false),
            mark_icon(line.mark),
        ))
        .gap(4.0)
        .align(AlignItems::Center),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .height(px(ROW))
    .width(percent(100.0));
    let rules: fn(&mut Cx) = if line.current {
        current_rules
    } else {
        row_rules
    };
    let address = line.address.clone();
    let shown = button(content)
        .grow(1.0)
        .min_width(px(0.0))
        .rules(rules)
        .tagged(Variant::Default)
        .on_activate(move |world| go(world, target));
    let shown = if address.is_empty() || address == line.name {
        shown.boxed()
    } else {
        shown.tooltip(move || note(address.clone())).boxed()
    };
    // The tree's lines, then the chevron of a lead, outside the row's
    // press so that folding does not open the thread.
    let mut around = Vec::new();
    if line.depth > 0 {
        around.push(branch(line.last, line.depth).boxed());
    }
    around.push(match line.fold {
        Some((group, shut)) => button(
            glyph(Glyph::Chevron, 10.0)
                .tone(Tone::Faint)
                .rotation(if shut { 90.0 } else { 180.0 }),
        )
        .width(px(CHEVRON))
        .padding(UiRect::all(px(0.0)))
        .rules(views::tint)
        .tooltip(move || {
            note(if shut {
                "Show the hires under it"
            } else {
                "Fold the hires under it"
            })
        })
        .on_activate(move |world| {
            world.resource_mut::<Rail>().toggle(group);
        })
        .boxed(),
        None => frame().width(px(CHEVRON)).boxed(),
    });
    around.push(shown);
    row(around)
        .gap(0.0)
        .align(AlignItems::Center)
        .width(percent(100.0))
        .boxed()
}

fn icon_only(line: &Line) -> AnyView {
    let target = line.target;
    let rules: fn(&mut Cx) = if line.current {
        current_rules
    } else {
        row_rules
    };
    let mark: AnyView = match line.mark.shown() {
        Some((which, tone)) => glyph(which, 10.0).tone(tone).boxed(),
        None => frame().height(px(10.0)).boxed(),
    };
    let name = line.name.clone();
    button(
        column((
            glyph(line.glyph, ICON + 2.0).tone(if line.current {
                Tone::Accent
            } else {
                Tone::Body
            }),
            mark,
        ))
        .gap(2.0)
        .align(AlignItems::Center),
    )
    .width(percent(100.0))
    .tooltip(move || note(name.clone()))
    .rules(rules)
    .tagged(Variant::Default)
    .on_activate(move |world| go(world, target))
    .boxed()
}

/// A section's header: a chevron that folds it, its title, which
/// opens its page where it has one, its count and its state.
fn header(section: &Section) -> AnyView {
    let group = section.group;
    let mut parts = Vec::new();
    let foldable = !matches!(group, Group::Needs);
    if foldable {
        parts.push(
            button(
                glyph(Glyph::Chevron, 10.0)
                    .tone(Tone::Faint)
                    // Down while open, to the right while folded.
                    .rotation(if section.folded {
                        90.0
                    } else {
                        180.0
                    }),
            )
            .padding(UiRect::all(px(2.0)))
            .rules(views::tint)
            .tooltip(|| note("Fold or open"))
            .on_activate(move |world| {
                world.resource_mut::<Rail>().toggle(group);
            })
            .boxed(),
        );
    }
    let title = caption(section.title.clone(), Tone::Faint);
    parts.push(match section.opens {
        Some(target) => button(title)
            .padding(UiRect::all(px(0.0)))
            .rules(views::tint)
            .on_activate(move |world| go(world, target))
            .boxed(),
        None => title.boxed(),
    });
    parts.push(
        label(section.count.to_string()).tone(Tone::Dim).boxed(),
    );
    parts.push(frame().grow(1.0).boxed());
    if matches!(group, Group::Hamlet(_)) {
        parts.push(mark_icon(section.mark));
    }
    row(parts)
        .gap(6.0)
        .align(AlignItems::Center)
        .padding(UiRect::new(px(2.0), px(8.0), px(10.0), px(2.0)))
        .boxed()
}

fn sections_view(icons: bool, sections: &[Section]) -> AnyView {
    let mut parts = Vec::new();
    for (at, section) in sections.iter().enumerate() {
        if icons {
            if at > 0 {
                parts.push(
                    with_cx(|cx| hairline(cx.theme().palette.gold_6))
                        .boxed(),
                );
            }
            // Icons have no room for a tree: a hamlet is its own
            // icon, marked with the state of all of it.
            if let (Group::Hamlet(_), Some(target)) =
                (section.group, section.opens)
            {
                parts.push(icon_only(&Line {
                    target,
                    glyph: Glyph::Hamlet,
                    name: section.title.clone(),
                    title: String::new(),
                    age: String::new(),
                    mark: section.mark,
                    address: String::new(),
                    depth: 0,
                    last: true,
                    current: false,
                    fold: None,
                }));
                continue;
            }
        } else if !section.title.is_empty() {
            parts.push(header(section));
        }
        if section.folded {
            continue;
        }
        for line in &section.lines {
            parts.push(if icons {
                icon_only(line)
            } else {
                expanded(line)
            });
        }
    }
    column(parts).gap(1.0).boxed()
}

/// A button that turns the rail between its rows and its icons.
fn fold(icons: bool) -> impl View {
    button(glyph(if icons { Glyph::Into } else { Glyph::Menu }, ICON))
        .padding(UiRect::axes(px(8.0), px(0.0)))
        .min_height(px(hamlet_ui::styles::CONTROL))
        .tooltip(move || {
            note(if icons {
                "Show the rows"
            } else {
                "Show only the icons"
            })
        })
        .rules(hamlet_ui::styles::button)
        .tagged(Variant::Default)
        .on_activate(|world| world.resource_mut::<Rail>().fold())
}

fn expanded_head() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        row((
            glyph(Glyph::Lens, ICON).tone(Tone::Faint),
            text_field(String::new(), |_, _| {})
                .grow(1.0)
                .min_width(px(0.0))
                .rules(text_input)
                .tagged(Search),
            fold(false),
        ))
        .gap(gap)
        .align(AlignItems::Center)
    })
}

/// The rail down the Hub's left side.
pub(super) fn view() -> impl View {
    keyed(resource::<Rail, _>(Rail::is_icons), |icons| {
        let icons = *icons;
        let body = keyed(
            derived(|world: &World| sections(world))
                .when(every_frame()),
            move |sections| sections_view(icons, sections),
        )
        .within(hamlet_ui::styles::bare());
        let head: AnyView = if icons {
            fold(true).boxed()
        } else {
            expanded_head().boxed()
        };
        with_cx(move |cx| {
            column((head, scroll_area((body,))))
                .gap(cx.theme().space.gap)
                .width(resource::<Rail, _>(|rail| px(rail.shown())))
                .min_width(resource::<Rail, _>(|rail| {
                    px(rail.shown())
                }))
                .shrink(0.0)
                .fills_height()
        })
        .boxed()
    })
    .within(hamlet_ui::styles::filling())
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
        let ada = named(app.world_mut(), "ada@home").unwrap();
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
        state.revision += 1;
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert_eq!(
            names(&app, |g| *g == Group::Needs),
            ["ada · home"]
        );
        assert_eq!(names(&app, in_hamlet), ["ada"]);
    }
}
