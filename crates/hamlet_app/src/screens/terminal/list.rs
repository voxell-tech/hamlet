//! The terminals beside the screen: by hamlet, then the Hand's, the
//! workers' of no hamlet and the unbounded ones, each a tree of the
//! agents they are under, whose groups and agents fold away or open.
//! Each terminal shows its name, what it is, the last command it ran,
//! and its close. A double click names one.
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
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm;
use hamlet_client::terminals::{self, Terminal, TerminalKind};
use hamlet_protocol::client::{AgentId, AgentKind};
use hamlet_ui::styles::{
    Fills, ICON, caption, close_button, hairline, link_row,
    scroll_area, text_input,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, View, with_cx};

use super::{Adding, Folds, Handing, PanelId, Panels, activate};
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
    pub(super) mayor: bool,
    /// The terminals under it, its own and its agents'.
    pub(super) held: usize,
}

#[derive(Clone, PartialEq, Debug)]
pub(super) struct Item {
    pub(super) terminal: Entity,
    pub(super) name: String,
    /// The agent's address, or "chief".
    pub(super) owner: String,
    pub(super) sandboxed: bool,
    pub(super) exit: Option<i64>,
    pub(super) commands: bool,
    /// The last command it ran.
    pub(super) last: Option<String>,
}

impl Listed {
    pub(super) fn find(&self, terminal: Entity) -> Option<&Item> {
        self.items().find(|item| item.terminal == terminal)
    }

    pub(super) fn items(&self) -> impl Iterator<Item = &Item> {
        self.0.iter().flat_map(|group| &group.rows).filter_map(
            |row| match &row.what {
                What::Terminal(item) => Some(item),
                What::Agent(_) => None,
            },
        )
    }

    /// The keys of the agents above `terminal`, the outermost first.
    pub(super) fn path(&self, terminal: Entity) -> Option<&[String]> {
        self.0.iter().flat_map(|group| &group.rows).find_map(|row| {
            match &row.what {
                What::Terminal(item) if item.terminal == terminal => {
                    Some(row.path.as_slice())
                }
                _ => None,
            }
        })
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
        let name = if view.named || view.title.is_empty() {
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
                sandboxed: view.sandboxed,
                exit: view.exit,
                commands,
                last: view.last.clone(),
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
                "" => (3, "Unbounded".to_owned()),
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
                    &mut rows,
                );
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
    rows: &mut Vec<Row>,
) -> usize {
    let Node {
        mut terminals,
        under,
    } = nodes.remove(&agent).unwrap_or_default();
    let found = owners.get(&agent);
    let key = format!("agent#{}", agent.0);
    let at = rows.len();
    rows.push(Row {
        path: path.clone(),
        what: What::Agent(Branch {
            key: key.clone(),
            address: found.map_or_else(
                || format!("agent#{}", agent.0),
                |o| o.address.clone(),
            ),
            name: found.map(|o| o.name.clone()).filter(|name| {
                !name.is_empty()
                    && Some(name) != found.map(|o| &o.address)
            }),
            mayor: found.is_some_and(|o| o.mayor),
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
        held += branch(below, nodes, owners, path, rows);
    }
    path.pop();
    if let What::Agent(branch) = &mut rows[at].what {
        branch.held = held;
    }
    held
}

/// The list of terminals in `panel` and the press that opens another.
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
                    .on_activate_with({
                        let panel = panel.clone();
                        move |world, node| {
                            let anchor =
                                crate::menu::rect_of(world, node);
                            world.resource_mut::<Adding>().0 = anchor
                                .map(|anchor| {
                                    (anchor, panel.clone())
                                });
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
                    move |(listed, folded, active, renaming)| {
                        groups(
                            &panel, listed, folded, *active,
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

fn groups(
    panel: &PanelId,
    listed: &Listed,
    folded: &BTreeSet<String>,
    active: Option<Entity>,
    renaming: Option<Entity>,
) -> impl View + use<> {
    // The guide of the agent the shown terminal is under is lit.
    let lit = active.and_then(|active| listed.path(active)).and_then(
        |path| {
            Some((path.len().checked_sub(1)?, path.last()?.clone()))
        },
    );
    let groups = listed
        .0
        .iter()
        .map(|group| {
            let shut = folded.contains(&group.key);
            let rows = if shut {
                Vec::new()
            } else {
                group
                    .rows
                    .iter()
                    .filter(|row| {
                        !row.path
                            .iter()
                            .any(|key| folded.contains(key))
                    })
                    .map(|row| {
                        let guides = guides(&row.path, lit.as_ref());
                        match &row.what {
                            What::Agent(branch) => agent_row(
                                panel,
                                branch.clone(),
                                folded.contains(&branch.key),
                                guides,
                            ),
                            What::Terminal(item) => entry(
                                panel,
                                item.clone(),
                                active,
                                renaming,
                                guides,
                            ),
                        }
                    })
                    .collect::<Vec<_>>()
            };
            let held = group
                .rows
                .iter()
                .filter(|row| matches!(row.what, What::Terminal(_)))
                .count();
            column((
                heading(panel, group, held, shut),
                column(rows).gap(0.0),
            ))
            .gap(0.0)
            .boxed()
        })
        .collect::<Vec<_>>();
    column(groups).gap(0.0).padding(UiRect::bottom(px(8.0)))
}

/// How far each level of the tree stands in from the one above.
const INDENT: f32 = 14.0;

/// What a row at `path` stands in by: a guide for each agent above
/// it, the one of `lit` lit.
#[derive(Clone)]
struct Guides(Vec<bool>);

fn guides(path: &[String], lit: Option<&(usize, String)>) -> Guides {
    Guides(
        path.iter()
            .enumerate()
            .map(|(level, key)| {
                lit.is_some_and(|(at, lit)| {
                    *at == level && lit == key
                })
            })
            .collect(),
    )
}

/// The guides of a row, as tall as the row, so a level's guides join
/// from row to row.
fn guide_views(guides: &Guides) -> Vec<AnyView> {
    guides
        .0
        .iter()
        .map(|lit| {
            let lit = *lit;
            with_cx(move |cx| {
                let palette = &cx.theme().palette;
                let color = if lit {
                    palette.gold_4.with_alpha(0.7)
                } else {
                    palette.border
                };
                column((column(())
                    .width(px(1.0))
                    .height(percent(100.0))
                    .fill(color),))
                .width(px(INDENT))
                .shrink(0.0)
                .align(AlignItems::Center)
            })
            .boxed()
        })
        .collect()
}

/// A group's title, which folds the group away or opens it, with how
/// many it holds while folded.
fn heading(
    panel: &PanelId,
    group: &Group,
    held: usize,
    shut: bool,
) -> impl View + use<> {
    let panel = panel.clone();
    let key = group.key.clone();
    let title = group.title.to_uppercase();
    let count = shut.then(|| held.to_string());
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let mark = if shut { Glyph::Into } else { Glyph::Down };
        let mut words = vec![
            glyph(mark, ICON).tone(Tone::Faint).boxed(),
            caption(title.clone(), Tone::Dim).boxed(),
        ];
        if let Some(count) = &count {
            words.push(caption(count.clone(), Tone::Faint).boxed());
        }
        let key = key.clone();
        let panel = panel.clone();
        button(row(words).gap(4.0).align(AlignItems::Center))
            .justify(JustifyContent::FlexStart)
            .padding(UiRect::new(
                px(space.section - 4.0),
                px(space.section),
                px(space.gap),
                px(2.0),
            ))
            .rules(link_row)
            .on_activate(move |world| fold(world, &panel, &key))
    })
}

/// An agent in the tree: its address, then its name, which folds
/// what is under it away or opens it, with how many terminals that
/// holds while folded.
fn agent_row(
    panel: &PanelId,
    branch: Branch,
    shut: bool,
    guides: Guides,
) -> AnyView {
    let panel = panel.clone();
    with_cx(move |cx| {
        let theme = cx.theme();
        let small = theme.text.small;
        let mark = if shut { Glyph::Into } else { Glyph::Down };
        let who = if branch.mayor {
            Glyph::Mayor
        } else {
            Glyph::Robot
        };
        let mut words = vec![
            glyph(mark, ICON).tone(Tone::Faint).boxed(),
            glyph(who, ICON).tone(Tone::Dim).boxed(),
            label(branch.address.clone())
                .size(small)
                .tone(Tone::Body)
                .wrap(false)
                .boxed(),
        ];
        if let Some(name) = &branch.name {
            words.push(
                label(name.clone())
                    .size(small)
                    .tone(Tone::Faint)
                    .wrap(false)
                    .boxed(),
            );
        }
        if shut {
            words.push(
                caption(branch.held.to_string(), Tone::Faint).boxed(),
            );
        }
        let key = branch.key.clone();
        let panel = panel.clone();
        let mut parts = guide_views(&guides);
        parts.push(
            button(row(words).gap(4.0).align(AlignItems::Center))
                .grow(1.0)
                .min_width(px(0.0))
                .justify(JustifyContent::FlexStart)
                .padding(UiRect::new(
                    px(0.0),
                    px(4.0),
                    px(3.0),
                    px(3.0),
                ))
                .rules(link_row)
                .on_activate(move |world| fold(world, &panel, &key))
                .boxed(),
        );
        row(parts)
            .gap(0.0)
            .padding(UiRect::left(px(theme.space.section - 4.0)))
    })
    .boxed()
}

/// Folds the group or the agent of `key` in the list of `panel` away,
/// or opens it.
fn fold(world: &mut World, panel: &PanelId, key: &str) {
    let mut folds = world.resource_mut::<Folds>();
    let folded = folds.0.entry(panel.0.clone()).or_default();
    if !folded.remove(key) {
        folded.insert(key.to_owned());
    }
}

/// One terminal: its name, what it is, the last command it ran, and
/// its close, lit when it is the one `panel` shows; the chief's says
/// so. While it is named, a field holds its name.
fn entry(
    panel: &PanelId,
    item: Item,
    shown: Option<Entity>,
    renaming: Option<Entity>,
    guides: Guides,
) -> AnyView {
    let panel = panel.clone();
    let active = shown == Some(item.terminal);
    let renaming = renaming == Some(item.terminal);
    let depth = guides.0.len();
    with_cx(move |cx| {
        let theme = cx.theme();
        let small = theme.text.small;
        let terminal = item.terminal;
        let mut about = Vec::new();
        // Under an agent, the tree says whose it is.
        if depth == 0 {
            about.push(item.owner.clone());
        }
        if item.commands {
            about.push("read-only".into());
        }
        if !item.sandboxed {
            about.push("unsandboxed".into());
        }
        if let Some(code) = item.exit {
            about.push(format!("exit {code}"));
        }
        if about.is_empty() {
            about.push("shell".into());
        }
        let mark = if item.commands {
            Glyph::Play
        } else {
            Glyph::Terminal
        };
        let tone = if active { Tone::Accent } else { Tone::Dim };
        let fill = if active {
            theme.palette.button
        } else {
            Color::NONE
        };
        let name = if renaming {
            rename_field(panel.clone(), terminal, item.name.clone())
        } else {
            label(item.name.clone())
                .size(small)
                .bold(active)
                .tone(if active { Tone::Body } else { Tone::Dim })
                .wrap(false)
                .boxed()
        };
        let mut lines = vec![
            name,
            label(about.join(" \u{b7} "))
                .size(theme.text.small)
                .tone(if item.exit.is_some_and(|code| code != 0) {
                    Tone::Critical
                } else {
                    Tone::Faint
                })
                .wrap(false)
                .boxed(),
        ];
        if let Some(last) = &item.last {
            // What the row leaves for the line: the list less its
            // paddings, the guides, the mark, the close and the gaps
            // between.
            let room = theme.terminal.list
                - theme.space.section
                - 4.0
                - depth as f32 * INDENT
                - ICON
                - theme.space.gap * 2.0
                - (ICON + 8.0);
            let most = (room / (theme.text.small * MONO_ADVANCE))
                .floor()
                .max(4.0) as usize;
            lines.push(
                label(elided(&format!("$ {last}"), most))
                    .size(theme.text.small)
                    .tone(Tone::Faint)
                    .wrap(false)
                    .tagged(MonoText)
                    .boxed(),
            );
        }
        let mut parts = guide_views(&guides);
        parts.push(
            row((
                button(
                    row((
                        glyph(mark, ICON).tone(tone),
                        column(lines)
                            .gap(1.0)
                            .min_width(px(0.0))
                            .grow(1.0),
                    ))
                    .gap(theme.space.gap)
                    .align(AlignItems::Center),
                )
                .grow(1.0)
                .min_width(px(0.0))
                .justify(JustifyContent::FlexStart)
                .padding(UiRect::new(
                    px(if depth == 0 { 4.0 } else { 0.0 }),
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
                close_button({
                    let panel = panel.clone();
                    move |world| close(world, &panel, terminal)
                }),
            ))
            .grow(1.0)
            .min_width(px(0.0))
            .align(AlignItems::Center)
            .boxed(),
        );
        row(parts)
            .gap(0.0)
            .padding(UiRect::new(
                px(theme.space.section - 4.0),
                px(theme.space.gap),
                px(0.0),
                px(0.0),
            ))
            .fill(fill)
    })
    .boxed()
}

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
            "name": id, "named": named, "title": title, "sandboxed": false,
            "last": "cargo test"
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
            "name": id, "sandboxed": true, "title": ""
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
        // commands, its shells, then the agents under it, one
        // with no terminal of its own still shown above those
        // that have.
        assert_eq!(
            rows(app.world().resource::<Listed>()),
            [
                group(
                    "home",
                    &[
                        "0:mine",
                        "0:worker#6 (1)",
                        "1:w6",
                        "0:hire#2 (3)",
                        "1:m-cmd",
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
                group("Unbounded", &["0:loose"]),
            ]
        );
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
