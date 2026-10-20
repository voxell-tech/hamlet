//! A WASM plugin's retained panels: each node it makes built as one
//! of the shell's own widgets, in the engine's theme and styles, kept
//! in step with what it sets, and what the person does to it sent
//! back to the plugin.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::message::{Message, MessageReader, MessageWriter};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::picking::events::{Pointer, Press, Release};
use bevy::picking::hover::Hovered as PickingHovered;
use bevy::time::Time;
use bevy::ui::{AlignItems, FlexDirection};
use bevy_fynix::tokens::{
    Curve, Motion as Movement, MotionTokens, Tone,
};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{
    Mounts, Signal, Theme, View, ViewExt, component, keyed,
};
use hamlet_engine_core::Budgets;
use hamlet_engine_plugins::ContributedBy;
use hamlet_engine_shell::fonts::DisplayText;
use hamlet_engine_shell::style::{ButtonStyle, action};
use hamlet_engine_shell::{AnyView, Cx, EngineTheme, panel};

use crate::budget::Spending;
use crate::instance::ui_bindings::hamlet::engine::ui::{
    self as wit, Kind, Property,
};
use crate::instance::{Instance, NodeInput, Spent};
use crate::nodes::NodeChange;
use crate::panel::{Drawn, Inbox, Shows, Work, serve, stopped};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<NodeStats>()
        .add_message::<NodeAction>()
        .add_systems(
            Update,
            (offer_roots, lose_roots, hovers, route)
                .chain()
                .before(serve),
        )
        .add_systems(Update, (travel, forget_stopped));
}

/// What retained panels cost, counted since the engine started.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct NodeStats {
    /// Calls into plugins for their retained panels.
    pub calls: u64,
    /// Calls the plugins made on their nodes, of every kind.
    pub node_calls: u64,
    /// Nodes made, set or ordered.
    pub changes: u64,
    /// Time in those calls, the plugins' and the host's.
    pub call_time: Duration,
    /// Time building and changing the widgets.
    pub apply_time: Duration,
}

impl NodeStats {
    pub(crate) fn count(&mut self, spent: &Spent) {
        self.calls += 1;
        self.node_calls += spent.node_calls;
        self.changes += u64::from(spent.changes);
        self.call_time += spent.time;
    }
}

/// What the person did to the widget of a plugin's node, named by
/// the plugin's key.
#[derive(Message, Clone, Copy, Debug)]
pub struct NodeAction {
    pub plugin: Entity,
    pub key: u32,
    pub input: NodeInput,
}

/// On a retained panel: the root its plugin builds under.
#[derive(Component, Clone, Copy, Debug)]
pub struct UiRoot(pub Entity);

/// On a plugin with retained panels: the widget built for each of
/// its nodes, by the number the host gave the node.
#[derive(Component, Default)]
pub(crate) struct Widgets(HashMap<u32, Built>);

#[derive(Clone, Copy)]
struct Built {
    entity: Entity,
    /// What the widget reads, on an entity of its own. A root has
    /// none, and is the host's.
    holder: Option<Entity>,
}

/// On the widget of a plugin's node: whose, and the plugin's key.
#[derive(Component, Clone, Copy, Debug)]
pub struct NodeOf {
    pub plugin: Entity,
    pub key: u32,
    hovered: bool,
}

/// On the widget of a node whose presses its plugin is told of.
#[derive(Component)]
struct TellsPresses;

/// The properties of a node, as its widget reads them.
#[derive(Component, Clone, PartialEq, Debug)]
struct Props {
    text: String,
    tone: Tone,
    size: f32,
    bold: bool,
    opacity: f32,
    style: ButtonStyle,
    direction: FlexDirection,
    align: AlignItems,
    gap: f32,
}

impl Props {
    fn new(theme: &EngineTheme) -> Self {
        Self {
            text: String::new(),
            tone: Tone::Body,
            size: theme.text.body,
            bold: false,
            opacity: 1.0,
            style: ButtonStyle::Plain,
            direction: FlexDirection::Column,
            align: AlignItems::Default,
            gap: theme.space.gap,
        }
    }

    /// The properties of a node whose holder is gone.
    fn gone() -> Self {
        Self {
            text: String::new(),
            tone: Tone::Body,
            size: 0.0,
            bold: false,
            opacity: 0.0,
            style: ButtonStyle::Plain,
            direction: FlexDirection::Column,
            align: AlignItems::Default,
            gap: 0.0,
        }
    }
}

/// On the holder of a node's properties: its opacity moving toward
/// `to`, as the engine animates it.
#[derive(Component, Clone, Copy)]
struct Travel {
    from: f32,
    to: f32,
    elapsed: Duration,
    curve: Curve,
}

/// The content of a WASM plugin's retained panel: a root its plugin
/// builds under, or why the plugin was stopped.
pub(crate) fn view(surface: Entity) -> AnyView {
    let stopped_why = component::<Drawn, _>(surface, |drawn| {
        match drawn.map(|drawn| &drawn.shows) {
            Some(Shows::Stopped(why)) => Some(why.clone()),
            _ => None,
        }
    });
    panel(
        keyed(stopped_why, move |why| match why {
            Some(why) => stopped(why.clone()),
            None => AnyView::new(move |cx: &mut Cx| {
                let root = cx.build(column(()).gap(0.0));
                cx.world.entity_mut(surface).insert(UiRoot(root));
                root
            }),
        })
        .within(column(()).gap(0.0).grow(1.0)),
    )
}

/// Changes `plugin`'s widgets as its nodes were changed, once the
/// commands are applied. `root` is a node the host made, with the
/// widget it stands for.
pub(crate) fn apply(
    commands: &mut Commands,
    plugin: Entity,
    root: Option<(u32, Entity)>,
    changes: Vec<NodeChange>,
) {
    if changes.is_empty() && root.is_none() {
        return;
    }
    commands.queue(move |world: &mut World| {
        let started = Instant::now();
        build(world, plugin, root, changes);
        if let Some(mut stats) = world.get_resource_mut::<NodeStats>()
        {
            stats.apply_time += started.elapsed();
        }
    });
}

fn build(
    world: &mut World,
    plugin: Entity,
    root: Option<(u32, Entity)>,
    changes: Vec<NodeChange>,
) {
    let built = world.contains_resource::<Mounts<EngineTheme>>()
        && world.contains_resource::<Theme<EngineTheme>>();
    let Some(mut widgets) =
        world.get_entity_mut(plugin).ok().filter(|_| built).map(
            |mut entity| entity.take::<Widgets>().unwrap_or_default(),
        )
    else {
        return;
    };
    if let Some((number, entity)) = root {
        widgets.0.insert(
            number,
            Built {
                entity,
                holder: None,
            },
        );
    }
    world.resource_scope::<Mounts<EngineTheme>, _>(
        |world, mut mounts| {
            world.resource_scope::<Theme<EngineTheme>, _>(
                |world, theme| {
                    for change in changes {
                        change_one(
                            world,
                            &theme.0,
                            &mut mounts,
                            plugin,
                            &mut widgets,
                            change,
                        );
                    }
                },
            );
        },
    );
    world.entity_mut(plugin).insert(widgets);
}

fn change_one(
    world: &mut World,
    theme: &EngineTheme,
    mounts: &mut Mounts<EngineTheme>,
    plugin: Entity,
    widgets: &mut Widgets,
    change: NodeChange,
) {
    match change {
        NodeChange::Make {
            node,
            key,
            kind,
            parent,
        } => {
            let Some(parent) = widgets
                .0
                .get(&parent)
                .map(|built| built.entity)
                .filter(|&parent| world.get_entity(parent).is_ok())
            else {
                return;
            };
            let holder = world
                .spawn((Props::new(theme), ChildOf(plugin)))
                .id();
            let view = widget(kind, holder, plugin, key);
            let entity = {
                let mut cx = Cx::new(world, theme, &mut mounts.0);
                cx.under(parent, |cx| view.build(cx))
            };
            world.entity_mut(entity).insert(NodeOf {
                plugin,
                key,
                hovered: false,
            });
            widgets.0.insert(
                node,
                Built {
                    entity,
                    holder: Some(holder),
                },
            );
        }
        NodeChange::Set {
            node,
            value,
            motion,
        } => {
            let Some(&Built {
                entity,
                holder: Some(holder),
            }) = widgets.0.get(&node)
            else {
                return;
            };
            set(world, theme, entity, holder, value, motion);
        }
        NodeChange::Order { node, children } => {
            let Some(entity) = widgets.0.get(&node).map(|b| b.entity)
            else {
                return;
            };
            let children = children
                .iter()
                .filter_map(|child| widgets.0.get(child))
                .map(|built| built.entity)
                .collect::<Vec<_>>();
            if let Ok(mut entity) = world.get_entity_mut(entity) {
                entity.replace_children(&children);
            }
        }
        NodeChange::Watch { node, hover, press } => {
            let Some(mut entity) =
                widgets.0.get(&node).and_then(|built| {
                    world.get_entity_mut(built.entity).ok()
                })
            else {
                return;
            };
            if hover && !entity.contains::<PickingHovered>() {
                entity.insert(PickingHovered::default());
            }
            if press && !entity.contains::<TellsPresses>() {
                entity
                    .insert(TellsPresses)
                    .observe(pressed)
                    .observe(released);
            }
        }
        NodeChange::Drop(node) => {
            let Some(Built {
                entity,
                holder: Some(holder),
            }) = widgets.0.remove(&node)
            else {
                return;
            };
            for gone in [entity, holder] {
                if let Ok(gone) = world.get_entity_mut(gone) {
                    gone.despawn();
                }
            }
        }
    }
}

/// The shell's widget for a node of `kind`, reading its properties
/// from `holder`.
fn widget(
    kind: Kind,
    holder: Entity,
    plugin: Entity,
    key: u32,
) -> AnyView {
    match kind {
        Kind::Frame => column(())
            .direction(read(holder, |props| props.direction))
            .gap(read(holder, |props| props.gap))
            .align(read(holder, |props| props.align))
            .boxed(),
        Kind::Label => {
            label(read(holder, |props| props.text.clone()))
                .tone(read(holder, |props| props.tone))
                .size(read(holder, |props| props.size))
                .bold(read(holder, |props| props.bold))
                .opacity(read(holder, |props| props.opacity))
                .boxed()
        }
        Kind::Button => {
            keyed(read(holder, |props| props.style), move |style| {
                action(
                    *style,
                    read(holder, |props| props.text.clone()),
                )
                .on_activate(move |world| {
                    world.write_message(NodeAction {
                        plugin,
                        key,
                        input: NodeInput::Click,
                    });
                })
                .boxed()
            })
            .within(row(()).gap(0.0))
            .boxed()
        }
    }
}

/// A signal of what `read` takes from the properties on `holder`.
fn read<T: Send + Sync + 'static>(
    holder: Entity,
    read: fn(&Props) -> T,
) -> Signal<T> {
    component::<Props, _>(holder, move |props| match props {
        Some(props) => read(props),
        None => read(&Props::gone()),
    })
}

/// Sets `value` on the node whose widget is `entity` and whose
/// properties are on `holder`.
fn set(
    world: &mut World,
    theme: &EngineTheme,
    entity: Entity,
    holder: Entity,
    value: Property,
    motion: Option<wit::Motion>,
) {
    if let Property::Size(wit::Size::Title) = value
        && let Ok(mut entity) = world.get_entity_mut(entity)
    {
        entity.insert(DisplayText);
    }
    let Ok(mut holder) = world.get_entity_mut(holder) else {
        return;
    };
    let travel = match (&value, motion) {
        (&Property::Opacity(to), Some(motion)) => {
            let from =
                holder.get::<Props>().map_or(to, |p| p.opacity);
            let motion = match motion {
                wit::Motion::Interact => Movement::Interact,
                wit::Motion::Expand => Movement::Expand,
            };
            Some(Travel {
                from,
                to,
                elapsed: Duration::ZERO,
                curve: theme.motion(motion),
            })
        }
        _ => None,
    };
    if let Some(travel) = travel {
        holder.insert(travel);
        return;
    }
    if matches!(value, Property::Opacity(_)) {
        holder.remove::<Travel>();
    }
    let Some(mut props) = holder.get_mut::<Props>() else {
        return;
    };
    match value {
        Property::Text(text) => props.text = text,
        Property::Tone(tone) => props.tone = tone_of(tone),
        Property::Size(size) => {
            props.size = match size {
                wit::Size::Small => theme.text.small,
                wit::Size::Body => theme.text.body,
                wit::Size::Heading => theme.text.heading,
                wit::Size::Title => theme.text.title,
            };
        }
        Property::Bold(bold) => props.bold = bold,
        Property::Opacity(opacity) => props.opacity = opacity,
        Property::Style(style) => {
            props.style = match style {
                wit::ButtonStyle::Plain => ButtonStyle::Plain,
                wit::ButtonStyle::Primary => ButtonStyle::Primary,
                wit::ButtonStyle::Critical => ButtonStyle::Critical,
                wit::ButtonStyle::Quiet => ButtonStyle::Quiet,
            };
        }
        Property::Direction(direction) => {
            (props.direction, props.align) = match direction {
                wit::Direction::Column => {
                    (FlexDirection::Column, AlignItems::Default)
                }
                wit::Direction::Row => {
                    (FlexDirection::Row, AlignItems::Center)
                }
            };
        }
        Property::Space(space) => {
            let gap = theme.space.gap;
            props.gap = match space {
                wit::Space::None => 0.0,
                wit::Space::Half => gap / 2.0,
                wit::Space::Gap => gap,
                wit::Space::Double => gap * 2.0,
            };
        }
    }
}

fn tone_of(tone: wit::Tone) -> Tone {
    match tone {
        wit::Tone::Body => Tone::Body,
        wit::Tone::Dim => Tone::Dim,
        wit::Tone::Faint => Tone::Faint,
        wit::Tone::Accent => Tone::Accent,
        wit::Tone::Critical => Tone::Critical,
    }
}

fn pressed(
    press: On<Pointer<Press>>,
    nodes: Query<&NodeOf>,
    mut actions: MessageWriter<NodeAction>,
) {
    tell(&nodes, press.event_target(), true, &mut actions);
}

fn released(
    release: On<Pointer<Release>>,
    nodes: Query<&NodeOf>,
    mut actions: MessageWriter<NodeAction>,
) {
    tell(&nodes, release.event_target(), false, &mut actions);
}

fn tell(
    nodes: &Query<&NodeOf>,
    entity: Entity,
    on: bool,
    actions: &mut MessageWriter<NodeAction>,
) {
    if let Ok(node) = nodes.get(entity) {
        actions.write(NodeAction {
            plugin: node.plugin,
            key: node.key,
            input: NodeInput::Press(on),
        });
    }
}

/// Asks each plugin to build its retained panel under the root just
/// built for it.
fn offer_roots(
    roots: Query<(&UiRoot, &Drawn, &ContributedBy), Changed<UiRoot>>,
    mut inboxes: Query<&mut Inbox>,
) {
    for (root, drawn, by) in &roots {
        if let Ok(mut inbox) = inboxes.get_mut(by.0) {
            inbox.0.push_back(Work::Mount {
                root: root.0,
                name: drawn.name.clone(),
            });
        }
    }
}

/// Tells each plugin of its retained panels whose root is gone, as
/// when the dock stops showing one.
fn lose_roots(
    roots: Query<(Entity, &UiRoot, &Drawn, &ContributedBy)>,
    alive: Query<()>,
    mut inboxes: Query<&mut Inbox>,
    mut commands: Commands,
) {
    for (surface, root, drawn, by) in &roots {
        if alive.contains(root.0) {
            continue;
        }
        commands.entity(surface).remove::<UiRoot>();
        if let Ok(mut inbox) = inboxes.get_mut(by.0) {
            inbox.0.push_back(Work::Unmount(drawn.name.clone()));
        }
    }
}

/// Tells each plugin when the pointer comes over a watched node of
/// its, or leaves it.
fn hovers(
    mut nodes: Query<
        (&mut NodeOf, &PickingHovered),
        Changed<PickingHovered>,
    >,
    mut actions: MessageWriter<NodeAction>,
) {
    for (mut node, hovered) in &mut nodes {
        let on = hovered.get();
        if node.hovered == on {
            continue;
        }
        node.hovered = on;
        actions.write(NodeAction {
            plugin: node.plugin,
            key: node.key,
            input: NodeInput::Hover(on),
        });
    }
}

/// Puts what the person did in the inbox of the plugin it was done
/// to, a second's events at most.
fn route(
    mut actions: MessageReader<NodeAction>,
    mut inboxes: Query<(&mut Inbox, &mut Spending)>,
    budgets: Res<Budgets>,
) {
    for action in actions.read() {
        let Ok((mut inbox, mut spending)) =
            inboxes.get_mut(action.plugin)
        else {
            continue;
        };
        if inbox.0.len() >= budgets.events_per_second as usize {
            spending.dropped_events += 1;
            continue;
        }
        inbox.0.push_back(Work::Node(action.key, action.input));
    }
}

/// Moves each opacity the engine animates on.
fn travel(
    mut holders: Query<(Entity, &mut Props, &mut Travel)>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (holder, mut props, mut travel) in &mut holders {
        travel.elapsed += time.delta();
        let duration = travel.curve.duration;
        if travel.elapsed >= duration {
            props.opacity = travel.to;
            commands.entity(holder).remove::<Travel>();
            continue;
        }
        let progress =
            travel.elapsed.as_secs_f32() / duration.as_secs_f32();
        let eased = (travel.curve.ease)(progress);
        props.opacity =
            travel.from + (travel.to - travel.from) * eased;
    }
}

/// Takes away the widgets of a plugin that no longer runs.
fn forget_stopped(
    plugins: Query<(Entity, &Widgets), Without<Instance>>,
    mut commands: Commands,
) {
    for (plugin, widgets) in &plugins {
        forget(&mut commands, plugin, widgets);
    }
}

/// Takes away every widget of `plugin`'s nodes but its roots.
pub(crate) fn forget(
    commands: &mut Commands,
    plugin: Entity,
    widgets: &Widgets,
) {
    for built in widgets.0.values() {
        if let Some(holder) = built.holder {
            commands.entity(built.entity).try_despawn();
            commands.entity(holder).try_despawn();
        }
    }
    commands.entity(plugin).try_remove::<Widgets>();
}

/// Has a reloaded plugin build its retained panels again: its
/// widgets are taken away, and each panel shown is offered anew.
pub(crate) fn remount(
    commands: &mut Commands,
    plugin: Entity,
    widgets: Option<&Widgets>,
    roots: &Query<(Entity, &UiRoot, &ContributedBy)>,
) {
    if let Some(widgets) = widgets {
        forget(commands, plugin, widgets);
    }
    for (surface, root, by) in roots {
        if by.0 == plugin {
            commands.entity(surface).insert(*root);
        }
    }
}
