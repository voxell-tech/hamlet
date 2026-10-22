//! A WASM plugin's retained panels: each node it makes built as one
//! of the shell's own widgets, in the engine's theme and styles, kept
//! in step with what it sets, and what the person does to it sent
//! back to the plugin.

use std::collections::HashMap;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::color::Color;
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
use bevy_fynix::patch::PatchInk;
use bevy_fynix::tokens::{
    Curve, Motion as Movement, MotionTokens, TextTokens, Tone,
};
use bevy_fynix::transition::blend_color;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{
    Bevy, Mounts, ReducedMotion, Signal, Theme, View, ViewExt,
    component, keyed,
};
use fynix::Patch;
use hamlet_engine_core::Budgets;
use hamlet_engine_plugins::ContributedBy;
use hamlet_engine_shell::fonts::DisplayText;
use hamlet_engine_shell::style::{ButtonStyle, action};
use hamlet_engine_shell::{AnyView, Cx, EngineTheme, StyleExt};

use crate::WasmCommandsExt;
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
pub(crate) struct Widgets(pub(crate) HashMap<u32, Built>);

#[derive(Clone, Copy)]
pub(crate) struct Built {
    pub(crate) entity: Entity,
    /// What the widget reads, on an entity of its own. A root has
    /// none, and is the host's.
    pub(crate) holder: Option<Entity>,
}

/// On the widget of a plugin's node: whose, and the plugin's key.
#[derive(Component, Clone, Copy, Debug)]
pub struct NodeOf {
    pub plugin: Entity,
    pub key: u32,
    hovered: bool,
}

impl NodeOf {
    /// The node pressed, `on`, or let go.
    fn press(&self, on: bool) -> NodeAction {
        NodeAction {
            plugin: self.plugin,
            key: self.key,
            input: NodeInput::Press(on),
        }
    }
}

/// On the widget of a node whose presses its plugin is told of.
#[derive(Component)]
struct TellsPresses;

/// The properties of a node, as its widget reads them.
#[derive(Component, Clone, PartialEq, Debug)]
struct Props {
    text: String,
    /// The colour of its text, by its tone.
    ink: Color,
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
            ink: theme.tone(Tone::Body),
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
            ink: Color::NONE,
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

/// A property of a node the engine can move, with its value.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Movable {
    Ink(Color),
    Size(f32),
    Gap(f32),
    Opacity(f32),
}

impl Movable {
    /// The value `value` sets, if it is one the engine moves.
    fn of(value: &Property, theme: &EngineTheme) -> Option<Self> {
        let gap = theme.space.gap;
        Some(match *value {
            Property::Tone(tone) => {
                Self::Ink(theme.tone(tone.into()))
            }
            Property::Size(size) => Self::Size(match size {
                wit::Size::Small => theme.text.small,
                wit::Size::Body => theme.text.body,
                wit::Size::Heading => theme.text.heading,
                wit::Size::Title => theme.text.title,
            }),
            Property::Space(space) => Self::Gap(match space {
                wit::Space::None => 0.0,
                wit::Space::Half => gap / 2.0,
                wit::Space::Gap => gap,
                wit::Space::Double => gap * 2.0,
            }),
            Property::Opacity(opacity) => Self::Opacity(opacity),
            _ => return None,
        })
    }

    /// The same property's value in `props`.
    fn read(self, props: &Props) -> Self {
        match self {
            Self::Ink(_) => Self::Ink(props.ink),
            Self::Size(_) => Self::Size(props.size),
            Self::Gap(_) => Self::Gap(props.gap),
            Self::Opacity(_) => Self::Opacity(props.opacity),
        }
    }

    fn write(self, props: &mut Props) {
        match self {
            Self::Ink(ink) => props.ink = ink,
            Self::Size(size) => props.size = size,
            Self::Gap(gap) => props.gap = gap,
            Self::Opacity(opacity) => props.opacity = opacity,
        }
    }

    /// `t` of the way from this to `to`, the same property.
    fn blend(self, to: Self, t: f32) -> Self {
        let mix = |from: f32, to: f32| from + (to - from) * t;
        match (self, to) {
            (Self::Ink(from), Self::Ink(to)) => {
                Self::Ink(blend_color(&from, &to, t))
            }
            (Self::Size(from), Self::Size(to)) => {
                Self::Size(mix(from, to))
            }
            (Self::Gap(from), Self::Gap(to)) => {
                Self::Gap(mix(from, to))
            }
            (Self::Opacity(from), Self::Opacity(to)) => {
                Self::Opacity(mix(from, to))
            }
            _ => to,
        }
    }

    fn is_same(self, other: Self) -> bool {
        core::mem::discriminant(&self)
            == core::mem::discriminant(&other)
    }
}

/// On the holder of a node's properties: the values the engine is
/// moving, each over a curve of its theme.
#[derive(Component, Default)]
struct Travels(Vec<Travel>);

#[derive(Clone, Copy)]
struct Travel {
    from: Movable,
    to: Movable,
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
    keyed(stopped_why, move |why| match why {
        Some(why) => stopped(why.clone()),
        None => AnyView::new(move |cx: &mut Cx| {
            let root = cx.build(column(()).gap(0.0));
            cx.world.entity_mut(surface).insert(UiRoot(root));
            root
        }),
    })
    .within(column(()).gap(0.0).grow(1.0))
    .in_panel()
}

/// The WASM host's methods on [`World`].
pub(crate) trait WasmWorldExt {
    /// Changes `plugin`'s widgets as its nodes were changed. `root`
    /// is a node the host made, with the widget it stands for.
    fn build_nodes(
        &mut self,
        plugin: Entity,
        root: Option<(u32, Entity)>,
        changes: Vec<NodeChange>,
    );

    /// Changes one of `plugin`'s `widgets` as its node was changed.
    fn change_node(
        &mut self,
        theme: &EngineTheme,
        mounts: &mut Mounts<EngineTheme>,
        plugin: Entity,
        widgets: &mut Widgets,
        change: NodeChange,
    );

    /// Sets `value` on the node whose widget is `entity` and whose
    /// properties are on `holder`: a [`Movable`] one over the theme's
    /// curve for `motion`, any other at once.
    fn set_node(
        &mut self,
        theme: &EngineTheme,
        entity: Entity,
        holder: Entity,
        value: Property,
        motion: Option<wit::Motion>,
    );
}

impl WasmWorldExt for World {
    fn build_nodes(
        &mut self,
        plugin: Entity,
        root: Option<(u32, Entity)>,
        changes: Vec<NodeChange>,
    ) {
        let built = self.contains_resource::<Mounts<EngineTheme>>()
            && self.contains_resource::<Theme<EngineTheme>>();
        let Some(mut widgets) =
            self.get_entity_mut(plugin).ok().filter(|_| built).map(
                |mut entity| {
                    entity.take::<Widgets>().unwrap_or_default()
                },
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
        self.resource_scope::<Mounts<EngineTheme>, _>(
            |world, mut mounts| {
                world.resource_scope::<Theme<EngineTheme>, _>(
                    |world, theme| {
                        for change in changes {
                            world.change_node(
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
        self.entity_mut(plugin).insert(widgets);
    }

    fn change_node(
        &mut self,
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
                    .filter(|&parent| {
                        self.get_entity(parent).is_ok()
                    })
                else {
                    return;
                };
                let holder = self
                    .spawn((Props::new(theme), ChildOf(plugin)))
                    .id();
                let view = widget(kind, holder, plugin, key);
                let entity = {
                    let mut cx = Cx::new(self, theme, &mut mounts.0);
                    cx.under(parent, |cx| view.build(cx))
                };
                self.entity_mut(entity).insert(NodeOf {
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
                self.set_node(theme, entity, holder, value, motion);
            }
            NodeChange::Order { node, children } => {
                let Some(entity) =
                    widgets.0.get(&node).map(|b| b.entity)
                else {
                    return;
                };
                let children = children
                    .iter()
                    .filter_map(|child| widgets.0.get(child))
                    .map(|built| built.entity)
                    .collect::<Vec<_>>();
                if let Ok(mut entity) = self.get_entity_mut(entity) {
                    entity.replace_children(&children);
                }
            }
            NodeChange::Watch { node, hover, press } => {
                let Some(mut entity) =
                    widgets.0.get(&node).and_then(|built| {
                        self.get_entity_mut(built.entity).ok()
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
                    if let Ok(gone) = self.get_entity_mut(gone) {
                        gone.despawn();
                    }
                }
            }
        }
    }

    fn set_node(
        &mut self,
        theme: &EngineTheme,
        entity: Entity,
        holder: Entity,
        value: Property,
        motion: Option<wit::Motion>,
    ) {
        if let Property::Size(wit::Size::Title) = value
            && let Ok(mut entity) = self.get_entity_mut(entity)
        {
            entity.insert(DisplayText);
        }
        let reduced = self
            .get_resource::<ReducedMotion>()
            .is_some_and(|reduced| reduced.0);
        let Ok(mut holder) = self.get_entity_mut(holder) else {
            return;
        };
        if let Some(to) = Movable::of(&value, theme) {
            let Some(from) =
                holder.get::<Props>().map(|p| to.read(p))
            else {
                return;
            };
            let mut travels =
                holder.take::<Travels>().unwrap_or_default();
            travels.0.retain(|travel| !travel.to.is_same(to));
            match motion.filter(|_| !reduced && from != to) {
                Some(motion) => {
                    let motion = match motion {
                        wit::Motion::Interact => Movement::Interact,
                        wit::Motion::Expand => Movement::Expand,
                    };
                    travels.0.push(Travel {
                        from,
                        to,
                        elapsed: Duration::ZERO,
                        curve: theme.motion(motion),
                    });
                }
                None => {
                    if let Some(mut props) = holder.get_mut::<Props>()
                    {
                        to.write(&mut props);
                    }
                }
            }
            if !travels.0.is_empty() {
                holder.insert(travels);
            }
            return;
        }
        let Some(mut props) = holder.get_mut::<Props>() else {
            return;
        };
        match value {
            Property::Text(text) => props.text = text,
            Property::Bold(bold) => props.bold = bold,
            Property::Style(style) => {
                props.style = match style {
                    wit::ButtonStyle::Plain => ButtonStyle::Plain,
                    wit::ButtonStyle::Primary => ButtonStyle::Primary,
                    wit::ButtonStyle::Critical => {
                        ButtonStyle::Critical
                    }
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
            Property::Tone(_)
            | Property::Size(_)
            | Property::Opacity(_)
            | Property::Space(_) => {}
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
        Kind::Label => AnyView::new(move |cx: &mut Cx| {
            let node = cx.build(
                label(read(holder, |props| props.text.clone()))
                    .size(read(holder, |props| props.size))
                    .bold(read(holder, |props| props.bold))
                    .opacity(read(holder, |props| props.opacity)),
            );
            // The colour moves between tones, which the label's own
            // tone cannot.
            cx.effect(
                node,
                read(holder, |props| props.ink).into(),
                |world, node, ink| {
                    <PatchInk as Patch<Bevy, Color>>::patch(
                        world, node, ink,
                    );
                },
            );
            node
        }),
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

impl From<wit::Tone> for Tone {
    fn from(tone: wit::Tone) -> Self {
        match tone {
            wit::Tone::Body => Self::Body,
            wit::Tone::Dim => Self::Dim,
            wit::Tone::Faint => Self::Faint,
            wit::Tone::Accent => Self::Accent,
            wit::Tone::Critical => Self::Critical,
        }
    }
}

fn pressed(
    press: On<Pointer<Press>>,
    nodes: Query<&NodeOf>,
    mut actions: MessageWriter<NodeAction>,
) {
    if let Ok(node) = nodes.get(press.event_target()) {
        actions.write(node.press(true));
    }
}

fn released(
    release: On<Pointer<Release>>,
    nodes: Query<&NodeOf>,
    mut actions: MessageWriter<NodeAction>,
) {
    if let Ok(node) = nodes.get(release.event_target()) {
        actions.write(node.press(false));
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

/// Moves on each value the engine is moving.
fn travel(
    mut holders: Query<(Entity, &mut Props, &mut Travels)>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (holder, mut props, mut travels) in &mut holders {
        travels.0.retain_mut(|travel| {
            travel.elapsed += time.delta();
            let duration = travel.curve.duration;
            if travel.elapsed >= duration {
                travel.to.write(&mut props);
                return false;
            }
            let progress =
                travel.elapsed.as_secs_f32() / duration.as_secs_f32();
            let eased = (travel.curve.ease)(progress);
            travel.from.blend(travel.to, eased).write(&mut props);
            true
        });
        if travels.0.is_empty() {
            commands.entity(holder).remove::<Travels>();
        }
    }
}

/// Takes away the widgets of a plugin that no longer runs.
fn forget_stopped(
    plugins: Query<(Entity, &Widgets), Without<Instance>>,
    mut commands: Commands,
) {
    for (plugin, widgets) in &plugins {
        commands.forget_widgets(plugin, widgets);
    }
}
