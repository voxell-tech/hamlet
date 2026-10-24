//! The dock and its panels. A panel is an entity, and the dock can
//! show it while the entity is there.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::platform::collections::HashMap;
use bevy::ui::ComputedNode;
use bevy_fynix::dock::{
    DockArea, DockNode, DockPlugin, DockRegistry, DockTree,
    DockWindowKind, NodeId, SplitAxis,
};
use hamlet_engine_api::{PanelId, PanelPadding};
use hamlet_engine_core::TuningPlugin;
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use serde::Deserialize;

use crate::icons::Glyph;
use crate::notes::glowing;
use crate::{AnyView, EngineTheme};

/// The name of the engine's plugin list, which the plugins module
/// adds.
pub const PLUGIN_LIST: &str = "plugins";

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        DockPlugin::<EngineTheme>::default(),
        TuningPlugin::<DockTuning>::default(),
    ))
    .add_observer(offer)
    .add_observer(withdraw)
    .add_systems(Update, keep_wide);
}

/// How the dock keeps its panels.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockTuning {
    /// The least width a panel keeps, in logical pixels.
    pub least_panel: f32,
}

impl Tuning for DockTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("dock");
}

/// The width `node` needs for every panel in it to keep `least`, and
/// the width it is drawn at, from the width of each area.
fn widths(
    tree: &DockTree,
    drawn: &HashMap<NodeId, f32>,
    node: NodeId,
    least: f32,
) -> (f32, f32) {
    match tree.get(node) {
        Some(DockNode::Split(split)) => {
            let (a, b) = (
                widths(tree, drawn, split.a, least),
                widths(tree, drawn, split.b, least),
            );
            match split.axis {
                SplitAxis::Horizontal => (a.0 + b.0, a.1 + b.1),
                SplitAxis::Vertical => (a.0.max(b.0), a.1.max(b.1)),
            }
        }
        _ => (least, drawn.get(&node).copied().unwrap_or_default()),
    }
}

/// The share `a` has of a split `span` wide, moved no further than
/// leaves each side the `least` it needs. A span too narrow for
/// both is shared by what each needs.
fn held(share: f32, least: (f32, f32), span: f32) -> f32 {
    if least.0 + least.1 >= span {
        return least.0 / (least.0 + least.1);
    }
    share.clamp(least.0 / span, 1.0 - least.1 / span)
}

/// Keeps every panel at least `least_panel` wide, however a divider
/// is dragged and whatever size the window takes.
fn keep_wide(
    tuning: Res<DockTuning>,
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
            let least = tuning.least_panel;
            let (a, b) = (
                widths(&tree, &drawn, split.a, least),
                widths(&tree, &drawn, split.b, least),
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

/// A panel the dock can show.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct Panel {
    pub id: PanelId,
    pub title: String,
    /// Builds its content, given the panel's entity.
    pub content: fn(Entity) -> AnyView,
}

/// The icon on a panel's tab, beside its [`Panel`].
#[derive(Component, Clone, Copy, Debug)]
#[component(immutable)]
pub struct PanelIcon(pub Glyph);

/// The padding of a panel's content in place of the theme's, beside
/// its [`Panel`].
#[derive(Component, Clone, Copy, Debug)]
#[component(immutable)]
pub struct PanelInset(pub PanelPadding);

/// Lets the dock show a new panel. A panel is opened from a tab
/// bar's "+", not the View menu.
fn offer(
    added: On<Add, Panel>,
    panels: Query<(&Panel, Option<&PanelIcon>)>,
    theme: Res<EngineTheme>,
    mut registry: ResMut<DockRegistry<EngineTheme>>,
) {
    let entity = added.entity;
    let Ok((panel, icon)) = panels.get(entity) else {
        return;
    };
    let content = panel.content;
    let id = panel.id.as_str().to_string();
    let mut kind =
        DockWindowKind::new(panel.title.to_uppercase(), move || {
            glowing(id.clone(), content(entity))
        });
    if let Some(PanelIcon(which)) = icon {
        kind = kind.icon(which.image(theme.space.icon));
    }
    registry.register(panel.id.as_str(), kind);
}

/// Takes a panel that is going out of the dock.
fn withdraw(
    removed: On<Remove, Panel>,
    panels: Query<&Panel>,
    mut registry: ResMut<DockRegistry<EngineTheme>>,
    mut tree: ResMut<DockTree>,
) {
    let Ok(panel) = panels.get(removed.entity) else {
        return;
    };
    registry.unregister(panel.id.as_str());
    tree.remove_window_kind(panel.id.as_str());
}
