//! The terminals beside the screen, in one flat list by hamlet, the
//! Hand's, the workers' of no hamlet, and the chief's own, whose
//! groups fold away or open. An agent's log of the commands it ran
//! shows only once the agent is pinned. Each row shows where its
//! terminal stands as a breadcrumb, the last command it ran, and
//! whether one runs. A double click names one.
use std::collections::{BTreeMap, BTreeSet};

use bevy::app::{App, PostUpdate, PreUpdate};
use bevy::color::{Alpha, Color};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::events::PointerClick;
use bevy::picking::pointer::PointerButton;
use bevy::text::{EditableText, TextEdit};
use bevy::ui::{
    AlignItems, JustifyContent, Overflow, UiRect, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm;
use hamlet_client::terminals::{self, Terminal, TerminalKind};
use hamlet_protocol::client::{AgentId, AgentKind};
use hamlet_ui::styles::{
    Fills, ICON, caption, close_button, hairline, link_row, note,
    scroll_area, text_input,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, View, with_cx};

use super::{Folds, Handing, PanelId, Panels, Pins, activate};
use crate::PanelSystems;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Listed>()
        .init_resource::<Renaming>()
        .add_observer(start_rename)
        .add_systems(PreUpdate, list.in_set(PanelSystems::Bind))
        // After the views mount and after the screen takes the keys
        // a click gave it, so the new field keeps them.
        .add_systems(PostUpdate, (focus_field, end_rename).chain());
}

/// The terminals as the list shows them.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub(super) struct Listed(pub(super) Vec<Group>);

/// The terminal whose name is being typed, in the list of which
/// panel, and whether its field has had the keys yet.
#[derive(Resource, Default, Clone, PartialEq)]
struct Renaming(Option<(Entity, PanelId, bool)>);

/// A terminal's row in a panel's list, which a double click names.
#[derive(Component)]
pub(super) struct Entry(pub(super) Entity, pub(super) PanelId);

/// The field a terminal's name is typed in.
#[derive(Component)]
struct RenameField;

/// The terminals of one hamlet, the Hand's, the workers' of no
/// hamlet, or the unbounded ones, as rows of a tree.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Group {
    /// The hamlet's id, [`HAND`], [`WORKERS`], or empty for the
    /// unbounded ones.
    pub(super) key: String,
    pub(super) title: String,
    pub(super) rows: Vec<Row>,
}

/// The key of the Hand's group.
pub(super) const HAND: &str = "~hand";
/// The key of the group of workers of no hamlet a chief started.
pub(super) const WORKERS: &str = "~workers";

/// A row of a group's tree: an agent, or a terminal.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Row {
    /// The keys of the agents above it, the outermost first.
    pub(super) path: Vec<String>,
    pub(super) what: What,
}

#[derive(Clone, PartialEq, Debug)]
pub(super) enum What {
    /// An agent with terminals of its own or under it.
    Agent(Branch),
    Terminal(Item),
}

#[derive(Clone, PartialEq, Debug)]
pub(super) struct Branch {
    /// What folds it: `agent#<id>`.
    pub(super) key: String,
    /// Its address, which comes first.
    pub(super) address: String,
    /// Its name, when it has one besides its address.
    pub(super) name: Option<String>,
    pub(super) kind: AgentKind,
    pub(super) mayor: bool,
    /// The log of the commands it ran, which the list shows once the
    /// agent is pinned.
    pub(super) log: Option<Item>,
    /// The shells under it, its own and its agents'.
    pub(super) held: usize,
}

#[derive(Clone, PartialEq, Debug)]
pub(super) struct Item {
    pub(super) terminal: Entity,
    pub(super) name: String,
    /// The agent's address, or "chief".
    pub(super) owner: String,
    pub(super) exit: Option<i64>,
    pub(super) commands: bool,
    /// What pins its agent's log: `agent#<id>`.
    pub(super) key: Option<String>,
    /// The last command it ran.
    pub(super) last: Option<String>,
    /// Whether a command is running in it.
    pub(super) running: bool,
    /// What it is under: the hamlet and the agents, the outermost
    /// first. Empty for the chief's own shells.
    pub(super) crumbs: Vec<String>,
}

impl Listed {
    pub(super) fn find(&self, terminal: Entity) -> Option<&Item> {
        self.items().find(|item| item.terminal == terminal)
    }

    pub(super) fn items(&self) -> impl Iterator<Item = &Item> {
        self.0.iter().flat_map(|group| &group.rows).filter_map(
            |row| match &row.what {
                What::Terminal(item) => Some(item),
                What::Agent(branch) => branch.log.as_ref(),
            },
        )
    }
}

/// An agent as the tree places it.
#[derive(Clone, Debug)]
pub(super) struct Owner {
    pub(super) kind: AgentKind,
    pub(super) parent: Option<AgentId>,
    pub(super) hamlet: Option<String>,
    pub(super) mayor: bool,
    pub(super) address: String,
    pub(super) name: String,
}

/// A terminal to place: its row, its agent and its hamlet.
pub(super) struct Placed {
    pub(super) item: Item,
    pub(super) agent: Option<AgentId>,
    pub(super) hamlet: Option<String>,
    /// What orders it among its agent's or the chief's.
    pub(super) order: (bool, String, String),
}

/// Lists the terminals by hamlet, then the Hand's, the workers' of no
/// hamlet and the unbounded ones, each as a tree of the agents they
/// are under.
pub(super) fn list(world: &mut World) {
    let Some(views) =
        world.try_query::<(Entity, &Terminal)>().map(|mut query| {
            query
                .iter(world)
                .map(|(entity, view)| (entity, view.0.clone()))
                .collect::<Vec<_>>()
        })
    else {
        return;
    };
    let mut owners = BTreeMap::new();
    let mut placed = Vec::new();
    for (entity, view) in views {
        let mut next = view.agent;
        while let Some(agent) =
            next.filter(|a| !owners.contains_key(a))
        {
            let found = owner(world, agent);
            next = found.as_ref().and_then(|found| found.parent);
            if let Some(found) = found {
                owners.insert(agent, found);
            }
        }
        let commands = view.kind == TerminalKind::Commands;
        // A name the chief gave shows over what programs set.
        let name = if view.named {
            view.name.clone()
        } else if commands {
            "bash".to_owned()
        } else if view.title.is_empty() {
            view.name.clone()
        } else {
            view.title.clone()
        };
        let address = view.agent.map(|agent| {
            owners.get(&agent).map_or_else(
                || format!("agent#{}", agent.0),
                |found| found.address.clone(),
            )
        });
        placed.push(Placed {
            item: Item {
                terminal: entity,
                name,
                owner: address.unwrap_or_else(|| "chief".into()),
                exit: view.exit,
                commands,
                key: view
                    .agent
                    .filter(|_| commands)
                    .map(|agent| format!("agent#{}", agent.0)),
                last: view.last.clone(),
                running: view.running,
                crumbs: Vec::new(),
            },
            agent: view.agent,
            hamlet: view.hamlet.clone(),
            order: (!commands, view.name.clone(), view.id.clone()),
        });
    }
    let titles = |id: &str| {
        realm::hamlet(world, id)
            .map(|h| realm::name_of(world, h))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| id.to_owned())
    };
    let listed = Listed(tree(placed, &owners, titles));
    if *world.resource::<Listed>() != listed {
        world.insert_resource(listed);
    }
}

/// `agent` as the realm knows it.
fn owner(world: &World, agent: AgentId) -> Option<Owner> {
    let entity = realm::agent(world, agent)?;
    let address = realm::name_of(world, entity);
    let name = realm::shown_of(world, entity);
    Some(Owner {
        kind: world.get::<realm::Kind>(entity)?.0,
        parent: world.get::<realm::Parent>(entity).map(|p| p.0),
        hamlet: world
            .get::<realm::LivesIn>(entity)
            .and_then(|lives| world.get::<realm::HamletId>(lives.0))
            .map(|hamlet| hamlet.0.clone()),
        mayor: world.get::<realm::Mayor>(entity).is_some(),
        address,
        name,
    })
}

/// The agent a tree puts `owner` under: a hire's lead or a worker's
/// starter. A mayor heads its hamlet's tree, though it answers to
/// the Hand.
fn above(
    owner: &Owner,
    owners: &BTreeMap<AgentId, Owner>,
) -> Option<AgentId> {
    let parent = owner.parent?;
    let hand = owners
        .get(&parent)
        .is_some_and(|parent| parent.kind == AgentKind::Hand);
    match owner.kind {
        AgentKind::Hand => None,
        AgentKind::Hire if hand => None,
        _ => Some(parent),
    }
}

/// The group of the agent at the top of a tree, `hamlet` being the
/// one of the terminal that brought it.
fn group_of(
    owner: Option<&Owner>,
    hamlet: Option<&String>,
) -> String {
    match owner {
        Some(owner) if owner.kind == AgentKind::Hand => {
            Some(HAND.into())
        }
        Some(owner) => owner.hamlet.clone(),
        None => hamlet.cloned(),
    }
    .unwrap_or_else(|| WORKERS.into())
}

/// The agents under an agent, and its terminals.
#[derive(Default)]
struct Node {
    terminals: Vec<Placed>,
    under: BTreeSet<AgentId>,
}

/// The groups of `placed`'s terminals, each a tree of the agents
/// they are under, with `owners` and the titles of the hamlets.
pub(super) fn tree(
    placed: Vec<Placed>,
    owners: &BTreeMap<AgentId, Owner>,
    titles: impl Fn(&str) -> String,
) -> Vec<Group> {
    let mut nodes = BTreeMap::<AgentId, Node>::new();
    let mut chiefs = BTreeMap::<String, Vec<Placed>>::new();
    let mut tops = BTreeMap::<String, BTreeSet<AgentId>>::new();
    for terminal in placed {
        let Some(agent) = terminal.agent else {
            let key = terminal.hamlet.clone().unwrap_or_default();
            chiefs.entry(key).or_default().push(terminal);
            continue;
        };
        let hamlet = terminal.hamlet.clone();
        let fresh = !nodes.contains_key(&agent);
        nodes.entry(agent).or_default().terminals.push(terminal);
        if !fresh {
            continue;
        }
        // Each agent above it joins the tree once.
        let mut at = agent;
        loop {
            let found = owners.get(&at);
            match found.and_then(|found| above(found, owners)) {
                Some(up) => {
                    let known = nodes.contains_key(&up);
                    nodes.entry(up).or_default().under.insert(at);
                    if known {
                        break;
                    }
                    at = up;
                }
                None => {
                    tops.entry(group_of(found, hamlet.as_ref()))
                        .or_default()
                        .insert(at);
                    break;
                }
            }
        }
    }
    let mut keys = chiefs
        .keys()
        .chain(tops.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| {
            let (rank, title) = match key.as_str() {
                "" => (3, "This computer".to_owned()),
                HAND => (1, "Hand".to_owned()),
                WORKERS => (2, "Workers".to_owned()),
                hamlet => (0, titles(hamlet)),
            };
            (rank, title, key)
        })
        .collect::<Vec<_>>();
    keys.sort();
    keys.into_iter()
        .map(|(_, title, key)| {
            let mut rows = Vec::new();
            let mut mine = chiefs.remove(&key).unwrap_or_default();
            mine.sort_by(|a, b| a.order.cmp(&b.order));
            rows.extend(mine.into_iter().map(|terminal| Row {
                path: Vec::new(),
                what: What::Terminal(terminal.item),
            }));
            let top = tops.remove(&key).unwrap_or_default();
            for agent in ordered(top, owners, true) {
                branch(
                    agent,
                    &mut nodes,
                    owners,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &mut rows,
                );
            }
            if !key.is_empty() {
                for row in &mut rows {
                    if let What::Agent(Branch {
                        log: Some(log),
                        ..
                    }) = &mut row.what
                    {
                        if log.crumbs.first() != Some(&title) {
                            log.crumbs.insert(0, title.clone());
                        }
                    }
                }
            }
            Group { key, title, rows }
        })
        .collect()
}

/// `agents` in the order a tree shows them: a chief's workers, then
/// the mayor, then hires in the order they came, then workers; or,
/// below an agent, its hires before its workers.
fn ordered(
    agents: BTreeSet<AgentId>,
    owners: &BTreeMap<AgentId, Owner>,
    top: bool,
) -> Vec<AgentId> {
    let mut agents = agents.into_iter().collect::<Vec<_>>();
    agents.sort_by_key(|agent| {
        let found = owners.get(agent);
        let rank = match found {
            Some(o) if o.kind == AgentKind::Worker && top => 0,
            Some(o) if o.mayor => 1,
            Some(o) if o.kind == AgentKind::Worker => 3,
            _ => 2,
        };
        (rank, agent.0)
    });
    agents
}

/// Adds the rows of `agent` and all under it, `path` being the agents
/// above it; gives how many terminals they hold.
fn branch(
    agent: AgentId,
    nodes: &mut BTreeMap<AgentId, Node>,
    owners: &BTreeMap<AgentId, Owner>,
    path: &mut Vec<String>,
    crumbs: &mut Vec<String>,
    rows: &mut Vec<Row>,
) -> usize {
    let Node { terminals, under } =
        nodes.remove(&agent).unwrap_or_default();
    let (logs, mut terminals): (Vec<_>, Vec<_>) =
        terminals.into_iter().partition(|t| t.item.commands);
    let found = owners.get(&agent);
    let key = format!("agent#{}", agent.0);
    let name = found.map(|o| o.name.clone()).filter(|name| {
        !name.is_empty() && Some(name) != found.map(|o| &o.address)
    });
    let address = found.map_or_else(
        || format!("agent#{}", agent.0),
        |o| o.address.clone(),
    );
    crumbs.push(name.clone().unwrap_or_else(|| address.clone()));
    let at = rows.len();
    rows.push(Row {
        path: path.clone(),
        what: What::Agent(Branch {
            key: key.clone(),
            address,
            name,
            kind: found.map_or(AgentKind::Worker, |o| o.kind),
            mayor: found.is_some_and(|o| o.mayor),
            log: logs.into_iter().next().map(|mut log| {
                log.item.crumbs = crumbs.clone();
                log.item
            }),
            held: 0,
        }),
    });
    path.push(key);
    terminals.sort_by(|a, b| a.order.cmp(&b.order));
    let mut held = terminals.len();
    rows.extend(terminals.into_iter().map(|terminal| Row {
        path: path.clone(),
        what: What::Terminal(terminal.item),
    }));
    for below in ordered(under, owners, false) {
        held += branch(below, nodes, owners, path, crumbs, rows);
    }
    path.pop();
    crumbs.pop();
    if let What::Agent(branch) = &mut rows[at].what {
        branch.held = held;
    }
    held
}

/// The list of terminals in `panel` and the press that opens the menu
/// of what to add.
pub(super) fn side(panel: &PanelId) -> impl View + use<> {
    let panel = panel.clone();
    with_cx(move |cx| {
        let space = &cx.theme().space;
        column((
            row((
                row((caption("TERMINALS".to_string(), Tone::Faint),))
                    .grow(1.0),
                button(glyph(Glyph::Plus, ICON).tone(Tone::Dim))
                    .padding(UiRect::all(px(4.0)))
                    .rules(link_row)
                    .tooltip(|| note("New terminal or pin a bash"))
                    .on_activate_with({
                        let panel = panel.clone();
                        move |world, node| {
                            super::pick::toggle(world, node, &panel)
                        }
                    }),
            ))
            .align(AlignItems::Center)
            .justify(JustifyContent::SpaceBetween)
            .padding(UiRect::new(
                px(space.section),
                px(space.gap),
                px(6.0),
                px(6.0),
            )),
            hairline(cx.theme().palette.border),
            scroll_area((keyed(
                derived({
                    let panel = panel.clone();
                    move |world: &World| {
                        let shown =
                            world.resource::<Panels>().get(&panel);
                        (
                            world.resource::<Listed>().clone(),
                            world.resource::<Pins>().clone(),
                            world
                                .resource::<Folds>()
                                .0
                                .get(&panel.0)
                                .cloned()
                                .unwrap_or_default(),
                            shown.and_then(|shown| shown.active),
                            world
                                .resource::<Renaming>()
                                .0
                                .as_ref()
                                .and_then(|(terminal, at, _)| {
                                    (*at == panel)
                                        .then_some(*terminal)
                                }),
                        )
                    }
                })
                .when(every_frame()),
                {
                    let panel = panel.clone();
                    move |(listed, pins, folded, active, renaming)| {
                        groups(
                            &panel, listed, pins, folded, *active,
                            *renaming,
                        )
                        .boxed()
                    }
                },
            )
            .within(column(()).gap(0.0)),)),
        ))
        .gap(0.0)
        .width(px(cx.theme().terminal.list))
        .shrink(0.0)
        .fill(cx.theme().palette.card)
    })
}

impl Listed {
    /// The terminals the list shows, by group: the chief's shells,
    /// and the logs of the agents `pins` holds.
    pub(super) fn shown(
        &self,
        pins: &Pins,
    ) -> Vec<(&Group, Vec<&Item>)> {
        self.0
            .iter()
            .map(|group| {
                let items = group
                    .rows
                    .iter()
                    .filter_map(|row| match &row.what {
                        What::Terminal(item) => Some(item),
                        What::Agent(branch) => branch
                            .log
                            .as_ref()
                            .filter(|_| pins.0.contains(&branch.key)),
                    })
                    .collect::<Vec<_>>();
                (group, items)
            })
            .filter(|(_, items)| !items.is_empty())
            .collect()
    }
}

fn groups(
    panel: &PanelId,
    listed: &Listed,
    pins: &Pins,
    folded: &BTreeSet<String>,
    active: Option<Entity>,
    renaming: Option<Entity>,
) -> impl View + use<> {
    let shown = Shown {
        panel,
        active,
        renaming,
    };
    let groups = listed
        .shown(pins)
        .into_iter()
        .enumerate()
        .map(|(at, (group, items))| {
            let shut = folded.contains(&group.key);
            let rows = if shut {
                Vec::new()
            } else {
                let count = items.len();
                items
                    .into_iter()
                    .enumerate()
                    .map(|(row, item)| {
                        entry(&shown, item.clone(), row + 1 == count)
                    })
                    .collect()
            };
            let held = group
                .rows
                .iter()
                .filter(|row| match &row.what {
                    What::Terminal(_) => true,
                    What::Agent(branch) => {
                        branch.log.is_some()
                            && pins.0.contains(&branch.key)
                    }
                })
                .count();
            let first = at == 0;
            let head = heading(panel, group, held, shut);
            with_cx(move |cx| {
                column((
                    hairline(if first {
                        Color::NONE
                    } else {
                        cx.theme().palette.border
                    }),
                    head,
                    column(rows)
                        .gap(0.0)
                        .padding(UiRect::axes(px(BOX_GAP), px(2.0))),
                ))
                .gap(0.0)
                .fill(cx.theme().palette.button.with_alpha(0.5))
            })
            .boxed()
        })
        .collect::<Vec<_>>();
    column(groups).gap(0.0).padding(UiRect::bottom(px(8.0)))
}

/// What a list draws its rows by.
struct Shown<'a> {
    panel: &'a PanelId,
    active: Option<Entity>,
    renaming: Option<Entity>,
}

/// How far the rows stand in from the list's edge.
const BOX_GAP: f32 = 4.0;

/// A group's title, which folds the group away or opens it, with how
/// many terminals it shows.
fn heading(
    panel: &PanelId,
    group: &Group,
    held: usize,
    shut: bool,
) -> impl View + use<> {
    let panel = panel.clone();
    let key = group.key.clone();
    let title = group.title.to_uppercase();
    let home = group.key.is_empty();
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let mark = if shut { Glyph::Into } else { Glyph::Down };
        let words = vec![
            glyph(mark, ICON).tone(Tone::Faint).boxed(),
            glyph(
                if home { Glyph::Globe } else { Glyph::Hamlet },
                ICON,
            )
            .tone(Tone::Dim)
            .boxed(),
            caption(title.clone(), Tone::Dim).boxed(),
            caption(held.to_string(), Tone::Faint).boxed(),
        ];
        let key = key.clone();
        let panel = panel.clone();
        button(
            row(words)
                .gap(4.0)
                .align(AlignItems::Center)
                .width(percent(100.0)),
        )
        .justify(JustifyContent::FlexStart)
        .padding(UiRect::new(
            px(space.section - 4.0),
            px(space.section),
            px(space.gap),
            px(4.0),
        ))
        .rules(link_row)
        .on_activate(move |world| fold(world, &panel, &key))
    })
}

/// Folds the group of `key` in the list of `panel` away, or opens it.
fn fold(world: &mut World, panel: &PanelId, key: &str) {
    let mut folds = world.resource_mut::<Folds>();
    let folded = folds.0.entry(panel.0.clone()).or_default();
    if !folded.remove(key) {
        folded.insert(key.to_owned());
    }
}

/// One terminal: where it stands as a breadcrumb, the last command it
/// ran, whether one runs, and its close, or for a pinned log its
/// unpin. It is lit, with a bar at its edge, when it is the one the
/// panel shows. While it is named, a field holds its name.
fn entry(shown: &Shown, item: Item, last: bool) -> AnyView {
    let panel = shown.panel.clone();
    let active = shown.active == Some(item.terminal);
    let renaming = shown.renaming == Some(item.terminal);
    with_cx(move |cx| {
        let theme = cx.theme();
        let terminal = item.terminal;
        let crumbs = if renaming {
            rename_field(panel.clone(), terminal, item.name.clone())
        } else {
            crumbs(&item, 1, active).boxed()
        };
        let mut marks = Vec::new();
        if let Some(code) = item.exit {
            marks.push(
                label(format!("exit {code}"))
                    .size(theme.text.small)
                    .tone(Tone::Critical)
                    .wrap(false)
                    .boxed(),
            );
        } else if item.running {
            marks.push(
                frame()
                    .width(px(7.0))
                    .height(px(7.0))
                    .radius(3.5)
                    .fill(theme.palette.gold_4)
                    .tooltip(|| note("Running"))
                    .boxed(),
            );
        }
        let mut lines: Vec<AnyView> = vec![crumbs];
        if let Some(last) = &item.last {
            let room = theme.terminal.list
                - BOX_GAP * 2.0
                - 4.0
                - ICON
                - theme.space.gap * 2.0
                - (ICON + 8.0)
                - marks.len() as f32 * 56.0
                - 12.0;
            let size = theme.text.small * PREVIEW_SCALE;
            let most = (room / (size * MONO_ADVANCE)).floor().max(4.0)
                as usize;
            lines.push(
                label(elided(&format!("$ {last}"), most))
                    .size(size)
                    .tone(Tone::Faint)
                    .opacity(0.7)
                    .wrap(false)
                    .tagged(MonoText)
                    .boxed(),
            );
        }
        let fill = if active {
            theme.palette.button
        } else {
            Color::NONE
        };
        let bar = if active {
            theme.palette.gold_4
        } else {
            Color::NONE
        };
        let close: AnyView = match &item.key {
            Some(key) => {
                let key = key.clone();
                button(glyph(Glyph::Pinned, ICON).tone(Tone::Faint))
                    .padding(UiRect::all(px(4.0)))
                    .rules(link_row)
                    .tooltip(|| note("Unpin"))
                    .on_activate(move |world| {
                        super::pick::pin(world, &key)
                    })
                    .boxed()
            }
            None => close_button({
                let panel = panel.clone();
                move |world| close(world, &panel, terminal)
            })
            .boxed(),
        };
        column((
            row((
                frame()
                    .width(px(2.0))
                    .height(percent(100.0))
                    .fill(bar),
                button(
                    row((
                        glyph(Glyph::Terminal, ICON).tone(
                            if active {
                                Tone::Accent
                            } else {
                                Tone::Dim
                            },
                        ),
                        column(lines)
                            .gap(1.0)
                            .width(px(0.0))
                            .min_width(px(0.0))
                            .overflow(Overflow::clip())
                            .grow(1.0),
                    ))
                    .gap(theme.space.gap)
                    .width(percent(100.0))
                    .align(AlignItems::Center),
                )
                .grow(1.0)
                .width(px(0.0))
                .min_width(px(0.0))
                .overflow(Overflow::clip())
                .justify(JustifyContent::FlexStart)
                .padding(UiRect::new(
                    px(6.0),
                    px(4.0),
                    px(5.0),
                    px(5.0),
                ))
                .rules(link_row)
                .on_activate({
                    let panel = panel.clone();
                    move |world| activate(world, &panel, terminal)
                })
                .tagged(Entry(terminal, panel.clone())),
                row(marks).gap(4.0).align(AlignItems::Center),
                close,
            ))
            .gap(4.0)
            .min_width(px(0.0))
            .align(AlignItems::Center)
            .fill(fill),
            // The last row of a group has the group's edge under it.
            hairline(if last {
                Color::NONE
            } else {
                theme.palette.border
            }),
        ))
        .gap(0.0)
    })
    .boxed()
}

/// What stands over `item`: the agents above a bash log, each
/// followed by a chevron, then its name, the first `skip` left out.
pub(super) fn crumbs(
    item: &Item,
    skip: usize,
    active: bool,
) -> impl View + use<> {
    let item = item.clone();
    with_cx(move |cx| {
        let theme = cx.theme();
        let small = theme.text.small;
        let mut parts = Vec::<AnyView>::new();
        for name in item.crumbs.iter().skip(skip) {
            parts.push(
                label(name.clone())
                    .size(small)
                    .tone(Tone::Dim)
                    .wrap(false)
                    .boxed(),
            );
            parts.push(
                glyph(Glyph::Into, 12.0).tone(Tone::Accent).boxed(),
            );
        }
        parts.push(
            label(item.name.clone())
                .size(small)
                .bold(true)
                .tone(if active { Tone::Accent } else { Tone::Body })
                .wrap(false)
                .boxed(),
        );
        row(parts)
            .gap(3.0)
            .align(AlignItems::Center)
            .width(percent(100.0))
            .min_width(px(0.0))
            .overflow(Overflow::clip())
    })
}

/// How much smaller than a caption a command preview is set.
const PREVIEW_SCALE: f32 = 0.85;

/// The width of a monospace letter, in ems.
const MONO_ADVANCE: f32 = 0.6;

/// `text` in at most `most` letters, three dots ending what is cut:
/// the monospace face has no ellipsis.
pub(super) fn elided(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut cut = text
        .chars()
        .take(most.saturating_sub(3))
        .collect::<String>();
    cut.push_str("...");
    cut
}

/// The field `terminal`'s name is typed in. Enter names it; Escape,
/// or the keys going elsewhere, leaves it as it was.
fn rename_field(
    panel: PanelId,
    terminal: Entity,
    name: String,
) -> AnyView {
    text_field(name, move |world, name| {
        // Enter keeps the field focused while it commits; focus
        // leaving first drops the name.
        let entered = world
            .resource::<InputFocus>()
            .get()
            .and_then(|input| world.get::<ChildOf>(input))
            .is_some_and(|parent| {
                world.get::<RenameField>(parent.parent()).is_some()
            });
        if !entered {
            return;
        }
        terminals::rename(world, terminal, &name);
        world.resource_mut::<Renaming>().0 = None;
        world.resource_mut::<Handing>().0 = Some(panel.clone());
    })
    .fills_width()
    .rules(text_input)
    .tagged(RenameField)
    .boxed()
}

/// Names the terminal of a row clicked twice in quick succession.
fn start_rename(
    click: On<PointerClick>,
    entries: Query<&Entry>,
    mut renaming: ResMut<Renaming>,
) {
    let Ok(entry) = entries.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary && click.count == 2 {
        renaming.0 = Some((entry.0, entry.1.clone(), false));
    }
}

/// Puts the keyboard in a new rename field, its name chosen so what
/// is typed replaces it.
fn focus_field(
    fields: Query<&Children, Added<RenameField>>,
    mut texts: Query<&mut EditableText>,
    mut focus: ResMut<InputFocus>,
    mut renaming: ResMut<Renaming>,
) {
    for children in &fields {
        if let Some(&text) =
            children.iter().find(|&&kid| texts.contains(kid))
        {
            focus.set(text, FocusCause::Navigated);
            if let Ok(mut field) = texts.get_mut(text) {
                field.queue_edit(TextEdit::SelectAll);
            }
            if let Some((_, _, had_keys)) = &mut renaming.0 {
                *had_keys = true;
            }
        }
    }
}

/// Leaves the name being typed on Escape, or once the keyboard is
/// elsewhere after the field had it.
fn end_rename(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    fields: Query<Entity, With<RenameField>>,
    parents: Query<&ChildOf>,
    mut renaming: ResMut<Renaming>,
    mut commands: Commands,
) {
    let Some((_, panel, had_keys)) = renaming.0.clone() else {
        return;
    };
    let inside = fields.iter().any(|field| {
        focus.get().is_some_and(|input| {
            input == field
                || parents
                    .iter_ancestors(input)
                    .any(|above| above == field)
        })
    });
    if keys.just_pressed(KeyCode::Escape) {
        renaming.0 = None;
        commands.queue(move |world: &mut World| {
            world.resource_mut::<Handing>().0 = Some(panel);
        });
    } else if had_keys && !inside {
        renaming.0 = None;
    }
}

/// Closes `terminal` from the list of `panel`. Closing the one it
/// shows gives the keys to the one it shows next.
fn close(world: &mut World, panel: &PanelId, terminal: Entity) {
    let others = world.resource::<Listed>().items().count() > 1;
    if others
        && world.resource::<Panels>().active(panel) == Some(terminal)
    {
        world.resource_mut::<Handing>().0 = Some(panel.clone());
    }
    terminals::close(world, terminal);
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client as protocol;
    use serde_json::json;

    use super::*;

    fn view(
        id: &str,
        title: &str,
        named: bool,
    ) -> terminals::TerminalView {
        serde_json::from_value(json!({
            "agent": null, "cols": 80, "rows": 24, "cwd": "/",
            "exit": null, "hamlet": null, "id": id, "kind": "shell",
            "name": id, "named": named, "title": title, "last": "cargo test"
        }))
        .unwrap()
    }

    fn of(
        id: &str,
        hamlet: Option<&str>,
        agent: Option<u64>,
        kind: &str,
    ) -> terminals::TerminalView {
        serde_json::from_value(json!({
            "agent": agent, "cols": 80, "rows": 24, "cwd": "/",
            "exit": null, "hamlet": hamlet, "id": id, "kind": kind,
            "name": id, "title": ""
        }))
        .unwrap()
    }

    fn worker(
        id: u64,
        hamlet: Option<&str>,
        parent: Option<u64>,
    ) -> protocol::Agent {
        let mut worker =
            hamlet_testing::realm::agent(id, "home", false);
        worker.kind = AgentKind::Worker;
        worker.name = format!("worker#{id}");
        worker.hamlet = hamlet.map(String::from);
        worker.parent = parent.map(AgentId);
        worker
    }

    /// Each row of the list as `<depth>:<words>`.
    fn rows(listed: &Listed) -> Vec<(String, Vec<String>)> {
        listed
            .0
            .iter()
            .map(|group| {
                let rows = group
                    .rows
                    .iter()
                    .map(|row| match &row.what {
                        What::Agent(branch) => format!(
                            "{}:{} ({})",
                            row.path.len(),
                            branch.address,
                            branch.held
                        ),
                        What::Terminal(item) => {
                            format!(
                                "{}:{}",
                                row.path.len(),
                                item.name
                            )
                        }
                    })
                    .collect();
                (group.title.clone(), rows)
            })
            .collect()
    }

    #[test]
    fn terminals_are_a_tree_of_the_agents_they_are_under_by_hamlet() {
        let mut app = crate::testing::panel_app();
        let mut state = hamlet_testing::realm::state();
        state.revision = 1;
        state.hamlets.push(hamlet_testing::realm::hamlet("mill", 3));
        state.agents.extend([
            hamlet_testing::realm::agent(3, "mill", true),
            hamlet_testing::realm::agent(4, "home", false),
            worker(5, Some("home"), Some(4)),
            worker(6, Some("home"), None),
            worker(7, None, Some(1)),
            worker(8, None, None),
        ]);
        state.terminals = vec![
            of("loose", None, None, "shell"),
            of("mine", Some("home"), None, "shell"),
            of("milled", Some("mill"), None, "shell"),
            of("m-sh", Some("home"), Some(2), "shell"),
            of("m-cmd", Some("home"), Some(2), "commands"),
            of("w5", Some("home"), Some(5), "shell"),
            of("w6", Some("home"), Some(6), "shell"),
            of("own", None, Some(1), "shell"),
            of("w7", None, Some(7), "shell"),
            of("w8", None, Some(8), "shell"),
        ];
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.init_resource::<Listed>();
        list(app.world_mut());
        let group = |title: &str, rows: &[&str]| {
            (
                title.to_owned(),
                rows.iter().map(|row| (*row).to_owned()).collect(),
            )
        };
        // The chief's come first in a hamlet, then the workers a
        // chief started, then the mayor's tree: each agent's
        // shells, then the agents under it, one with no terminal of
        // its own still shown above those that have. An agent's
        // commands are its log, not a row.
        assert_eq!(
            rows(app.world().resource::<Listed>()),
            [
                group(
                    "home",
                    &[
                        "0:mine",
                        "0:worker#6 (1)",
                        "1:w6",
                        "0:hire#2 (2)",
                        "1:m-sh",
                        "1:hire#4 (1)",
                        "2:worker#5 (1)",
                        "3:w5",
                    ]
                ),
                group("mill", &["0:milled"]),
                group(
                    "Hand",
                    &[
                        "0:hand (2)",
                        "1:own",
                        "1:worker#7 (1)",
                        "2:w7"
                    ]
                ),
                group("Workers", &["0:worker#8 (1)", "1:w8"]),
                group("This computer", &["0:loose"]),
            ]
        );
        let logs = app
            .world()
            .resource::<Listed>()
            .0
            .iter()
            .flat_map(|group| &group.rows)
            .filter_map(|row| match &row.what {
                What::Agent(branch) => {
                    branch.log.as_ref().map(|log| {
                        (branch.address.as_str(), log.name.as_str())
                    })
                }
                What::Terminal(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(logs, [("hire#2", "bash")]);

        // A log shows only once its agent is pinned, under the hamlet
        // and the agents above it.
        let listed = app.world().resource::<Listed>();
        let (key, log) = listed
            .0
            .iter()
            .flat_map(|group| &group.rows)
            .find_map(|row| match &row.what {
                What::Agent(Branch {
                    key,
                    log: Some(log),
                    ..
                }) => Some((key.clone(), log.clone())),
                _ => None,
            })
            .expect("a log");
        assert_eq!(log.key.as_deref(), Some(key.as_str()));
        assert_eq!(log.crumbs.len(), 2);
        assert_eq!(log.crumbs[0], "home");
        let count = |pins: &Pins| {
            listed
                .shown(pins)
                .iter()
                .map(|(_, items)| items.len())
                .sum::<usize>()
        };
        let loose = count(&Pins::default());
        assert_eq!(count(&Pins(BTreeSet::from([key]))), loose + 1);
    }

    #[test]
    fn a_long_command_is_cut_with_an_ellipsis() {
        assert_eq!(elided("$ ls", 10), "$ ls");
        assert_eq!(elided("$ cargo test --all", 10), "$ cargo...");
        assert_eq!(elided("$ ls", 4), "$ ls");
    }

    #[test]
    fn a_name_given_shows_over_the_title_and_a_group_folds_and_opens()
    {
        let mut app = crate::testing::panel_app();
        let mut state = hamlet_testing::realm::state();
        state.revision = 1;
        state.terminals = vec![
            view("plain", "vim", false),
            view("named", "vim", true),
        ];
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.init_resource::<Panels>().init_resource::<Listed>();
        list(app.world_mut());
        let listed = app.world().resource::<Listed>().clone();
        let names = listed
            .items()
            .map(|item| (item.name.as_str(), item.last.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                ("named", Some("cargo test")),
                ("vim", Some("cargo test"))
            ]
        );
        let panel = PanelId::first();
        app.init_resource::<Folds>();
        let folded = |app: &App| {
            app.world()
                .resource::<Folds>()
                .0
                .get(&panel.0)
                .cloned()
                .unwrap_or_default()
        };
        fold(app.world_mut(), &panel, "");
        assert!(folded(&app).contains(""));
        fold(app.world_mut(), &panel, "");
        assert!(folded(&app).is_empty());
    }
}
