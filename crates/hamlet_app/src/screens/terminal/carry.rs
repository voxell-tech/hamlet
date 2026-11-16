//! Dragging a terminal out of a panel's list onto the dock, which
//! opens it in a Terminal panel of its own where it is dropped, as a
//! tab dragged in the dock moves: beside a tab in a bar, into an
//! area, or at an area's edge, splitting it.

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::picking::Pickable;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::picking::pointer::PointerButton;
use bevy::text::{
    FontSize, FontWeight, LineBreak, TextColor, TextFont, TextLayout,
};
use bevy::ui::widget::Text;
use bevy::ui::{
    AlignItems, BackgroundColor, BorderRadius, ComputedNode,
    GlobalZIndex, Node, PositionType, UiGlobalTransform, UiRect,
    UiScale, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::Theme;
use bevy_fynix::cursor::OverrideCursor;
use bevy_fynix::dock::{DockArea, DockTab, DropTarget, Edge, NodeId};
use bevy_fynix::tokens::{
    SpacingTokens, SurfaceTokens, TextTokens, Tone,
};
use hamlet_ui::theme::HamletTheme;

use super::list::{Entry, Listed};
use super::{Handing, PANEL, PanelId, Panels, tab_name};
use crate::dock::open_numbered;

/// How far the pointer moves before a press on a terminal drags it.
const THRESHOLD: f32 = 5.0;

/// The outer share of an area, each side of its centre, that is its
/// edge.
const EDGE: f32 = 0.25;

/// The width of the hint in a bar with no tabs.
const EMPTY_HINT: f32 = 40.0;

/// Where the pointer holds the ghost, from its corner.
const GRAB: Vec2 = Vec2::new(40.0, 12.0);

const GHOST_Z: i32 = 200;
const HINT_Z: i32 = 190;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Carry>()
        .add_observer(start)
        .add_observer(moved)
        .add_observer(end);
}

/// The terminal dragged out of a list, if any.
#[derive(Resource, Default)]
enum Carry {
    #[default]
    Idle,
    /// Pressed, and not moved far enough yet.
    Pending { terminal: Entity, start: Vec2 },
    Carrying {
        terminal: Entity,
        ghost: Entity,
        hint: Option<Entity>,
        target: Option<DropTarget>,
    },
}

/// The bar of a leaf, and where its tabs are.
struct Bar {
    leaf: NodeId,
    rect: Rect,
    tabs: Vec<Rect>,
}

fn logical(position: Vec2, scale: Option<Res<UiScale>>) -> Vec2 {
    position / scale.map_or(1.0, |scale| scale.0)
}

/// The rect of a node in logical pixels.
fn rect_of(
    computed: &ComputedNode,
    place: &UiGlobalTransform,
) -> Rect {
    let inverse = computed.inverse_scale_factor();
    let (_, _, centre) = place.to_scale_angle_translation();
    Rect::from_center_size(
        centre.trunc() * inverse,
        computed.size() * inverse,
    )
}

/// A press on a terminal in a list waits for the pointer to move.
fn start(
    drag: On<PointerDragStart>,
    entries: Query<&Entry>,
    scale: Option<Res<UiScale>>,
    mut carry: ResMut<Carry>,
) {
    let Ok(Entry(terminal, _)) = entries.get(drag.event_target())
    else {
        return;
    };
    if drag.button != PointerButton::Primary {
        return;
    }
    *carry = Carry::Pending {
        terminal: *terminal,
        start: logical(drag.pointer.position, scale),
    };
}

/// Starts carrying once the pointer has moved far enough, then keeps
/// the ghost under the pointer and the hint on where it would open.
#[expect(
    clippy::too_many_arguments,
    reason = "a system reads the dock's nodes and the carry's state"
)]
fn moved(
    mut drag: On<PointerDrag>,
    mut carry: ResMut<Carry>,
    entries: Query<(), bevy::ecs::query::With<Entry>>,
    listed: Res<Listed>,
    theme: Option<Res<Theme<HamletTheme>>>,
    scale: Option<Res<UiScale>>,
    tabs: Query<(
        &DockTab,
        &ComputedNode,
        &UiGlobalTransform,
        &ChildOf,
    )>,
    areas: Query<(&DockArea, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    rects: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut nodes: Query<&mut Node>,
    mut forced: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    if !entries.contains(drag.event_target()) {
        return;
    }
    let Some(theme) = theme else {
        return;
    };
    let cursor = logical(drag.pointer.position, scale);
    match &mut *carry {
        Carry::Idle => {}
        Carry::Pending { terminal, start } => {
            drag.propagate(false);
            if cursor.distance(*start) < THRESHOLD {
                return;
            }
            let name = tab_name(listed.find(*terminal));
            let ghost =
                spawn_ghost(&mut commands, name, cursor, &theme.0);
            forced.0 = Some(SystemCursorIcon::Grabbing);
            *carry = Carry::Carrying {
                terminal: *terminal,
                ghost,
                hint: None,
                target: None,
            };
        }
        Carry::Carrying {
            ghost,
            hint,
            target,
            ..
        } => {
            drag.propagate(false);
            if let Ok(mut ghost) = nodes.get_mut(*ghost) {
                ghost.left = px(cursor.x - GRAB.x);
                ghost.top = px(cursor.y - GRAB.y);
            }
            let mut bars = Vec::<Bar>::new();
            for (tab, computed, place, row) in &tabs {
                let Some(bar) = parents
                    .get(row.parent())
                    .ok()
                    .and_then(|bar| rects.get(bar.parent()).ok())
                else {
                    continue;
                };
                let tab_rect = rect_of(computed, place);
                match bars.iter_mut().find(|bar| bar.leaf == tab.leaf)
                {
                    Some(bar) => bar.tabs.push(tab_rect),
                    None => bars.push(Bar {
                        leaf: tab.leaf,
                        rect: rect_of(bar.0, bar.1),
                        tabs: vec![tab_rect],
                    }),
                }
            }
            for bar in &mut bars {
                bar.tabs.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
            }
            let areas = areas
                .iter()
                .map(|(area, computed, place)| {
                    (area.leaf, rect_of(computed, place))
                })
                .collect::<Vec<_>>();
            let next = target_at(cursor, &bars, &areas);
            *target = next.map(|(next, _)| next);
            match (*hint, next) {
                (None, Some((_, rect))) => {
                    *hint = Some(spawn_hint(
                        &mut commands,
                        rect,
                        &theme.0,
                    ));
                }
                (Some(shown), Some((_, rect))) => {
                    if let Ok(mut shown) = nodes.get_mut(shown) {
                        place(&mut shown, rect);
                    }
                }
                (Some(shown), None) => {
                    commands.entity(shown).try_despawn();
                    *hint = None;
                }
                (None, None) => {}
            }
        }
    }
}

/// Opens the terminal carried in a panel of its own where it is
/// dropped. Dropped elsewhere, it stays where it was.
fn end(
    _: On<PointerDragEnd>,
    mut carry: ResMut<Carry>,
    mut forced: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    let Carry::Carrying {
        terminal,
        ghost,
        hint,
        target,
    } = std::mem::take(&mut *carry)
    else {
        *carry = Carry::Idle;
        return;
    };
    forced.0 = None;
    commands.entity(ghost).try_despawn();
    if let Some(hint) = hint {
        commands.entity(hint).try_despawn();
    }
    if let Some(target) = target {
        commands.queue(move |world: &mut World| {
            open_in_panel(world, terminal, target);
        });
    }
}

/// Opens `terminal` in a new Terminal panel at `target`, taking it
/// from the panel that showed it, and gives the new one the keys.
fn open_in_panel(
    world: &mut World,
    terminal: Entity,
    target: DropTarget,
) {
    let Some(id) = open_numbered(world, PANEL, target) else {
        return;
    };
    let panel = PanelId(id);
    let mut panels = world.resource_mut::<Panels>();
    for shown in panels.0.values_mut() {
        if shown.active == Some(terminal) {
            shown.active = None;
        }
    }
    panels.edit(&panel).active = Some(terminal);
    world.resource_mut::<Handing>().0 = Some(panel);
}

/// What dropping at `cursor` does, and the rect to hint it with.
fn target_at(
    cursor: Vec2,
    bars: &[Bar],
    areas: &[(NodeId, Rect)],
) -> Option<(DropTarget, Rect)> {
    if let Some(bar) =
        bars.iter().find(|bar| bar.rect.contains(cursor))
    {
        return Some(over_bar(cursor, bar));
    }
    let &(leaf, rect) =
        areas.iter().find(|(_, rect)| rect.contains(cursor))?;
    Some(match edge_of(rect, cursor) {
        Some(edge) => {
            (DropTarget::Edge { leaf, edge }, edge_half(rect, edge))
        }
        None => (DropTarget::Area(leaf), rect),
    })
}

/// A slot beside the tab nearest `cursor`, hinted with that side of
/// the tab.
fn over_bar(cursor: Vec2, bar: &Bar) -> (DropTarget, Rect) {
    let nearest =
        bar.tabs.iter().enumerate().min_by(|(_, a), (_, b)| {
            let distance =
                |rect: &Rect| rect.center().distance_squared(cursor);
            distance(a).total_cmp(&distance(b))
        });
    let Some((index, tab)) = nearest else {
        let hint = Rect::new(
            bar.rect.min.x,
            bar.rect.min.y,
            bar.rect.min.x + EMPTY_HINT,
            bar.rect.max.y,
        );
        return (
            DropTarget::Tabs {
                leaf: bar.leaf,
                index: 0,
            },
            hint,
        );
    };
    let centre = tab.center().x;
    let after = cursor.x > centre;
    let hint = if after {
        Rect::new(centre, tab.min.y, tab.max.x, tab.max.y)
    } else {
        Rect::new(tab.min.x, tab.min.y, centre, tab.max.y)
    };
    let target = DropTarget::Tabs {
        leaf: bar.leaf,
        index: index + usize::from(after),
    };
    (target, hint)
}

/// The edge of `rect` the cursor is in the outer share of, if any.
fn edge_of(rect: Rect, cursor: Vec2) -> Option<Edge> {
    let from_centre = (cursor - rect.center()) / rect.size();
    if from_centre.x < -EDGE {
        Some(Edge::Left)
    } else if from_centre.x > EDGE {
        Some(Edge::Right)
    } else if from_centre.y > EDGE {
        Some(Edge::Bottom)
    } else if from_centre.y < -EDGE {
        Some(Edge::Top)
    } else {
        None
    }
}

/// The half of `rect` on the `edge` side.
fn edge_half(rect: Rect, edge: Edge) -> Rect {
    let centre = rect.center();
    match edge {
        Edge::Top => {
            Rect::new(rect.min.x, rect.min.y, rect.max.x, centre.y)
        }
        Edge::Bottom => {
            Rect::new(rect.min.x, centre.y, rect.max.x, rect.max.y)
        }
        Edge::Left => {
            Rect::new(rect.min.x, rect.min.y, centre.x, rect.max.y)
        }
        Edge::Right => {
            Rect::new(centre.x, rect.min.y, rect.max.x, rect.max.y)
        }
    }
}

/// The terminal carried, following the pointer.
fn spawn_ghost(
    commands: &mut Commands,
    name: String,
    cursor: Vec2,
    theme: &HamletTheme,
) -> Entity {
    let mut ghost = commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(cursor.x - GRAB.x),
            top: px(cursor.y - GRAB.y),
            height: px(theme.row()),
            padding: UiRect::horizontal(px(theme.tab_padding())),
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(theme.radius())),
            ..Node::default()
        },
        BackgroundColor(theme.fill()),
        GlobalZIndex(GHOST_Z),
        Pickable::IGNORE,
    ));
    ghost.with_children(|ghost| {
        ghost.spawn((
            Text::new(name),
            TextFont {
                font_size: FontSize::Px(theme.body_size()),
                weight: FontWeight::BOLD,
                ..TextFont::default()
            },
            TextColor(theme.tone(Tone::Body)),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..TextLayout::default()
            },
            Pickable::IGNORE,
        ));
    });
    ghost.id()
}

/// Where the terminal would open.
fn spawn_hint(
    commands: &mut Commands,
    rect: Rect,
    theme: &HamletTheme,
) -> Entity {
    let mut node = Node {
        position_type: PositionType::Absolute,
        border_radius: BorderRadius::all(px(theme.radius())),
        ..Node::default()
    };
    place(&mut node, rect);
    commands
        .spawn((
            node,
            BackgroundColor(theme.selection()),
            GlobalZIndex(HINT_Z),
            Pickable::IGNORE,
        ))
        .id()
}

/// Puts `node` over `rect`.
fn place(node: &mut Node, rect: Rect) {
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width());
    node.height = px(rect.height());
}

#[cfg(test)]
mod tests {
    use bevy_fynix::dock::{DockAreaStyle, DockLeaf, DockTree};

    use super::*;
    use crate::dock::PanelAppExt;

    #[test]
    fn a_terminal_dropped_opens_in_a_panel_of_its_own_and_leaves_its_old_one_empty()
     {
        let mut app = App::new();
        app.init_resource::<DockTree>()
            .init_resource::<Panels>()
            .init_resource::<Handing>()
            .add_numbered_panel(PANEL, super::super::window);
        let leaf =
            app.world_mut().resource_mut::<DockTree>().set_root_leaf(
                DockLeaf::new("low", DockAreaStyle::TabBar)
                    .with_windows(vec![PANEL.into()]),
            );
        let terminal = app.world_mut().spawn_empty().id();
        let first = PanelId::first();
        app.world_mut()
            .resource_mut::<Panels>()
            .edit(&first)
            .active = Some(terminal);
        open_in_panel(
            app.world_mut(),
            terminal,
            DropTarget::Edge {
                leaf,
                edge: Edge::Right,
            },
        );
        let opened = PanelId("terminal-2".into());
        let panels = app.world().resource::<Panels>();
        assert_eq!(panels.active(&first), None);
        assert_eq!(panels.active(&opened), Some(terminal));
        assert_eq!(app.world().resource::<Handing>().0, Some(opened));
        let tree = app.world().resource::<DockTree>();
        assert_ne!(
            tree.find_leaf_with_window("terminal-2"),
            tree.find_leaf_with_window(PANEL)
        );
    }
}
