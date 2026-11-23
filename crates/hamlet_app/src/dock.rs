//! The dock: where each panel starts, the numbered panels of the desk
//! shown, and the width each panel keeps.

use std::collections::HashMap;

use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::ComputedNode;
use bevy_fynix::dock::{
    DockArea, DockAreaStyle, DockLeaf, DockNode, DockRegistry,
    DockTree, Edge, NodeId, SplitAxis,
};
use hamlet_client::waits::Waiting;
use hamlet_core::panels::{
    self, DeskCapture, Numbered, OnDesks, PanelSystems, free_copy,
    number,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Attention;

use crate::desk::{Desks, Part};
use crate::{desk, remember};

/// The least width a panel keeps, in logical pixels.
const LEAST: f32 = 300.0;

/// The share of the window's width the realm takes on a new desk.
pub(crate) const REALM_SHARE: f32 = 0.5;

/// The dock window of the Hub.
pub(crate) const HUB: &str = "hub";

/// The ids of panels a stored dock may still name that the window no
/// longer has.
const GONE: [&str; 2] = ["agent", "needs"];

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        PreUpdate,
        count_waits.in_set(PanelSystems::Bind),
    )
    .add_systems(
        Update,
        (
            keep_wide,
            // The Hubs keep the panels a desk holds, after the
            // dock's changes are taken in.
            hold_desked
                .run_if(resource_changed::<Desks>)
                .after(desk::capture)
                .in_set(DeskCapture),
            register_numbered
                .before(desk::show)
                .run_if(resource_changed::<Desks>),
            prune_numbered
                .run_if(resource_changed::<DockTree>)
                .after(desk::capture),
        ),
    );
}

/// Tells the panels' highlights how many things wait for the chief.
fn count_waits(
    waiting: Query<(), With<Waiting>>,
    mut attention: ResMut<Attention>,
) {
    let count = waiting.iter().count();
    if attention.0 != count {
        attention.0 = count;
    }
}

/// The ids of the panels in the tabs of `parts`.
fn tabs_of(parts: &[Part]) -> impl Iterator<Item = &String> {
    parts.iter().flat_map(|part| match part {
        Part::Tabs { tabs, .. } => tabs.as_slice(),
        Part::Split { .. } => &[],
    })
}

/// Tells the panels which of them the desks hold.
pub(crate) fn hold_desked(
    desks: Res<Desks>,
    mut held: ResMut<OnDesks>,
) {
    let now = OnDesks(
        desks
            .list()
            .iter()
            .flat_map(|desk| tabs_of(&desk.dock).cloned())
            .collect(),
    );
    if *held != now {
        *held = now;
    }
}

/// Registers the numbered panels the shown desk and the dock hold, so
/// the desk is laid out with them.
pub fn register_numbered(world: &mut World) {
    let mut ids = world
        .get_resource::<Desks>()
        .and_then(|desks| desks.list().get(desks.shown()))
        .map(|desk| tabs_of(&desk.dock).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(tree) = world.get_resource::<DockTree>() {
        ids.extend(
            tree.nodes
                .values()
                .filter_map(DockNode::as_leaf)
                .flat_map(|leaf| {
                    leaf.tabs().map(|(id, _)| id.to_owned())
                }),
        );
    }
    for id in ids {
        panels::register(world, &id);
    }
}

/// Forgets the numbered panels the dock no longer holds, and keeps
/// one of each not in the dock, so the dock's add menu always offers
/// one more: the first, else a free copy.
fn prune_numbered(world: &mut World) {
    let firsts = world
        .resource::<Numbered>()
        .firsts()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    // A spare copy of each numbered panel that is docked, for the
    // add menu to open.
    let spares = firsts
        .iter()
        .filter(|first| {
            world
                .resource::<DockTree>()
                .find_leaf_with_window(first)
                .is_some()
        })
        .map(|first| free_copy(world, first))
        .collect::<Vec<_>>();
    let gone = {
        let tree = world.resource::<DockTree>();
        world
            .resource::<DockRegistry<HamletTheme>>()
            .iter()
            .map(|(id, _)| id)
            .filter(|id| {
                firsts.iter().any(|first| number(first, id).is_some())
            })
            .filter(|id| tree.find_leaf_with_window(id).is_none())
            .filter(|id| !spares.iter().any(|spare| spare == id))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let mut registry =
        world.resource_mut::<DockRegistry<HamletTheme>>();
    for id in gone {
        registry.unregister(&id);
    }
    for spare in spares {
        let known = world
            .resource::<DockRegistry<HamletTheme>>()
            .iter()
            .any(|(id, _)| id == spare);
        if !known {
            panels::register(world, &spare);
        }
    }
}

/// The width `node` needs for every panel in it to keep [`LEAST`],
/// and the width it is drawn at, from the width of each area.
fn widths(
    tree: &DockTree,
    drawn: &HashMap<NodeId, f32>,
    node: NodeId,
) -> (f32, f32) {
    match tree.get(node) {
        Some(DockNode::Split(split)) => {
            let (a, b) = (
                widths(tree, drawn, split.a),
                widths(tree, drawn, split.b),
            );
            match split.axis {
                SplitAxis::Horizontal => (a.0 + b.0, a.1 + b.1),
                SplitAxis::Vertical => (a.0.max(b.0), a.1.max(b.1)),
            }
        }
        _ => (LEAST, drawn.get(&node).copied().unwrap_or_default()),
    }
}

/// The share `a` has of a split `span` wide, moved no further than
/// leaves each side the `least` it needs. A span too narrow for both
/// is shared by what each needs.
fn held(share: f32, least: (f32, f32), span: f32) -> f32 {
    if least.0 + least.1 >= span {
        return least.0 / (least.0 + least.1);
    }
    share.clamp(least.0 / span, 1.0 - least.1 / span)
}

/// Keeps every panel at least [`LEAST`] wide, however a divider is
/// dragged and whatever size the window takes.
pub(crate) fn keep_wide(
    mut tree: ResMut<DockTree>,
    areas: Query<(&DockArea, &ComputedNode)>,
) {
    let drawn = areas
        .iter()
        .map(|(area, node)| {
            (area.leaf, node.size().x * node.inverse_scale_factor())
        })
        .collect::<HashMap<_, _>>();
    let moved = tree
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            let split = node.as_split()?;
            if split.axis != SplitAxis::Horizontal {
                return None;
            }
            let (a, b) = (
                widths(&tree, &drawn, split.a),
                widths(&tree, &drawn, split.b),
            );
            let span = a.1 + b.1;
            if span <= 0.0 {
                return None;
            }
            let share = held(split.fraction, (a.0, b.0), span);
            ((share - split.fraction).abs() > 0.002)
                .then_some((*id, share))
        })
        .collect::<Vec<_>>();
    for (split, share) in moved {
        tree.set_fraction(split, share);
    }
}

/// Puts back the kept view and lays out the desk shown.
pub(crate) fn seed(world: &mut World) {
    remember::restore(world);
    let _ = world.run_system_cached(hold_desked);
    register_numbered(world);
    // Here rather than in the next frame, so the panels are mounted
    // on the desk.
    let _ = world.run_system_cached(desk::show);
}

/// The dock the window starts with: the realm on the left, and the
/// Hub on the right.
pub(crate) fn designed(tree: &mut DockTree) {
    let centre = tree.set_root_leaf(
        DockLeaf::new("centre", DockAreaStyle::TabBar)
            .with_windows(vec!["world".into()]),
    );
    if let Some((side, _)) =
        tree.split(centre, Edge::Right, HUB.into())
        && let Some(parent) = tree.parent_of(side)
    {
        tree.set_fraction(parent, REALM_SHARE);
    }
}

/// Whether a stored dock names a panel the window no longer has.
pub(crate) fn is_stale(
    tabs: impl IntoIterator<Item = String>,
) -> bool {
    tabs.into_iter().any(|tab| GONE.contains(&tab.as_str()))
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy_fynix::dock::{
        DockAreaStyle, DockLeaf, DockWindowKind, DropTarget,
    };
    use bevy_fynix::views::frame;
    use hamlet_core::panels::{
        PanelAppExt, flush_panel, open_numbered,
    };

    use super::*;

    fn window(id: &str) -> DockWindowKind<HamletTheme> {
        DockWindowKind::new(id.to_owned(), || flush_panel(frame()))
    }

    #[test]
    fn a_numbered_panel_is_known_only_while_the_dock_or_the_shown_desk_holds_it()
     {
        let mut app = App::new();
        app.init_resource::<DockTree>()
            .init_resource::<OnDesks>()
            .add_numbered_panel("terminal", window);
        let desk = |tabs: &[&str]| {
            vec![Part::Tabs {
                area: "low".into(),
                tabs: tabs
                    .iter()
                    .map(|tab| (*tab).to_owned())
                    .collect(),
                front: 0,
                headless: false,
                kept: false,
            }]
        };
        let mut desks = serde_json::to_value(Desks::first(desk(&[
            "terminal",
            "terminal-3",
        ])))
        .unwrap();
        desks["list"].as_array_mut().unwrap().push(
            serde_json::json!({
                "name": "Other",
                "dock": desk(&["terminal-5"]),
            }),
        );
        app.insert_resource(
            serde_json::from_value::<Desks>(desks).unwrap(),
        );
        app.world_mut().run_system_cached(hold_desked).unwrap();
        register_numbered(app.world_mut());
        let known = |app: &App, id: &str| {
            app.world()
                .resource::<DockRegistry<HamletTheme>>()
                .get(id)
                .is_some()
        };
        assert!(known(&app, "terminal-3"));
        // Another desk's is registered once that desk is shown.
        assert!(!known(&app, "terminal-5"));
        app.world_mut().resource_mut::<DockTree>().set_root_leaf(
            DockLeaf::new("low", DockAreaStyle::TabBar)
                .with_windows(vec!["terminal".into()]),
        );
        let leaf = app.world().resource::<DockTree>().root.unwrap();
        // The next number is one no desk holds.
        let opened = open_numbered(
            app.world_mut(),
            "terminal",
            DropTarget::Edge {
                leaf,
                edge: Edge::Right,
            },
        );
        assert_eq!(opened.as_deref(), Some("terminal-2"));
        let opened = open_numbered(
            app.world_mut(),
            "terminal",
            DropTarget::Area(leaf),
        );
        assert_eq!(opened.as_deref(), Some("terminal-4"));
        prune_numbered(app.world_mut());
        assert!(
            known(&app, "terminal-2") && known(&app, "terminal-4")
        );
        // Out of the dock, a panel leaves the add menu; the first
        // stays, and the menu offers a copy no desk holds.
        assert!(!known(&app, "terminal-3"));
        assert!(known(&app, "terminal"));
        assert!(known(&app, "terminal-6"));
    }

    #[test]
    fn a_desk_that_names_an_old_panel_starts_again() {
        assert!(is_stale(["world".into(), "agent".into()]));
        assert!(is_stale([HUB.into(), "needs".into()]));
        assert!(!is_stale(["world".into(), HUB.into()]));
    }
}
