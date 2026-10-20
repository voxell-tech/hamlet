//! A WASM plugin's retained panels: each node it makes built as one
//! of the shell's own widgets, in the engine's theme and styles, kept
//! in step with what it sets, and what the person does to it sent
//! back to the plugin.
//!
//! Each node has a holder, an entity of its own with its properties
//! and, for a node that holds others, them in order. Its widget reads
//! the holder: the widgets of what a node holds are an `each` over
//! that list, so they are built under the rules of the widget they
//! are in.

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
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::{
    resource_exists, resource_exists_and_changed,
};
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::picking::events::{Pointer, Press, Release};
use bevy::picking::hover::Hovered as PickingHovered;
use bevy::time::Time;
use bevy::ui::{AlignItems, FlexDirection, Node, px};
use bevy_fynix::patch::{PatchInk, PatchOpacity};
use bevy_fynix::tokens::{
    Motion as Movement, MotionTokens, TextTokens, Tone,
};
use bevy_fynix::transition::blend_color;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, label};
use bevy_fynix::{
    Bevy, Each, Keyed, ReducedMotion, Signal, ViewExt, component,
    each, keyed,
};
use fynix::Patch;
use hamlet_engine_api::PanelId;
use hamlet_engine_core::Budgets;
use hamlet_engine_plugins::{
    ContributedBy, Contributions, PluginInfo,
};
use hamlet_engine_shell::fonts::DisplayText;
use hamlet_engine_shell::icons::{Glyph, glyph};
use hamlet_engine_shell::style::{
    ButtonStyle, action, content_button, divider, dropdown, field,
    icon_row, list_row, raised, scroll_area, toggle,
};
use hamlet_engine_shell::{
    AnyView, Cx, EngineTheme, Overlay, OverlayShown, Panel,
    SettingsPage, ShellWorldExt, StatusItem, StyleExt,
};

use crate::WasmCommandsExt;
use crate::budget::Spending;
use crate::instance::ui_bindings::hamlet::engine::ui::{
    self as wit, Kind, Property,
};
use crate::instance::{Instance, NodeInput, Spent};
use crate::nodes::NodeChange;
use crate::panel::{Inbox, Surface, Work, serve, stopped};
use crate::surfaces::SurfaceChange;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<NodeStats>()
        .add_message::<NodeAction>()
        .add_systems(
            Update,
            (offer_roots, lose_roots, hovers, route)
                .chain()
                .before(serve),
        )
        .add_systems(
            Update,
            (
                (
                    restyle.run_if(
                        resource_exists_and_changed::<EngineTheme>,
                    ),
                    travel,
                )
                    .chain()
                    .run_if(resource_exists::<EngineTheme>),
                forget_stopped,
            ),
        );
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
    /// Time changing the holders of the nodes.
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
#[derive(Message, Clone, Debug)]
pub struct NodeAction {
    pub plugin: Entity,
    pub key: u32,
    pub input: NodeInput,
}

/// On a retained panel: the widget its plugin builds under, and the
/// holder of the root node the plugin is given.
#[derive(Component, Clone, Copy, Debug)]
pub struct UiRoot {
    pub widget: Entity,
    pub holder: Entity,
}

/// On a plugin with retained panels: the holder of each of its
/// nodes, by the number the host gave the node.
#[derive(Component, Default)]
pub(crate) struct Holders(pub(crate) HashMap<u32, Holder>);

#[derive(Clone, Copy)]
pub(crate) struct Holder {
    pub(crate) entity: Entity,
    /// Whether it holds a panel's root, which is the host's.
    pub(crate) root: bool,
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

/// The properties of a node, as its widget reads them. A value the
/// engine moves is shown as it is now, filled in from its role.
#[derive(Component, Clone, PartialEq, Debug)]
struct Props {
    text: String,
    /// The colour of its text, by its tone.
    ink: Color,
    size: f32,
    bold: bool,
    face: wit::Face,
    opacity: f32,
    style: ButtonStyle,
    direction: FlexDirection,
    align: AlignItems,
    gap: f32,
    placeholder: String,
    on: bool,
    offset: Vec2,
    options: Vec<String>,
    selected: u32,
    /// The roles its ink, size and gap are filled in from.
    tone: wit::Tone,
    text_size: wit::Size,
    space: wit::Space,
    /// The properties the plugin set, a bit each by
    /// [`Property::bit`].
    set: u16,
    /// Whether its plugin is told when the pointer comes over it or
    /// leaves, and when it is pressed or let go.
    hover: bool,
    press: bool,
}

impl Props {
    fn new(theme: &EngineTheme) -> Self {
        let mut props = Self::gone();
        props.restyle(theme, None);
        props
    }

    /// The properties of a node whose holder is gone.
    fn gone() -> Self {
        Self {
            text: String::new(),
            ink: Color::NONE,
            size: 0.0,
            bold: false,
            face: wit::Face::Body,
            opacity: 1.0,
            style: ButtonStyle::Plain,
            direction: FlexDirection::Column,
            align: AlignItems::Default,
            gap: 0.0,
            placeholder: String::new(),
            on: false,
            offset: Vec2::ZERO,
            options: Vec::new(),
            selected: 0,
            tone: wit::Tone::Body,
            text_size: wit::Size::Body,
            space: wit::Space::Gap,
            set: 0,
            hover: false,
            press: false,
        }
    }

    fn has(&self, property: u16) -> bool {
        self.set & property != 0
    }

    /// Fills in the values of its roles from `theme`, but those the
    /// engine is moving, `travels`.
    fn restyle(
        &mut self,
        theme: &EngineTheme,
        travels: Option<&Travels>,
    ) {
        let roles = [
            Target::Tone(self.tone),
            Target::Size(self.text_size),
            Target::Space(self.space),
        ];
        for role in roles {
            let moving = travels.is_some_and(|travels| {
                travels.0.iter().any(|travel| travel.to.is_same(role))
            });
            if !moving {
                role.resolve(theme).write(self);
            }
        }
    }

    /// Sets `value`, one the engine does not move.
    fn apply(&mut self, value: Property) {
        match value {
            Property::Text(text) => self.text = text,
            Property::Bold(bold) => self.bold = bold,
            Property::Face(face) => self.face = face,
            Property::Style(style) => {
                self.style = match style {
                    wit::ButtonStyle::Plain => ButtonStyle::Plain,
                    wit::ButtonStyle::Primary => ButtonStyle::Primary,
                    wit::ButtonStyle::Critical => {
                        ButtonStyle::Critical
                    }
                    wit::ButtonStyle::Quiet => ButtonStyle::Quiet,
                };
            }
            Property::Direction(direction) => {
                (self.direction, self.align) = match direction {
                    wit::Direction::Column => {
                        (FlexDirection::Column, AlignItems::Default)
                    }
                    wit::Direction::Row => {
                        (FlexDirection::Row, AlignItems::Center)
                    }
                };
            }
            Property::Placeholder(text) => self.placeholder = text,
            Property::On(on) => self.on = on,
            Property::Options(options) => self.options = options,
            Property::Selected(at) => self.selected = at,
            Property::Tone(tone) => self.tone = tone,
            Property::Size(size) => self.text_size = size,
            Property::Space(space) => self.space = space,
            Property::Opacity(_) | Property::Offset(_) => {}
        }
    }
}

/// The bits of [`Props::set`] its widget is built by.
const TONE: u16 = 1 << 1;
const SIZE: u16 = 1 << 2;
const BOLD: u16 = 1 << 3;
const OPACITY: u16 = 1 << 5;
const OFFSET: u16 = 1 << 11;

impl Property {
    /// The bit of the property this sets, in the order the WIT names
    /// them.
    fn bit(&self) -> u16 {
        let at = match self {
            Property::Text(_) => 0,
            Property::Tone(_) => 1,
            Property::Size(_) => 2,
            Property::Bold(_) => 3,
            Property::Face(_) => 4,
            Property::Opacity(_) => 5,
            Property::Style(_) => 6,
            Property::Direction(_) => 7,
            Property::Space(_) => 8,
            Property::Placeholder(_) => 9,
            Property::On(_) => 10,
            Property::Offset(_) => 11,
            Property::Options(_) => 12,
            Property::Selected(_) => 13,
        };
        1 << at
    }
}

/// On the holder of a node that holds others: them, in order.
#[derive(Component, Clone, Default)]
pub(crate) struct Kids(Vec<Child>);

/// A node under another, as the other's widget builds it.
#[derive(Clone, PartialEq, Debug)]
struct Child {
    holder: Entity,
    key: u32,
    kind: Kind,
    shape: Shape,
}

/// What a node's widget is built as, past what it reads from its
/// holder. A change of it builds the widget again.
#[derive(Clone, PartialEq, Debug)]
enum Shape {
    Plain,
    Label {
        /// The bits of [`Props::set`] it binds.
        set: u16,
        display: bool,
    },
    Button {
        style: ButtonStyle,
        holds: bool,
    },
    Icon {
        name: String,
        tinted: bool,
    },
    Field {
        placeholder: String,
    },
    Dropdown {
        options: Vec<String>,
    },
}

impl Shape {
    /// The shape of a node of `kind` with `props`, which `holds`
    /// others or not.
    fn of(kind: Kind, props: &Props, holds: bool) -> Self {
        match kind {
            Kind::Label => Self::Label {
                set: props.set & (TONE | SIZE | BOLD),
                display: props.face == wit::Face::Display,
            },
            Kind::Button => Self::Button {
                style: props.style,
                holds,
            },
            Kind::Icon => Self::Icon {
                name: props.text.clone(),
                tinted: props.has(TONE),
            },
            Kind::Field => Self::Field {
                placeholder: props.placeholder.clone(),
            },
            Kind::Dropdown => Self::Dropdown {
                options: props.options.clone(),
            },
            _ => Self::Plain,
        }
    }
}

/// Where a node's widget is built: what holds it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Place {
    Frame,
    Button,
    List,
}

/// What a property the engine can move is set to: a role of the
/// theme, or a value of the plugin's.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    Tone(wit::Tone),
    Size(wit::Size),
    Space(wit::Space),
    Opacity(f32),
    Offset(Vec2),
}

impl Target {
    /// What `value` sets, if it is one the engine moves.
    fn of(value: &Property) -> Option<Self> {
        Some(match *value {
            Property::Tone(tone) => Self::Tone(tone),
            Property::Size(size) => Self::Size(size),
            Property::Space(space) => Self::Space(space),
            Property::Opacity(opacity) => Self::Opacity(opacity),
            Property::Offset((x, y)) => Self::Offset(Vec2::new(x, y)),
            _ => return None,
        })
    }

    /// Its value, its role filled in from `theme`.
    fn resolve(self, theme: &EngineTheme) -> Shown {
        let gap = theme.space.gap;
        match self {
            Self::Tone(tone) => Shown::Ink(theme.tone(tone.into())),
            Self::Size(size) => Shown::Size(match size {
                wit::Size::Small => theme.text.small,
                wit::Size::Body => theme.text.body,
                wit::Size::Heading => theme.text.heading,
                wit::Size::Title => theme.text.title,
            }),
            Self::Space(space) => Shown::Gap(match space {
                wit::Space::None => 0.0,
                wit::Space::Half => gap / 2.0,
                wit::Space::Gap => gap,
                wit::Space::Double => gap * 2.0,
            }),
            Self::Opacity(opacity) => Shown::Opacity(opacity),
            Self::Offset(offset) => Shown::Offset(offset),
        }
    }

    /// Whether `other` sets the same property.
    fn is_same(self, other: Self) -> bool {
        core::mem::discriminant(&self)
            == core::mem::discriminant(&other)
    }
}

/// A value of a property the engine can move, as it is shown.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Shown {
    Ink(Color),
    Size(f32),
    Gap(f32),
    Opacity(f32),
    Offset(Vec2),
}

impl Shown {
    /// The value of the property `target` sets, in `props`.
    fn read(target: Target, props: &Props) -> Self {
        match target {
            Target::Tone(_) => Self::Ink(props.ink),
            Target::Size(_) => Self::Size(props.size),
            Target::Space(_) => Self::Gap(props.gap),
            Target::Opacity(_) => Self::Opacity(props.opacity),
            Target::Offset(_) => Self::Offset(props.offset),
        }
    }

    fn write(self, props: &mut Props) {
        match self {
            Self::Ink(ink) => props.ink = ink,
            Self::Size(size) => props.size = size,
            Self::Gap(gap) => props.gap = gap,
            Self::Opacity(opacity) => props.opacity = opacity,
            Self::Offset(offset) => props.offset = offset,
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
            (Self::Offset(from), Self::Offset(to)) => {
                Self::Offset(from.lerp(to, t))
            }
            _ => to,
        }
    }
}

/// On the holder of a node's properties: the values the engine is
/// moving, each over the theme's curve for its motion.
#[derive(Component, Default)]
struct Travels(Vec<Travel>);

/// A value moving from where it was shown to what its target is in
/// the theme as it is now.
#[derive(Clone, Copy)]
struct Travel {
    from: Shown,
    to: Target,
    elapsed: Duration,
    motion: Movement,
}

/// The content of a WASM plugin's panel, inside the panel's padding.
pub(crate) fn view(surface: Entity) -> AnyView {
    shown(surface)
        .within(column(()).gap(0.0).grow(1.0))
        .in_panel()
}

/// The content of a WASM plugin's settings page.
pub(crate) fn page(surface: Entity) -> AnyView {
    shown(surface).within(column(()).gap(0.0)).boxed()
}

/// A root the plugin of `surface` builds under, or why the plugin
/// was stopped.
fn shown(surface: Entity) -> Keyed<EngineTheme, Option<String>> {
    let stopped_why = component::<Surface, _>(surface, |shown| {
        shown.and_then(|shown| shown.stopped.clone())
    });
    keyed(stopped_why, move |why| match why {
        Some(why) => stopped(why.clone()),
        None => root(surface),
    })
}

/// The root a plugin builds a panel or settings page under, with a
/// holder of its own that goes with `surface`.
fn root(surface: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let plugin = cx
            .world
            .get::<ContributedBy>(surface)
            .map_or(Entity::PLACEHOLDER, |by| by.0);
        let holder =
            cx.world.spawn((Kids::default(), ChildOf(surface))).id();
        let widget = cx.build(
            kids(holder, plugin, Place::Frame)
                .within(column(()).gap(0.0)),
        );
        cx.world
            .entity_mut(surface)
            .insert(UiRoot { widget, holder });
        widget
    })
}

/// The widgets of what the node of `holder` holds, in order.
fn kids(
    holder: Entity,
    plugin: Entity,
    place: Place,
) -> Each<EngineTheme, Child, (Entity, Shape)> {
    each(
        component::<Kids, _>(holder, |kids| {
            kids.map_or_else(Vec::new, |kids| kids.0.clone())
        }),
        |child| (child.holder, child.shape.clone()),
        move |child| widget(child, plugin, place, holder),
    )
}

/// The shell's widget for `child`, a node of `plugin` held by the
/// node of `parent` as `place` says.
fn widget(
    child: &Child,
    plugin: Entity,
    place: Place,
    parent: Entity,
) -> AnyView {
    let (holder, key) = (child.holder, child.key);
    let built = match (child.kind, &child.shape) {
        (Kind::Frame, _) => kids(holder, plugin, Place::Frame)
            .within(
                column(())
                    .direction(read(holder, |props| props.direction))
                    .gap(read(holder, |props| props.gap))
                    .align(read(holder, |props| props.align)),
            )
            .boxed(),
        (Kind::Label, &Shape::Label { set, display }) => {
            inked(text(holder, set, display), holder, set & TONE != 0)
        }
        (Kind::Button, &Shape::Button { style, holds }) => {
            let click = move |world: &mut World| {
                world.write_message(NodeAction {
                    plugin,
                    key,
                    input: NodeInput::Click,
                });
            };
            if holds {
                content_button(
                    style,
                    kids(holder, plugin, Place::Button)
                        .within(icon_row(())),
                )
                .on_activate(click)
                .boxed()
            } else {
                action(
                    style,
                    read(holder, |props| props.text.clone()),
                )
                .on_activate(click)
                .boxed()
            }
        }
        (Kind::Icon, Shape::Icon { name, tinted }) => {
            let icon = inked(icon(name), holder, *tinted);
            if place == Place::Button {
                raised(icon)
            } else {
                icon
            }
        }
        (Kind::Toggle, _) => toggle(
            read(holder, |props| props.on),
            move |world, on| {
                world.write_message(NodeAction {
                    plugin,
                    key,
                    input: NodeInput::Toggle(on),
                });
            },
        ),
        (Kind::Field, Shape::Field { placeholder }) => field(
            read(holder, |props| props.text.clone()),
            placeholder.clone(),
            move |world, text| {
                world.write_message(NodeAction {
                    plugin,
                    key,
                    input: NodeInput::Commit(text),
                });
            },
        ),
        (Kind::List, _) => kids(holder, plugin, Place::List)
            .within(column(()).gap(0.0))
            .boxed(),
        (Kind::Scroll, _) => kids(holder, plugin, Place::Frame)
            .within(scroll_area(()))
            .boxed(),
        (Kind::Divider, _) => divider(),
        (Kind::Dropdown, Shape::Dropdown { options }) => dropdown(
            options.clone(),
            read(holder, |props| props.selected as usize),
            move |world, at| {
                world.write_message(NodeAction {
                    plugin,
                    key,
                    input: NodeInput::Pick(at as u32),
                });
            },
        ),
        _ => column(()).boxed(),
    };
    let built = node(built, holder, plugin, key);
    if place != Place::List {
        return built;
    }
    let divided = component::<Kids, _>(parent, move |kids| {
        kids.and_then(|kids| kids.0.first())
            .is_some_and(|first| first.holder != holder)
    });
    list_row(built, divided)
}

/// A label reading the node of `holder`, its tone, size and boldness
/// left to the rules it is built under unless `set` has them.
fn text(holder: Entity, set: u16, display: bool) -> AnyView {
    let mut text = label(read(holder, |props| props.text.clone()));
    if set & SIZE != 0 {
        text = text.size(read(holder, |props| props.size));
    }
    if set & BOLD != 0 {
        text = text.bold(read(holder, |props| props.bold));
    }
    if display {
        text.wrap(false).tagged(DisplayText).boxed()
    } else {
        text.boxed()
    }
}

/// The icon of the engine's set named `name`; a name the set does not
/// have is shown as it is.
fn icon(name: &str) -> AnyView {
    let Some(which) = Glyph::named(name) else {
        return label(name.to_owned()).tone(Tone::Dim).boxed();
    };
    AnyView::new(move |cx: &mut Cx| {
        let size = cx.theme().space.icon;
        cx.build(glyph(which, size))
    })
}

/// `view`, its colour the ink of the node of `holder` when `tinted`.
/// The colour moves between tones, which a view's own tone cannot.
fn inked(view: AnyView, holder: Entity, tinted: bool) -> AnyView {
    if !tinted {
        return view;
    }
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(view);
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
    })
}

/// `view` as the widget of the node `key` of `plugin`: named for its
/// inputs, watched as the plugin asks, and as opaque and as far
/// moved as it is set.
fn node(
    view: AnyView,
    holder: Entity,
    plugin: Entity,
    key: u32,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(view);
        cx.world.entity_mut(node).insert(NodeOf {
            plugin,
            key,
            hovered: false,
        });
        cx.effect(
            node,
            read(holder, |props| (props.hover, props.press)).into(),
            watch,
        );
        cx.effect(
            node,
            read(holder, |props| {
                props.has(OPACITY).then_some(props.opacity)
            })
            .into(),
            |world, node, opacity| {
                if let Some(opacity) = opacity {
                    <PatchOpacity as Patch<Bevy, f32>>::patch(
                        world, node, opacity,
                    );
                }
            },
        );
        cx.effect(
            node,
            read(holder, |props| {
                props.has(OFFSET).then_some(props.offset)
            })
            .into(),
            |world, node, offset| {
                if let Some(offset) = offset
                    && let Some(mut laid) =
                        world.get_mut::<Node>(node)
                {
                    laid.left = px(offset.x);
                    laid.top = px(offset.y);
                }
            },
        );
        node
    })
}

/// Has the widget `node` tell its plugin of the pointer over it, if
/// `hover`, and of presses on it, if `press`.
fn watch(
    world: &mut World,
    node: Entity,
    &(hover, press): &(bool, bool),
) {
    let Ok(mut entity) = world.get_entity_mut(node) else {
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

/// The WASM host's methods on [`World`].
pub(crate) trait WasmWorldExt {
    /// Changes the holders of `plugin`'s nodes as its nodes were
    /// changed. `root` is a node the host made, with its holder.
    fn build_nodes(
        &mut self,
        plugin: Entity,
        root: Option<(u32, Entity)>,
        changes: Vec<NodeChange>,
    );

    /// Changes one of the holders of a plugin's nodes as its node was
    /// changed.
    fn change_node(
        &mut self,
        theme: &EngineTheme,
        holders: &mut Holders,
        change: NodeChange,
    );

    /// Sets `value` on the node whose properties are on `holder`: one
    /// the engine moves over the theme's curve for `motion`, any
    /// other at once.
    fn set_node(
        &mut self,
        theme: &EngineTheme,
        holder: Entity,
        value: Property,
        motion: Option<wit::Motion>,
    );

    /// Builds the widget of the node of `holder` again if its shape
    /// changed.
    fn reshape(&mut self, holder: Entity);

    /// Changes what `plugin` shows outside its panels, each change
    /// to what it added under the name the change gives.
    fn change_surfaces(
        &mut self,
        plugin: Entity,
        changes: Vec<SurfaceChange>,
    );

    /// What `plugin` added under the name `name`, of the kind `C`
    /// names, by the id `id` reads from it.
    fn added_named<C: Component>(
        &self,
        plugin: Entity,
        name: &str,
        id: fn(&C) -> &PanelId,
    ) -> Option<Entity>;
}

impl WasmWorldExt for World {
    fn build_nodes(
        &mut self,
        plugin: Entity,
        root: Option<(u32, Entity)>,
        changes: Vec<NodeChange>,
    ) {
        if !self.contains_resource::<EngineTheme>() {
            return;
        }
        let Ok(mut entity) = self.get_entity_mut(plugin) else {
            return;
        };
        let mut holders =
            entity.take::<Holders>().unwrap_or_default();
        if let Some((number, entity)) = root {
            holders.0.insert(number, Holder { entity, root: true });
        }
        self.resource_scope::<EngineTheme, _>(|world, theme| {
            for change in changes {
                world.change_node(&theme, &mut holders, change);
            }
        });
        self.entity_mut(plugin).insert(holders);
    }

    fn change_node(
        &mut self,
        theme: &EngineTheme,
        holders: &mut Holders,
        change: NodeChange,
    ) {
        let holder_of = |node: &u32| {
            holders.0.get(node).map(|holder| holder.entity)
        };
        match change {
            NodeChange::Make {
                node,
                key,
                kind,
                parent,
            } => {
                let Some(parent) =
                    holder_of(&parent).filter(|&parent| {
                        self.get_entity(parent).is_ok()
                    })
                else {
                    return;
                };
                let props = Props::new(theme);
                let shape = Shape::of(kind, &props, false);
                let mut made = self.spawn((props, ChildOf(parent)));
                if kind.holds() {
                    made.insert(Kids::default());
                }
                let holder = made.id();
                if let Some(mut kids) = self.get_mut::<Kids>(parent) {
                    kids.0.push(Child {
                        holder,
                        key,
                        kind,
                        shape,
                    });
                }
                holders.0.insert(
                    node,
                    Holder {
                        entity: holder,
                        root: false,
                    },
                );
                self.reshape(parent);
            }
            NodeChange::Set {
                node,
                value,
                motion,
            } => {
                let Some(holder) = holder_of(&node) else {
                    return;
                };
                self.set_node(theme, holder, value, motion);
                self.reshape(holder);
            }
            NodeChange::Order { node, children } => {
                let order = children
                    .iter()
                    .filter_map(holder_of)
                    .collect::<Vec<_>>();
                let Some(mut kids) = holder_of(&node)
                    .and_then(|holder| self.get_mut::<Kids>(holder))
                else {
                    return;
                };
                let rank = |child: &Child| {
                    order
                        .iter()
                        .position(|&holder| holder == child.holder)
                        .unwrap_or(order.len())
                };
                kids.0.sort_by_key(rank);
            }
            NodeChange::Watch { node, hover, press } => {
                let Some(mut props) = holder_of(&node)
                    .and_then(|holder| self.get_mut::<Props>(holder))
                else {
                    return;
                };
                props.hover |= hover;
                props.press |= press;
            }
            NodeChange::Drop(node) => {
                let Some(holder) = holders.0.remove(&node) else {
                    return;
                };
                if holder.root {
                    return;
                }
                let Ok(gone) = self.get_entity_mut(holder.entity)
                else {
                    return;
                };
                let parent =
                    gone.get::<ChildOf>().map(ChildOf::parent);
                gone.despawn();
                let Some(parent) = parent else {
                    return;
                };
                if let Some(mut kids) = self.get_mut::<Kids>(parent) {
                    kids.0.retain(|kid| kid.holder != holder.entity);
                }
                self.reshape(parent);
            }
        }
    }

    fn set_node(
        &mut self,
        theme: &EngineTheme,
        holder: Entity,
        value: Property,
        motion: Option<wit::Motion>,
    ) {
        let reduced = self
            .get_resource::<ReducedMotion>()
            .is_some_and(|reduced| reduced.0);
        let Ok(mut holder) = self.get_entity_mut(holder) else {
            return;
        };
        let Some(mut props) = holder.get_mut::<Props>() else {
            return;
        };
        props.set |= value.bit();
        let target = Target::of(&value);
        props.apply(value);
        let Some(to) = target else {
            return;
        };
        let from = Shown::read(to, &props);
        let motion =
            motion.filter(|_| !reduced && from != to.resolve(theme));
        if motion.is_none() {
            to.resolve(theme).write(&mut props);
        }
        let mut travels =
            holder.take::<Travels>().unwrap_or_default();
        travels.0.retain(|travel| !travel.to.is_same(to));
        if let Some(motion) = motion {
            travels.0.push(Travel {
                from,
                to,
                elapsed: Duration::ZERO,
                motion: match motion {
                    wit::Motion::Interact => Movement::Interact,
                    wit::Motion::Expand => Movement::Expand,
                },
            });
        }
        if !travels.0.is_empty() {
            holder.insert(travels);
        }
    }

    fn reshape(&mut self, holder: Entity) {
        let Ok(entity) = self.get_entity(holder) else {
            return;
        };
        let (Some(props), Some(parent)) =
            (entity.get::<Props>(), entity.get::<ChildOf>())
        else {
            return;
        };
        let holds =
            entity.get::<Kids>().is_some_and(|k| !k.0.is_empty());
        let parent = parent.parent();
        let Some(kind) = self.get::<Kids>(parent).and_then(|kids| {
            kids.0
                .iter()
                .find(|kid| kid.holder == holder)
                .map(|kid| kid.kind)
        }) else {
            return;
        };
        let shape = Shape::of(kind, props, holds);
        let Some(mut kids) = self.get_mut::<Kids>(parent) else {
            return;
        };
        if let Some(kid) =
            kids.0.iter_mut().find(|kid| kid.holder == holder)
            && kid.shape != shape
        {
            kid.shape = shape;
        }
    }

    fn change_surfaces(
        &mut self,
        plugin: Entity,
        changes: Vec<SurfaceChange>,
    ) {
        for change in changes {
            match change {
                SurfaceChange::Status { name, item } => {
                    let Some(at) = self.added_named::<StatusItem>(
                        plugin,
                        &name,
                        |shown| &shown.id,
                    ) else {
                        continue;
                    };
                    let Some(mut shown) =
                        self.get_mut::<StatusItem>(at)
                    else {
                        continue;
                    };
                    let now = StatusItem {
                        id: shown.id.clone(),
                        icon: Glyph::named(&item.icon),
                        text: item.text,
                        tone: item.tone.into(),
                        tip: item.tip,
                    };
                    if *shown != now {
                        *shown = now;
                    }
                }
                SurfaceChange::Overlay { name, shown } => {
                    let Some(at) = self.added_named::<Overlay>(
                        plugin,
                        &name,
                        |overlay| &overlay.id,
                    ) else {
                        continue;
                    };
                    let mut overlay = self.entity_mut(at);
                    if shown {
                        overlay.insert(OverlayShown);
                    } else {
                        overlay.remove::<OverlayShown>();
                    }
                }
                SurfaceChange::Open(name) => {
                    if let Some(page) = self
                        .added_named::<SettingsPage>(
                            plugin,
                            &name,
                            |page| &page.id,
                        )
                    {
                        self.open_settings_page(page);
                    } else if let Some(panel) = self
                        .added_named::<Panel>(
                            plugin,
                            &name,
                            |panel| &panel.id,
                        )
                        .and_then(|panel| self.get::<Panel>(panel))
                        .map(|panel| panel.id.clone())
                    {
                        self.open_panel(&panel);
                    }
                }
            }
        }
    }

    fn added_named<C: Component>(
        &self,
        plugin: Entity,
        name: &str,
        id: fn(&C) -> &PanelId,
    ) -> Option<Entity> {
        let owner = self.get::<PluginInfo>(plugin)?.id.as_str();
        let wanted = PanelId::new(owner, name).ok()?;
        self.get::<Contributions>(plugin)?.iter().find(|&added| {
            self.get::<C>(added).is_some_and(|it| *id(it) == wanted)
        })
    }
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

/// Asks each plugin to build its panel or settings page under the
/// root just built for it.
fn offer_roots(
    roots: Query<
        (&UiRoot, &Surface, &ContributedBy),
        Changed<UiRoot>,
    >,
    mut inboxes: Query<&mut Inbox>,
) {
    for (root, shown, by) in &roots {
        if let Ok(mut inbox) = inboxes.get_mut(by.0) {
            inbox.0.push_back(Work::Mount {
                root: root.holder,
                name: shown.name.clone(),
            });
        }
    }
}

/// Tells each plugin of its panels and settings pages whose root is
/// gone, as when the dock stops showing one, and takes away the
/// root's holder.
fn lose_roots(
    roots: Query<(Entity, &UiRoot, &Surface, &ContributedBy)>,
    alive: Query<()>,
    mut inboxes: Query<&mut Inbox>,
    mut commands: Commands,
) {
    for (surface, root, shown, by) in &roots {
        if alive.contains(root.widget) {
            continue;
        }
        commands.entity(root.holder).try_despawn();
        commands.entity(surface).remove::<UiRoot>();
        if let Ok(mut inbox) = inboxes.get_mut(by.0) {
            inbox.0.push_back(Work::Unmount(shown.name.clone()));
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
        inbox
            .0
            .push_back(Work::Node(action.key, action.input.clone()));
    }
}

/// Moves on each value the engine is moving.
fn travel(
    mut holders: Query<(Entity, &mut Props, &mut Travels)>,
    theme: Res<EngineTheme>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (holder, mut props, mut travels) in &mut holders {
        travels.0.retain_mut(|travel| {
            travel.elapsed += time.delta();
            let curve = theme.motion(travel.motion);
            let to = travel.to.resolve(&theme);
            if travel.elapsed >= curve.duration {
                to.write(&mut props);
                return false;
            }
            let progress = travel.elapsed.as_secs_f32()
                / curve.duration.as_secs_f32();
            travel
                .from
                .blend(to, (curve.ease)(progress))
                .write(&mut props);
            true
        });
        if travels.0.is_empty() {
            commands.entity(holder).remove::<Travels>();
        }
    }
}

/// Fills in each node's roles from the theme again when it changes.
fn restyle(
    theme: Res<EngineTheme>,
    mut holders: Query<(&mut Props, Option<&Travels>)>,
) {
    for (mut props, travels) in &mut holders {
        props.restyle(&theme, travels);
    }
}

/// Takes away the holders of the nodes of a plugin that no longer
/// runs.
fn forget_stopped(
    plugins: Query<(Entity, &Holders), Without<Instance>>,
    mut commands: Commands,
) {
    for (plugin, holders) in &plugins {
        commands.forget_nodes(plugin, holders);
    }
}
