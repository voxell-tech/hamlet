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
use bevy::ecs::hierarchy::{ChildOf, Children};
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
use bevy::picking::events::{Click, Pointer, Press, Release};
use bevy::picking::hover::Hovered as PickingHovered;
use bevy::picking::pointer::PointerButton;
use bevy::time::Time;
use bevy::ui::{
    AlignItems, FlexDirection, JustifyContent, Node, UiRect, percent,
    px,
};
use bevy_fynix::patch::{PatchInk, PatchOpacity};
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::tokens::{
    Motion as Movement, MotionTokens, TextTokens, Tone,
};
use bevy_fynix::transition::blend_color;
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, TooltipExt, column,
    label, menu_item, row, submenu,
};
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
use hamlet_engine_shell::message::message_field;
use hamlet_engine_shell::stick::{ReachedTop, StickToEnd};
use hamlet_engine_shell::style::{
    ButtonStyle, action, content_button, divider, dropdown, field,
    icon_row, labelled_switch, list_row, note, raised, scroll_area,
    switch,
};
use hamlet_engine_shell::{
    AnyView, Cx, EngineTheme, Overlay, OverlayShown, Panel,
    PanelSection, SettingsPage, ShellWorldExt, StatusItem, StyleExt,
};

use crate::WasmCommandsExt;
use crate::budget::Spending;
use crate::dialog::{Asker, ChooseFolder};
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
    /// The holder of the node's properties.
    pub holder: Entity,
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
    lines: u32,
    stick: bool,
    tip: String,
    justify: JustifyContent,
    fill: wit::Fill,
    menu: Menu,
    /// The title of the dialog a folder is chosen in, if it opens
    /// one.
    folder: String,
    /// The roles its ink, size and gap are filled in from.
    tone: wit::Tone,
    text_size: wit::Size,
    space: wit::Space,
    /// The properties the plugin set, a bit each by
    /// [`Property::bit`].
    set: u32,
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
            lines: 1,
            stick: false,
            tip: String::new(),
            justify: JustifyContent::Default,
            fill: wit::Fill::None,
            menu: Menu::default(),
            folder: String::new(),
            tone: wit::Tone::Body,
            text_size: wit::Size::Body,
            space: wit::Space::Gap,
            set: 0,
            hover: false,
            press: false,
        }
    }

    fn has(&self, property: u32) -> bool {
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
                    wit::ButtonStyle::Row => ButtonStyle::Row,
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
            Property::Lines(lines) => self.lines = lines,
            Property::Stick(stick) => self.stick = stick,
            Property::Tip(tip) => self.tip = tip,
            Property::Align(align) => {
                self.justify = match align {
                    wit::Align::Start => JustifyContent::Default,
                    wit::Align::End => JustifyContent::FlexEnd,
                };
            }
            Property::Fill(fill) => self.fill = fill,
            Property::Menu(menu) => self.menu = menu.into(),
            Property::Folder(title) => self.folder = title,
            Property::Tone(tone) => self.tone = tone,
            Property::Size(size) => self.text_size = size,
            Property::Space(space) => self.space = space,
            Property::Opacity(_) | Property::Offset(_) => {}
        }
    }
}

/// The bits of [`Props::set`] its widget is built by.
const TONE: u32 = 1 << 1;
const SIZE: u32 = 1 << 2;
const BOLD: u32 = 1 << 3;
const OPACITY: u32 = 1 << 5;
const OFFSET: u32 = 1 << 11;

impl Property {
    /// The bit of the property this sets, in the order the WIT names
    /// them.
    fn bit(&self) -> u32 {
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
            Property::Lines(_) => 14,
            Property::Stick(_) => 15,
            Property::Tip(_) => 16,
            Property::Align(_) => 17,
            Property::Fill(_) => 18,
            Property::Menu(_) => 19,
            Property::Folder(_) => 20,
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
    around: Around,
}

/// What a node's widget is given besides what it is built as, of any
/// kind. A change of it builds the widget again.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Around {
    /// It has words to show under the pointer.
    tip: bool,
    /// The menu it opens, if any.
    menu: Option<Menu>,
}

impl Around {
    fn of(props: &Props) -> Self {
        Self {
            tip: !props.tip.is_empty(),
            menu: (!props.menu.items.is_empty())
                .then(|| props.menu.clone()),
        }
    }
}

/// A node's menu, as its widget builds it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Menu {
    items: Vec<MenuItem>,
    /// It opens on a click too.
    click: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct MenuItem {
    text: String,
    /// The item it is under, or none if empty.
    group: String,
    critical: bool,
}

impl From<wit::Menu> for Menu {
    fn from(menu: wit::Menu) -> Self {
        Self {
            items: menu
                .items
                .into_iter()
                .map(|item| MenuItem {
                    text: item.text,
                    group: item.group,
                    critical: item.critical,
                })
                .collect(),
            click: menu.click,
        }
    }
}

/// What a node's widget is built as, past what it reads from its
/// holder. A change of it builds the widget again.
#[derive(Clone, PartialEq, Debug)]
enum Shape {
    Plain,
    Frame {
        fill: wit::Fill,
    },
    Label {
        /// The bits of [`Props::set`] it binds.
        set: u32,
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
    Toggle {
        /// It has words beside it.
        labelled: bool,
    },
    Field {
        placeholder: String,
        lines: u32,
    },
    Dropdown {
        options: Vec<String>,
    },
    Scroll {
        stick: bool,
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
            Kind::Toggle => Self::Toggle {
                labelled: !props.text.is_empty(),
            },
            Kind::Field => Self::Field {
                placeholder: props.placeholder.clone(),
                lines: props.lines,
            },
            Kind::Dropdown => Self::Dropdown {
                options: props.options.clone(),
            },
            Kind::Scroll => Self::Scroll { stick: props.stick },
            Kind::Frame => Self::Frame { fill: props.fill },
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

/// The content of a WASM plugin's settings page, footer, overlay or
/// section.
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
) -> Each<EngineTheme, Child, (Entity, Shape, Around)> {
    each(
        component::<Kids, _>(holder, |kids| {
            kids.map_or_else(Vec::new, |kids| kids.0.clone())
        }),
        |child| {
            (child.holder, child.shape.clone(), child.around.clone())
        },
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
        (Kind::Frame, &Shape::Frame { fill }) => {
            frame(holder, plugin, fill)
        }
        (Kind::Label, &Shape::Label { set, display }) => {
            inked(text(holder, set, display), holder, set & TONE != 0)
        }
        (Kind::Button, &Shape::Button { style, holds }) => {
            let click = move |world: &mut World| {
                let title = world
                    .get::<Props>(holder)
                    .map(|props| props.folder.clone())
                    .unwrap_or_default();
                if title.is_empty() {
                    world.write_message(NodeAction {
                        plugin,
                        key,
                        input: NodeInput::Click,
                    });
                } else {
                    world.trigger(ChooseFolder {
                        title,
                        asker: Asker::Node { plugin, key },
                    });
                }
            };
            if holds && style == ButtonStyle::Row {
                row_button(holder, plugin, click)
            } else if holds {
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
                beside_words(icon, parent)
            }
        }
        (Kind::Toggle, &Shape::Toggle { labelled }) => {
            let on = read(holder, |props| props.on);
            let change = move |world: &mut World, on| {
                world.write_message(NodeAction {
                    plugin,
                    key,
                    input: NodeInput::Toggle(on),
                });
            };
            if labelled {
                labelled_switch(
                    read(holder, |props| props.text.clone()),
                    on,
                    change,
                )
            } else {
                switch(on, change)
            }
        }
        (Kind::Field, Shape::Field { placeholder, lines }) => {
            let tell = move |input: fn(String) -> NodeInput| {
                move |world: &mut World, text: String| {
                    world.write_message(NodeAction {
                        plugin,
                        key,
                        input: input(text),
                    });
                }
            };
            let text = read(holder, |props| props.text.clone());
            if *lines > 1 {
                message_field(
                    text,
                    placeholder.clone(),
                    *lines,
                    tell(NodeInput::Submit),
                    tell(NodeInput::Commit),
                )
            } else {
                field(
                    text,
                    placeholder.clone(),
                    tell(NodeInput::Commit),
                )
            }
        }
        (Kind::List, _) => kids(holder, plugin, Place::List)
            .within(column(()).gap(0.0))
            .boxed(),
        (Kind::Scroll, &Shape::Scroll { stick }) => {
            let area = AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let rows = kids(holder, plugin, Place::Frame)
                    .within(column(()).gap(gap));
                cx.build(scroll_area((rows,)))
            });
            if stick {
                sticking(area, plugin, key)
            } else {
                area
            }
        }
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
    let built = around(built, holder, plugin, key, &child.around);
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
fn text(holder: Entity, set: u32, display: bool) -> AnyView {
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

/// `icon`, raised as in a button when `parent` lays it in a row
/// beside words in the display face, as a heading's icon is.
fn beside_words(icon: AnyView, parent: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let in_row =
            cx.world.get::<Props>(parent).is_some_and(|props| {
                props.direction == FlexDirection::Row
            });
        let by_display =
            cx.world.get::<Kids>(parent).is_some_and(|kids| {
                kids.0.iter().any(|kid| {
                    matches!(
                        kid.shape,
                        Shape::Label { display: true, .. }
                    )
                })
            });
        if in_row && by_display {
            cx.build(raised(icon))
        } else {
            cx.build(icon)
        }
    })
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

/// The frame of the node of `holder`, a node of `plugin`, drawn on
/// `fill`, its colours from the theme.
/// A button of the style `Row` holding what the node of `holder`
/// holds, set out as a row of a list is: from its start, its last at
/// its end. A press runs `click`.
fn row_button(
    holder: Entity,
    plugin: Entity,
    click: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let section = cx.theme().space.section;
        cx.build(
            kids(holder, plugin, Place::Frame)
                .within(
                    row(())
                        .gap(section)
                        .grow(1.0)
                        .align(AlignItems::Center)
                        .justify(JustifyContent::SpaceBetween),
                )
                .styled(ButtonStyle::Row)
                .on_activate(click),
        )
    })
}

fn frame(holder: Entity, plugin: Entity, fill: wit::Fill) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let frame = column(())
            .direction(read(holder, |props| props.direction))
            .gap(read(holder, |props| props.gap))
            .align(read(holder, |props| props.align))
            .justify(read(holder, |props| props.justify));
        let space = &cx.theme().space;
        let colors = &cx.theme().colors;
        let card = UiRect::axes(
            px(space.card_padding_x),
            px(space.card_padding_y),
        );
        let drawn = match fill {
            wit::Fill::None => None,
            wit::Fill::Card => Some((
                *colors.surface,
                *colors.border,
                space.radius,
                card,
            )),
            wit::Fill::Accent => Some((
                *colors.active_low,
                *colors.active_high,
                space.radius,
                card,
            )),
            // As round as anything it holds is tall.
            wit::Fill::Disc => Some((
                *colors.surface_raised,
                *colors.border,
                space.row,
                UiRect::all(px(space.gap / 2.0)),
            )),
        };
        let frame = match drawn {
            None => frame,
            Some((ground, line, radius, padding)) => frame
                .fill(ground)
                .border(space.border)
                .border_color(line)
                .radius(radius)
                .padding(padding)
                .min_width(px(0.0))
                .max_width(percent(100.0)),
        };
        cx.build(kids(holder, plugin, Place::Frame).within(frame))
    })
}

/// `view` with what is around it: the tip of the node of `holder`
/// under the pointer.
fn around(
    view: AnyView,
    holder: Entity,
    plugin: Entity,
    key: u32,
    around: &Around,
) -> AnyView {
    let view = match &around.menu {
        Some(menu) => menued(view, menu.clone(), plugin, key),
        None => view,
    };
    if !around.tip {
        return view;
    }
    view.tooltip(move || {
        note(read(holder, |props| props.tip.clone()))
    })
    .boxed()
}

/// `view` opening `menu`, the menu of the node `key` of `plugin`, at
/// the pointer on a right-click, and on a click when it says so.
fn menued(
    view: AnyView,
    menu: Menu,
    plugin: Entity,
    key: u32,
) -> AnyView {
    let click = menu.click;
    let items = menu.items;
    let opened =
        view.context_menu(move || menu_rows(&items, plugin, key));
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(opened);
        if click {
            cx.world.entity_mut(node).observe(open_on_click);
        }
        node
    })
}

/// The rows of a menu of `items`: each item, or a row opening those
/// of a group beside it, in the order they first come.
fn menu_rows(
    items: &[MenuItem],
    plugin: Entity,
    key: u32,
) -> Vec<AnyView> {
    let row = move |at: usize, item: &MenuItem| {
        let tone = if item.critical {
            Tone::Critical
        } else {
            Tone::Body
        };
        let at = u32::try_from(at).unwrap_or(u32::MAX);
        menu_item(label(item.text.clone()).tone(tone))
            .on_activate(move |world| {
                world.write_message(NodeAction {
                    plugin,
                    key,
                    input: NodeInput::Chose(at),
                });
            })
            .boxed()
    };
    let mut rows = Vec::new();
    let mut groups = Vec::<&str>::new();
    for (at, item) in items.iter().enumerate() {
        if item.group.is_empty() {
            rows.push(row(at, item));
            continue;
        }
        if groups.contains(&item.group.as_str()) {
            continue;
        }
        groups.push(&item.group);
        let under = items
            .iter()
            .enumerate()
            .filter(|(_, one)| one.group == item.group)
            .map(|(at, one)| (at, one.clone()))
            .collect::<Vec<_>>();
        rows.push(
            submenu(label(item.group.clone()), move || {
                under
                    .iter()
                    .map(|(at, one)| row(*at, one))
                    .collect::<Vec<_>>()
            })
            .boxed(),
        );
    }
    rows
}

/// Opens the menu of the node clicked as a right-click would.
fn open_on_click(click: On<Pointer<Click>>, mut commands: Commands) {
    if click.button != PointerButton::Primary {
        return;
    }
    commands.trigger(Pointer::new_without_propagate(
        click.pointer_id,
        click.pointer_location.clone(),
        Press {
            button: PointerButton::Secondary,
            hit: click.hit.clone(),
            count: 1,
        },
        click.event_target(),
    ));
}

/// `area`, a scroll area, keeping to its end, and telling the node
/// `key` of `plugin` when it reaches its top.
fn sticking(area: AnyView, plugin: Entity, key: u32) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(area);
        // The scroll itself, under the column that cuts it.
        let scroll = cx
            .world
            .get::<Children>(node)
            .and_then(|children| {
                children.iter().find(|&child| {
                    cx.world.get::<ScrollGoal>(child).is_some()
                })
            })
            .unwrap_or(node);
        cx.world.entity_mut(scroll).insert(StickToEnd::default()).observe(
            move |_: On<ReachedTop>,
                  mut actions: MessageWriter<NodeAction>| {
                actions.write(NodeAction {
                    plugin,
                    key,
                    input: NodeInput::Top,
                });
            },
        );
        node
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
            holder,
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
                        around: Around::default(),
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
        let (shape, around) =
            (Shape::of(kind, props, holds), Around::of(props));
        let Some(mut kids) = self.get_mut::<Kids>(parent) else {
            return;
        };
        if let Some(kid) =
            kids.0.iter_mut().find(|kid| kid.holder == holder)
            && (kid.shape != shape || kid.around != around)
        {
            kid.shape = shape;
            kid.around = around;
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
                        grow: shown.grow,
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
                SurfaceChange::Section { name, holds } => {
                    let Some(at) = self.added_named::<PanelSection>(
                        plugin,
                        &name,
                        |section| &section.id,
                    ) else {
                        continue;
                    };
                    if let Some(mut section) =
                        self.get_mut::<PanelSection>(at)
                        && section.holds != holds
                    {
                        section.holds = holds;
                    }
                }
                SurfaceChange::Open(name) => {
                    // A full id names a panel of any plugin, which
                    // changes only the dock.
                    if let Ok(id) = PanelId::try_from(name.as_str()) {
                        let there = self
                            .try_query::<&Panel>()
                            .is_some_and(|mut panels| {
                                panels
                                    .iter(self)
                                    .any(|panel| panel.id == id)
                            });
                        if there {
                            self.open_panel(&id);
                        }
                    } else if let Some(page) = self
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
    mut inboxes: Query<(&mut Inbox, &mut Spending)>,
    budgets: Res<Budgets>,
) {
    for (root, shown, by) in &roots {
        if let Ok((mut inbox, mut spending)) = inboxes.get_mut(by.0) {
            inbox.offer(
                Work::Mount {
                    root: root.holder,
                    name: shown.name.clone(),
                },
                &mut spending,
                &budgets,
            );
        }
    }
}

/// Tells each plugin of its panels and settings pages whose root is
/// gone, as when the dock stops showing one, and takes away the
/// root's holder.
fn lose_roots(
    roots: Query<(Entity, &UiRoot, &Surface, &ContributedBy)>,
    alive: Query<()>,
    mut inboxes: Query<(&mut Inbox, &mut Spending)>,
    budgets: Res<Budgets>,
    mut commands: Commands,
) {
    for (surface, root, shown, by) in &roots {
        if alive.contains(root.widget) {
            continue;
        }
        commands.entity(root.holder).try_despawn();
        commands.entity(surface).remove::<UiRoot>();
        if let Ok((mut inbox, mut spending)) = inboxes.get_mut(by.0) {
            inbox.offer(
                Work::Unmount(shown.name.clone()),
                &mut spending,
                &budgets,
            );
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
        if let Ok((mut inbox, mut spending)) =
            inboxes.get_mut(action.plugin)
        {
            inbox.offer(
                Work::Node(action.key, action.input.clone()),
                &mut spending,
                &budgets,
            );
        }
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
