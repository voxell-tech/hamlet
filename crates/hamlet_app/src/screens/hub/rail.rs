//! The Hub's rail: the realm's agents grouped by what they are
//! doing, over its scholars, the library and those that left.

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, ComputedNode, JustifyContent, Overflow, UiRect,
    percent, px,
};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, label, row, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource, style,
};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::realm::{
    Activity, Busy, Doing, Faulted, HamletId, Identity, Kind, Left,
    Lived, LivesIn, MergedInto, Parent, Purpose, Scholar, Skills,
    name_of,
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
use crate::hover::short;
use crate::status::now_ms;

/// The width of the rail with its rows, and with only their icons.
const WIDE: f32 = 232.0;
const ICONS: f32 = 46.0;

/// The width of the Hub under which the rail shows only icons.
const NARROW: f32 = 640.0;

/// The most that left a rail shows.
const LEFT_SHOWN: usize = 6;

/// The letters of a name and of a thread's title a row shows.
const NAME_SHOWN: usize = 22;
const TITLE_SHOWN: usize = 22;

/// How tall a row is, so that the lines of a branch meet.
const ROW: f32 = 44.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Rail>()
        .add_systems(Update, (read_search, measure));
}

/// The rail's search, filter and width.
#[derive(Resource, Default, PartialEq)]
pub struct Rail {
    search: String,
    hamlet: Option<Entity>,
    /// What the chief chose, over what the Hub's width says.
    chosen: Option<bool>,
    narrow: bool,
}

impl Rail {
    /// Keeps the rail showing its rows, however narrow the Hub is.
    pub fn show_rows(&mut self) {
        self.chosen = Some(false);
    }

    fn is_icons(&self) -> bool {
        self.chosen.unwrap_or(self.narrow)
    }
}

/// The root of the Hub, whose width says whether the rail is
/// narrow.
#[derive(Component)]
pub(super) struct Frame;

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

fn measure(
    frames: Query<&ComputedNode, With<Frame>>,
    mut rail: ResMut<Rail>,
) {
    let Some(node) = frames.iter().next() else {
        return;
    };
    let width = node.size().x * node.inverse_scale_factor();
    // A frame not laid out yet is as wide as nothing.
    if width <= 1.0 {
        return;
    }
    let narrow = width < NARROW;
    if rail.narrow != narrow {
        rail.narrow = narrow;
    }
}

/// A part of the rail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Group {
    Hand,
    Needs,
    Working,
    Resting,
    Scholars,
    Harbor,
    Library,
    Left,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Self::Hand => "",
            Self::Needs => "NEEDS YOU",
            Self::Working => "WORKING",
            Self::Resting => "RESTING",
            Self::Scholars => "SCHOLARS",
            Self::Harbor => "AT THE HARBOR",
            Self::Library => "LIBRARY",
            Self::Left => "LEFT",
        }
    }
}

/// What a row leads to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Target {
    Hand,
    Agent(Entity),
    Scholar(Entity),
    Library,
    Gone(Entity),
}

/// The state shown at a row's end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mark {
    Waits,
    Works,
    Rests,
    Faulted,
    Gone,
}

impl Mark {
    fn shown(self) -> (Glyph, Tone) {
        match self {
            Self::Waits => (Glyph::Bell, Tone::Critical),
            Self::Works => (Glyph::Bolt, Tone::Accent),
            Self::Rests => (Glyph::Moon, Tone::Dim),
            Self::Faulted => (Glyph::Stop, Tone::Critical),
            Self::Gone => (Glyph::Gone, Tone::Faint),
        }
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
    /// How far under another agent it is nested.
    pub depth: u8,
    /// Whether no row after it is nested under the same agent.
    pub last: bool,
    pub current: bool,
}

/// A group and its rows.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Section {
    pub group: Group,
    pub lines: Vec<Line>,
}

/// An agent as the rail reads it.
struct Agent {
    entity: Entity,
    id: protocol::AgentId,
    kind: protocol::AgentKind,
    parent: Option<protocol::AgentId>,
    hamlet: Option<Entity>,
    mark: Mark,
    awaiting: bool,
    name: String,
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

/// The first line of `text`, cut short.
fn first_line(text: &str, most: usize) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty());
    short(line.unwrap_or("").trim(), most)
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
                Some(first_line(text, TITLE_SHOWN))
            }
            _ => None,
        });
    (said, items.last().map_or(0, |item| item.at))
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
        (Has<Busy>, Has<Faulted>),
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
    let living = world
        .try_query_filtered::<Entity, bevy::ecs::query::Without<Left>>()
        .map(|mut living| living.iter(world).collect::<Vec<_>>())
        .unwrap_or_default();
    query
        .iter(world)
        .filter(|(entity, ..)| living.contains(entity))
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
                (busy, faulted),
            )| {
                let name = name_of(world, entity);
                let (said, last) = thread(world, id.0);
                let mark = if waiting.contains(&name) {
                    Mark::Waits
                } else if faulted {
                    Mark::Faulted
                } else if busy {
                    Mark::Works
                } else {
                    Mark::Rests
                };
                let title = said
                    .or_else(|| {
                        purpose
                            .map(|p| first_line(&p.0, TITLE_SHOWN))
                            .filter(|line| !line.is_empty())
                    })
                    .unwrap_or_else(|| {
                        short(&activity.words, TITLE_SHOWN)
                    });
                Agent {
                    entity,
                    id: id.0,
                    kind: kind.0,
                    parent: parent.map(|parent| parent.0),
                    hamlet: lives.map(|lives| lives.0),
                    mark,
                    awaiting: busy && activity.doing == Doing::Await,
                    name,
                    title,
                    age: age(now, last.max(lived.since)),
                }
            },
        )
        .collect()
}

/// The agents nested under `agent`: workers it started, and hires
/// it waits on.
fn nested<'a>(
    agent: &'a Agent,
    all: &'a [Agent],
) -> impl Iterator<Item = &'a Agent> {
    all.iter().filter(move |other| {
        other.parent == Some(agent.id)
            && (other.kind == protocol::AgentKind::Worker
                || agent.awaiting)
    })
}

/// The agent and everyone nested under it, deepest last.
fn tree<'a>(
    agent: &'a Agent,
    all: &'a [Agent],
) -> Vec<(&'a Agent, u8)> {
    fn walk<'a>(
        agent: &'a Agent,
        all: &'a [Agent],
        depth: u8,
        out: &mut Vec<(&'a Agent, u8)>,
    ) {
        out.push((agent, depth));
        if depth < 3 {
            for child in nested(agent, all) {
                walk(child, all, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(agent, all, 0, &mut out);
    out
}

fn kind_glyph(kind: protocol::AgentKind) -> Glyph {
    match kind {
        protocol::AgentKind::Hand => Glyph::Mayor,
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
        | (Target::Gone(a), Some(Showing::Gone(b))) => a == b,
        (Target::Library, Some(Showing::Library)) => true,
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
        glyph: kind_glyph(agent.kind),
        name: if hand {
            "The Hand".into()
        } else {
            short(&agent.name, NAME_SHOWN)
        },
        title: agent.title.clone(),
        age: agent.age.clone(),
        mark: agent.mark,
        depth,
        last,
        current: is_current(world, target),
    }
}

/// Whether `line` matches what is typed in the search.
fn matches(line: &Line, search: &str) -> bool {
    search.is_empty()
        || line.name.to_lowercase().contains(search)
        || line.title.to_lowercase().contains(search)
}

/// The scholars, the library and those that left, after the
/// agents.
fn society(world: &World, search: &str, sections: &mut Vec<Section>) {
    let mut housed = Vec::new();
    let mut harbor = Vec::new();
    let mut gone = Vec::new();
    let hires_working = |scholar: &protocol::Scholar| {
        scholar.hires.iter().any(|hire| {
            hamlet_client::realm::agent(world, *hire)
                .is_some_and(|hire| world.get::<Busy>(hire).is_some())
        })
    };
    if let Some(mut query) = world.try_query::<(Entity, &Scholar)>() {
        let mut found =
            query.iter(world).collect::<Vec<(Entity, &Scholar)>>();
        found.sort_by_key(|(_, scholar)| {
            scholar.0.name.to_lowercase()
        });
        for (entity, scholar) in found {
            let scholar = &scholar.0;
            let hires = scholar
                .hires
                .iter()
                .filter(|hire| {
                    hamlet_client::realm::agent(world, **hire)
                        .is_some_and(|hire| {
                            hamlet_client::realm::id_of(world, hire)
                                .is_some()
                        })
                })
                .count();
            let target = Target::Scholar(entity);
            let (title, mark) = match (&scholar.left, scholar.arrival)
            {
                (Some(_), _) => ("Left the city".into(), Mark::Gone),
                (None, protocol::Arrival::Waiting) => {
                    ("Waits to be let in".into(), Mark::Waits)
                }
                (None, protocol::Arrival::Turned) => {
                    ("Turned away".into(), Mark::Gone)
                }
                (None, protocol::Arrival::Housed) => (
                    match hires {
                        0 => "Not hired".to_string(),
                        1 => "1 hire".to_string(),
                        n => format!("{n} hires"),
                    },
                    if hires_working(scholar) {
                        Mark::Works
                    } else {
                        Mark::Rests
                    },
                ),
            };
            let line = Line {
                target,
                glyph: Glyph::Hamlet,
                name: short(&scholar.name, NAME_SHOWN),
                title,
                age: String::new(),
                mark,
                depth: 0,
                last: true,
                current: is_current(world, target),
            };
            if !matches(&line, search) {
                continue;
            }
            match (&scholar.left, scholar.arrival) {
                (Some(_), _) => gone.push(line),
                (None, protocol::Arrival::Housed) => {
                    housed.push(line);
                }
                (None, _) => harbor.push(line),
            }
        }
    }
    sections.push(Section {
        group: Group::Scholars,
        lines: housed,
    });
    sections.push(Section {
        group: Group::Harbor,
        lines: harbor,
    });
    let shelf =
        world.get_resource::<Skills>().map_or(0, |s| s.0.len());
    sections.push(Section {
        group: Group::Library,
        lines: vec![Line {
            target: Target::Library,
            glyph: Glyph::Book,
            name: "Library".into(),
            title: match shelf {
                1 => "1 skill".into(),
                n => format!("{n} skills"),
            },
            age: String::new(),
            mark: Mark::Rests,
            depth: 0,
            last: true,
            current: is_current(world, Target::Library),
        }],
    });
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
            let target = Target::Gone(entity);
            let line = Line {
                target,
                glyph: Glyph::Robot,
                name: short(&name_of(world, entity), NAME_SHOWN),
                title: match into {
                    Some(host) => {
                        format!(
                            "Merged into {}",
                            name_of(world, host.0)
                        )
                    }
                    None => "Removed".into(),
                },
                age: String::new(),
                mark: Mark::Gone,
                depth: 0,
                last: true,
                current: is_current(world, target),
            };
            if matches(&line, search) {
                gone.push(line);
            }
        }
    }
    sections.push(Section {
        group: Group::Left,
        lines: gone,
    });
}

/// The rail's rows in their groups: the Hand first, then the agents
/// by what they are doing, then the society. A group with no row is
/// left out.
pub(super) fn sections(world: &World) -> Vec<Section> {
    let (search, hamlet) = world.get_resource::<Rail>().map_or_else(
        || (String::new(), None),
        |rail| (rail.search.trim().to_lowercase(), rail.hamlet),
    );
    let all = agents(world);
    let mut hand = Vec::new();
    let (mut needs, mut working, mut resting) =
        (Vec::new(), Vec::new(), Vec::new());
    for agent in &all {
        if agent.kind == protocol::AgentKind::Hand {
            hand.push(agent_line(world, agent, 0, true));
            continue;
        }
        // Nested under the agent that waits on it.
        let under = all.iter().any(|other| {
            Some(other.id) == agent.parent
                && nested(other, &all).any(|n| n.id == agent.id)
        });
        if under || hamlet.is_some_and(|h| agent.hamlet != Some(h)) {
            continue;
        }
        let family = tree(agent, &all);
        let lines = family
            .iter()
            .enumerate()
            .map(|(at, (member, depth))| {
                let last = family.get(at + 1..).is_none_or(|after| {
                    after.iter().all(|(_, d)| d < depth)
                });
                agent_line(world, member, *depth, last)
            })
            .collect::<Vec<_>>();
        if !lines.iter().any(|line| matches(line, &search)) {
            continue;
        }
        let marks = lines.iter().map(|line| line.mark);
        let into = if marks.clone().any(|m| m == Mark::Waits) {
            &mut needs
        } else if marks.clone().any(|m| m == Mark::Works) {
            &mut working
        } else {
            &mut resting
        };
        into.extend(lines);
    }
    let mut sections = vec![
        Section {
            group: Group::Hand,
            lines: hand,
        },
        Section {
            group: Group::Needs,
            lines: needs,
        },
        Section {
            group: Group::Working,
            lines: working,
        },
        Section {
            group: Group::Resting,
            lines: resting,
        },
    ];
    society(world, &search, &mut sections);
    sections.retain(|section| !section.lines.is_empty());
    sections
}

/// What pressing a row does.
fn go(world: &mut World, target: Target) {
    match target {
        Target::Hand => talk_to_hand(world),
        Target::Agent(agent) => talk_to(world, agent),
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
        row((
            frame()
                .width(px(f32::from(depth.saturating_sub(1)) * 12.0)),
            elbow,
        ))
        .gap(0.0)
    })
}

fn mark_icon(mark: Mark) -> impl View {
    let (which, tone) = mark.shown();
    glyph(which, ICON).tone(tone)
}

fn expanded(line: &Line) -> AnyView {
    let target = line.target;
    let name_tone = if line.current {
        Tone::Accent
    } else {
        Tone::Body
    };
    let mut parts = Vec::new();
    if line.depth > 0 {
        parts.push(branch(line.last, line.depth).boxed());
    }
    parts.push(glyph(line.glyph, ICON).tone(Tone::Dim).boxed());
    parts.push(
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
        .overflow(Overflow::clip())
        .boxed(),
    );
    parts.push(
        column((
            label(line.age.clone()).tone(Tone::Faint).wrap(false),
            mark_icon(line.mark),
        ))
        .gap(2.0)
        .align(AlignItems::FlexEnd)
        .boxed(),
    );
    let content = row(parts)
        .gap(8.0)
        .align(AlignItems::Center)
        .height(px(ROW))
        .width(percent(100.0));
    let rules: fn(&mut Cx) = if line.current {
        current_rules
    } else {
        row_rules
    };
    button(content)
        .width(percent(100.0))
        .rules(rules)
        .tagged(Variant::Default)
        .on_activate(move |world| go(world, target))
        .boxed()
}

fn icon_only(line: &Line) -> AnyView {
    let target = line.target;
    let (which, tone) = line.mark.shown();
    let rules: fn(&mut Cx) = if line.current {
        current_rules
    } else {
        row_rules
    };
    button(
        column((
            glyph(line.glyph, ICON + 2.0).tone(if line.current {
                Tone::Accent
            } else {
                Tone::Body
            }),
            glyph(which, 10.0).tone(tone),
        ))
        .gap(2.0)
        .align(AlignItems::Center),
    )
    .width(percent(100.0))
    .rules(rules)
    .tagged(Variant::Default)
    .on_activate(move |world| go(world, target))
    .boxed()
}

fn sections_view(icons: bool, sections: &[Section]) -> AnyView {
    let mut parts = Vec::new();
    for (at, section) in sections.iter().enumerate() {
        if at > 0 && icons {
            parts.push(
                with_cx(|cx| hairline(cx.theme().palette.gold_6))
                    .boxed(),
            );
        }
        if !icons && !section.group.title().is_empty() {
            let count = section
                .lines
                .iter()
                .filter(|line| line.depth == 0)
                .count();
            parts.push(
                row((
                    caption(section.group.title(), Tone::Faint),
                    label(count.to_string()).tone(Tone::Dim),
                ))
                .gap(6.0)
                .padding(UiRect::new(
                    px(6.0),
                    px(0.0),
                    px(8.0),
                    px(2.0),
                ))
                .boxed(),
            );
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
        .on_activate(|world| {
            let mut rail = world.resource_mut::<Rail>();
            rail.chosen = Some(!rail.is_icons());
        })
}

/// The dropdown that narrows the rail to one hamlet.
fn hamlets() -> impl View {
    keyed(
        derived(|world: &World| {
            let mut found = world
                .try_query::<(Entity, &HamletId)>()
                .map(|mut query| {
                    query
                        .iter(world)
                        .map(|(hamlet, _)| {
                            (hamlet, name_of(world, hamlet))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            found.sort_by_key(|(_, name)| name.to_lowercase());
            let now = world.resource::<Rail>().hamlet;
            (found, now)
        })
        .when(every_frame()),
        |(found, now)| {
            let mut names = vec!["All hamlets".to_string()];
            names.extend(found.iter().map(|(_, name)| name.clone()));
            let at = now
                .and_then(|now| {
                    found
                        .iter()
                        .position(|(hamlet, _)| *hamlet == now)
                })
                .map_or(0, |at| at + 1);
            let hamlets = found
                .iter()
                .map(|(hamlet, _)| *hamlet)
                .collect::<Vec<_>>();
            dropdown(
                names,
                at,
                Glyph::Chevron.image(ICON),
                move |world, chosen| {
                    let hamlet = chosen
                        .checked_sub(1)
                        .and_then(|at| hamlets.get(at).copied());
                    world.resource_mut::<Rail>().hamlet = hamlet;
                },
            )
            .boxed()
        },
    )
    .within(hamlet_ui::styles::bare())
}

fn expanded_head() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        column((
            row((
                text_field(String::new(), |_, _| {})
                    .grow(1.0)
                    .min_width(px(0.0))
                    .rules(text_input)
                    .tagged(Search),
                fold(false),
            ))
            .gap(gap)
            .align(AlignItems::Center),
            hamlets(),
        ))
        .gap(gap)
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
                .width(px(if icons { ICONS } else { WIDE }))
                .min_width(px(if icons { ICONS } else { WIDE }))
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

    fn groups(app: &mut App) -> Vec<(Group, Vec<String>)> {
        sections(app.world())
            .into_iter()
            .map(|section| {
                (
                    section.group,
                    section
                        .lines
                        .into_iter()
                        .map(|line| line.name)
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn the_rail_groups_agents_by_what_they_wait_for_and_do() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        app.update();
        let ada = named(app.world_mut(), "ada@home").unwrap();
        let at = |groups: &[(Group, Vec<String>)], group| {
            groups
                .iter()
                .find(|(of, _)| *of == group)
                .map(|(_, names)| names.clone())
                .unwrap_or_default()
        };
        let rest = groups(&mut app);
        assert_eq!(at(&rest, Group::Hand), ["The Hand"]);
        assert_eq!(at(&rest, Group::Resting), ["ada@home"]);
        assert_eq!(at(&rest, Group::Scholars), ["ada"]);

        app.world_mut().entity_mut(ada).insert(Busy);
        let working = groups(&mut app);
        assert_eq!(at(&working, Group::Working), ["ada@home"]);
        assert!(at(&working, Group::Resting).is_empty());

        let mut state = hamlet_testing::realm::state();
        state.waits.push(hamlet_testing::realm::wait("ask"));
        state.revision += 1;
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        let waiting = groups(&mut app);
        assert_eq!(at(&waiting, Group::Needs), ["ada@home"]);
    }
}
