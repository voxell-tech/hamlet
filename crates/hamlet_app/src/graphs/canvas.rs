//! The canvas: each node a card, each wire a curve of short segments,
//! all on a sheet that pans and zooms. A card is dragged to move it,
//! an output's dot onto an input's to wire them, and an input's dot
//! is clicked to cut its wire. Values sit on the cards as chips: a
//! number is dragged sideways to change it, and a switch or a choice
//! is clicked.

use bevy::app::{App, Update};
use bevy::asset::Assets;
use bevy::color::{Alpha, Color};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Or, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, If, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseScrollUnit;
use bevy::math::Vec2;
use bevy::picking::Pickable;
use bevy::picking::events::{
    PointerClick, PointerDrag, PointerDragDrop, PointerDragEnd,
    PointerDragStart, PointerPress, PointerScroll,
};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::text::{TextColor, TextFont};
use bevy::ui::widget::Text;
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius,
    ComputedNode, FlexDirection, JustifyContent, Node, Overflow,
    PositionType, UiGlobalTransform, UiRect, UiTransform, Val, Val2,
    ZIndex, px,
};
use bevy::ui_render::ui_material::MaterialNode;
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{BehaviorExt, frame};
use bevy_fynix::{EntityCursor, OverrideCursor, Theme};
use hamlet_blueprint::{
    Constant, Does, Edit, Initial, NodeId, Port, PortSpec, Roll,
    Setting, Spread, ValueType, Wire, ports,
};

use super::Editor;
use super::menu::Menu;
use super::wire::{Shape, WireMaterial};
use crate::looks::Fills;
use crate::theme::{HamletTheme, Palette};
use crate::{View, with_cx};

const WIDTH: f32 = 196.0;
const HEADER: f32 = 26.0;
const ROW: f32 = 22.0;
const PAD: f32 = 4.0;
const BORDER: f32 = 1.0;
const DOT: f32 = 10.0;
const LEAST_ZOOM: f32 = 0.3;
const MOST_ZOOM: f32 = 2.0;
/// How far one pixel of dragging moves a number.
const STEP: f32 = 0.05;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Sight>()
        .init_resource::<Gesture>()
        .add_systems(Update, (build, place, relabel, paint).chain());
}

/// Marks the frame the canvas is drawn in.
#[derive(Component)]
pub(super) struct Canvas;

/// The node every card and wire hangs off, moved and scaled to pan
/// and zoom them as one.
#[derive(Component)]
struct Sheet;

/// A node's card.
#[derive(Component)]
struct Card(NodeId);

/// The node a wire is drawn on: the wire's, or none for one being
/// dragged out of a port.
#[derive(Component)]
struct Drawn(Option<Wire>);

/// A port's dot on a card.
#[derive(Component, Clone)]
struct Dot {
    port: Port,
    output: bool,
    value_type: ValueType,
}

/// A value shown on a card that can be changed there.
#[derive(Component, Clone)]
struct Chip {
    node: NodeId,
    spec: Input,
    part: Part,
}

/// What a chip knows of the input it shows: a kind's port, or an
/// input of the graph a node runs.
#[derive(Clone, Debug)]
struct Input {
    name: String,
    value_type: ValueType,
    initial: Option<Constant>,
    /// The choices a choice takes. Empty for any text.
    choices: &'static [&'static str],
}

impl Input {
    fn of_port(spec: &PortSpec) -> Self {
        Self {
            name: spec.name.to_owned(),
            value_type: spec.value_type,
            initial: spec.initial.map(|initial| match initial {
                Initial::Number(n) => Constant::Number(n),
                Initial::Bool(on) => Constant::Bool(on),
                Initial::Point(p) => Constant::Point(p),
                Initial::Choice(c) => Constant::Choice(c.to_owned()),
            }),
            choices: spec.choices,
        }
    }
}

/// The part of an input's setting a [`Chip`] shows.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Part {
    Number,
    Axis(usize),
    Low,
    High,
    Chance,
    Flag,
    Choice,
}

impl Part {
    fn is_number(self) -> bool {
        !matches!(self, Part::Flag | Part::Choice)
    }
}

/// How far the sheet is panned, in canvas pixels, and how much it is
/// zoomed.
#[derive(Resource)]
struct Sight {
    pan: Vec2,
    zoom: f32,
}

impl Default for Sight {
    fn default() -> Self {
        Self {
            pan: Vec2::splat(16.0),
            zoom: 1.0,
        }
    }
}

/// What a drag on the canvas is doing.
#[derive(Resource, Default, Clone)]
enum Gesture {
    #[default]
    None,
    Pan,
    /// A card, and where it was when the drag began.
    Card(NodeId, Vec2),
    /// A wire out of an output's port, from where on the sheet, and
    /// how far the pointer has gone.
    Link(Port, ValueType, Vec2, Vec2),
    /// A number dragged sideways, from the value it had.
    Scrub(Chip, f32),
}

pub(super) fn view() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        frame()
            .fills_width()
            .fills_height()
            .overflow(Overflow::clip())
            .fill(theme.palette.well)
            .radius(theme.space.round)
            .tagged(Canvas)
    })
}

/// The middle of what is in sight, in the sheet's layout, for a new
/// card.
pub(super) fn middle(world: &mut World) -> Vec2 {
    let size = canvas_size(world);
    let sight = world.resource::<Sight>();
    (size * 0.5 - sight.pan) / sight.zoom
        - Vec2::new(WIDTH * 0.5, HEADER)
}

/// Pans and zooms so that every card is in sight.
pub(super) fn fit(world: &mut World) {
    let size = canvas_size(world);
    let editor = world.resource::<Editor>();
    let (mut low, mut high) =
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for node in &editor.sheet.graph.nodes {
        let at = card_at(editor, node.id);
        let (inputs, outputs) = rows(editor, node);
        low = low.min(at);
        high = high.max(
            at + Vec2::new(
                WIDTH,
                height(inputs.len() + outputs.len()),
            ),
        );
    }
    if low.x > high.x {
        return;
    }
    let margin = 40.0;
    let span = (high - low).max(Vec2::ONE);
    let zoom = ((size - margin * 2.0) / span)
        .min_element()
        .clamp(LEAST_ZOOM, 1.0);
    let pan = size * 0.5 - (low + high) * 0.5 * zoom;
    *world.resource_mut::<Sight>() = Sight { pan, zoom };
}

/// Where the window's point `at` is on the sheet, if it is over the
/// canvas.
pub(super) fn on_sheet(world: &mut World, at: Vec2) -> Option<Vec2> {
    let corner = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Canvas>>()
        .iter(world)
        .next()
        .map(|(node, transform)| corner(node, transform))?;
    let size = canvas_size(world);
    let inside = at - corner;
    if inside.cmplt(Vec2::ZERO).any() || inside.cmpgt(size).any() {
        return None;
    }
    let sight = world.resource::<Sight>();
    Some((inside - sight.pan) / sight.zoom)
}

/// Whether the pointer is on the canvas.
pub(super) fn is_hovered(world: &mut World) -> bool {
    world
        .query_filtered::<&Hovered, With<Canvas>>()
        .iter(world)
        .any(Hovered::get)
}

/// A node's top left corner in the window, in logical pixels.
pub(super) fn corner(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Vec2 {
    (transform.translation - node.size() * 0.5)
        * node.inverse_scale_factor()
}

fn canvas_size(world: &mut World) -> Vec2 {
    world
        .query_filtered::<&ComputedNode, With<Canvas>>()
        .iter(world)
        .next()
        .map_or(Vec2::splat(400.0), |node| {
            node.size() * node.inverse_scale_factor()
        })
}

/// The height of a card with `rows` rows.
fn height(rows: usize) -> f32 {
    BORDER * 2.0 + HEADER + PAD * 2.0 + rows as f32 * ROW
}

/// The inputs and outputs of each node, in the order drawn.
fn rows(
    editor: &Editor,
    node: &hamlet_blueprint::Node,
) -> (Vec<Dot>, Vec<Dot>) {
    let Ok((inputs, outputs)) =
        ports(&editor.registry, &editor.rules, node)
    else {
        return (Vec::new(), Vec::new());
    };
    let dot = |output: bool| {
        move |info: hamlet_blueprint::PortInfo| Dot {
            port: Port {
                node: node.id,
                name: info.name,
            },
            output,
            value_type: info.value_type,
        }
    };
    (
        inputs.into_iter().map(dot(false)).collect(),
        outputs.into_iter().map(dot(true)).collect(),
    )
}

/// What a chip needs of each input of `node`: its kind's ports, or
/// the inputs of the graph it runs.
fn specs(editor: &Editor, node: NodeId) -> Vec<Input> {
    match editor.sheet.graph.node(node).map(|node| &node.does) {
        Some(Does::Kind(kind)) => editor
            .registry
            .get(kind)
            .map(|kind| {
                kind.inputs.iter().map(Input::of_port).collect()
            })
            .unwrap_or_default(),
        Some(Does::Graph(graph)) => editor
            .rules
            .graphs
            .get(graph)
            .map(|graph| {
                graph
                    .inputs
                    .iter()
                    .map(|input| Input {
                        name: input.name.clone(),
                        value_type: input.value_type,
                        initial: input.default.clone(),
                        choices: &[],
                    })
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Where a port's dot is on the sheet, from its card's place and row.
fn dot_at(card: Vec2, row: usize, output: bool) -> Vec2 {
    Vec2::new(
        card.x + if output { WIDTH } else { 0.0 },
        card.y + BORDER + HEADER + PAD + row as f32 * ROW + ROW * 0.5,
    )
}

fn card_at(editor: &Editor, node: NodeId) -> Vec2 {
    editor.sheet.layout.get(&node).copied().unwrap_or_default()
}

/// The colour of a wire or a dot of `value_type`.
fn tint(palette: &Palette, value_type: ValueType) -> Color {
    match value_type {
        ValueType::Number => palette.brass_3,
        ValueType::Bool => palette.moss,
        ValueType::Point | ValueType::Curve => palette.rune,
        ValueType::Choice => palette.grey_1,
        ValueType::Placements => palette.lamp,
        ValueType::Field => palette.ember,
        ValueType::Geometry => palette.flame,
    }
}

fn number_text(n: f32) -> String {
    let text = format!("{n:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// The chips that show how an unwired input is set, with any words
/// between them.
fn chips(
    spec: &Input,
    setting: Option<&Setting>,
) -> Vec<Result<Part, String>> {
    let written = match spec.value_type {
        ValueType::Number | ValueType::Field => {
            Some(ValueType::Number)
        }
        other => Some(other),
    };
    match (setting, written) {
        (Some(Setting::Random(roll)), _) => match roll.spread {
            Spread::Range(..) => vec![
                Ok(Part::Low),
                Err("to".to_owned()),
                Ok(Part::High),
            ],
            Spread::Chance(_) => vec![Ok(Part::Chance)],
            Spread::Weights(_) => vec![Err("random".to_owned())],
        },
        (Some(Setting::FromParent(name)), _) => {
            vec![Err(format!("from {name}"))]
        }
        (_, Some(ValueType::Number)) => vec![Ok(Part::Number)],
        (_, Some(ValueType::Bool)) => vec![Ok(Part::Flag)],
        (_, Some(ValueType::Point)) => {
            (0..3).map(|axis| Ok(Part::Axis(axis))).collect()
        }
        (_, Some(ValueType::Choice)) if !spec.choices.is_empty() => {
            vec![Ok(Part::Choice)]
        }
        (Some(Setting::Fixed(Constant::Choice(text))), _) => {
            vec![Err(text.clone())]
        }
        (None, Some(ValueType::Choice)) => match &spec.initial {
            Some(Constant::Choice(text)) => vec![Err(text.clone())],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// The value a chip shows: its setting's, or the kind's initial
/// value.
fn shown(
    setting: Option<&Setting>,
    spec: &Input,
    part: Part,
) -> String {
    let initial =
        || spec.initial.clone().map(hamlet_blueprint::Value::from);
    match (part, setting) {
        (
            Part::Low,
            Some(Setting::Random(Roll {
                spread: Spread::Range(low, _),
                ..
            })),
        ) => number_text(*low),
        (
            Part::High,
            Some(Setting::Random(Roll {
                spread: Spread::Range(_, high),
                ..
            })),
        ) => number_text(*high),
        (
            Part::Chance,
            Some(Setting::Random(Roll {
                spread: Spread::Chance(chance),
                ..
            })),
        ) => {
            format!("{}%", number_text(chance * 100.0))
        }
        (Part::Number | Part::Axis(_), _) => {
            number(part, setting, spec)
                .map(number_text)
                .unwrap_or_default()
        }
        (Part::Flag, Some(Setting::Fixed(Constant::Bool(on)))) => {
            on_off(*on)
        }
        (Part::Flag, _) => match initial() {
            Some(hamlet_blueprint::Value::Bool(on)) => on_off(on),
            _ => on_off(true),
        },
        (
            Part::Choice,
            Some(Setting::Fixed(Constant::Choice(choice))),
        ) => choice.replace('_', " "),
        (Part::Choice, _) => match initial() {
            Some(hamlet_blueprint::Value::Choice(choice)) => {
                choice.replace('_', " ")
            }
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_owned()
}

/// The number a chip's part holds now.
fn number(
    part: Part,
    setting: Option<&Setting>,
    spec: &Input,
) -> Option<f32> {
    match (part, setting) {
        (Part::Number, Some(Setting::Fixed(Constant::Number(n)))) => {
            Some(*n)
        }
        (Part::Number, None) => match spec.initial {
            Some(Constant::Number(n)) => Some(n),
            _ => Some(0.0),
        },
        (
            Part::Axis(axis),
            Some(Setting::Fixed(Constant::Point(p))),
        ) => Some(p[axis]),
        (Part::Axis(axis), None) => match spec.initial {
            Some(Constant::Point(p)) => Some(p[axis]),
            _ => Some(0.0),
        },
        (
            Part::Low,
            Some(Setting::Random(Roll {
                spread: Spread::Range(low, _),
                ..
            })),
        ) => Some(*low),
        (
            Part::High,
            Some(Setting::Random(Roll {
                spread: Spread::Range(_, high),
                ..
            })),
        ) => Some(*high),
        (
            Part::Chance,
            Some(Setting::Random(Roll {
                spread: Spread::Chance(c),
                ..
            })),
        ) => Some(*c),
        _ => None,
    }
}

/// The setting with a chip's part set to `value`.
fn with_number(
    part: Part,
    setting: Option<&Setting>,
    spec: &Input,
    value: f32,
) -> Option<Setting> {
    Some(match (part, setting.cloned()) {
        (Part::Number, _) => Setting::Fixed(Constant::Number(value)),
        (Part::Axis(axis), current) => {
            let mut point = match (current, spec.initial.clone()) {
                (Some(Setting::Fixed(Constant::Point(p))), _)
                | (None, Some(Constant::Point(p))) => p,
                _ => bevy::math::Vec3::ZERO,
            };
            point[axis] = value;
            Setting::Fixed(Constant::Point(point))
        }
        (
            Part::Low | Part::High | Part::Chance,
            Some(Setting::Random(mut roll)),
        ) => {
            match (&mut roll.spread, part) {
                (Spread::Range(low, _), Part::Low) => *low = value,
                (Spread::Range(_, high), Part::High) => *high = value,
                (Spread::Chance(chance), Part::Chance) => {
                    *chance = value.clamp(0.0, 1.0);
                }
                _ => return None,
            }
            Setting::Random(roll)
        }
        _ => return None,
    })
}

/// Builds every card again when the nodes, the wires or how an input
/// is set change, and when the canvas is new. Not while a number is
/// being dragged, whose chip would go.
fn build(
    mut commands: Commands,
    editor: Res<Editor>,
    gesture: Res<Gesture>,
    // The theme loads after the first frames.
    If(theme): If<Res<Theme<HamletTheme>>>,
    canvas: Query<Entity, With<Canvas>>,
    cards: Query<Entity, With<Card>>,
    mut built: Local<Option<(Entity, Entity, u64)>>,
) {
    let Some(canvas) = canvas.iter().next() else {
        return;
    };
    if matches!(*gesture, Gesture::Scrub(..)) {
        return;
    }
    let sheet = match *built {
        Some((was, sheet, shape)) if was == canvas => {
            if shape == editor.shape {
                return;
            }
            sheet
        }
        _ => {
            commands
                .entity(canvas)
                .insert(Hovered::default())
                .observe(ask)
                .observe(pan_start)
                .observe(pan)
                .observe(gesture_end)
                .observe(zoom)
                .observe(deselect);
            commands
                .spawn((
                    Sheet,
                    // A point at the canvas's corner, so it scales
                    // about the corner.
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0.0),
                        top: px(0.0),
                        width: px(0.0),
                        height: px(0.0),
                        ..Node::default()
                    },
                    UiTransform::default(),
                    Pickable::IGNORE,
                    ChildOf(canvas),
                ))
                .id()
        }
    };
    *built = Some((canvas, sheet, editor.shape));
    for card in &cards {
        commands.entity(card).despawn();
    }
    let palette = &theme.0.palette;
    for node in &editor.sheet.graph.nodes {
        let (inputs, outputs) = rows(&editor, node);
        // A graph run as a node is marked by its header's colour.
        let (title, header_fill, title_colour) = match &node.does {
            Does::Kind(kind) => (
                kind.replace('_', " "),
                palette.plate,
                palette.brass_1,
            ),
            Does::Graph(graph) => {
                (graph.clone(), palette.lamp_low, palette.flame)
            }
        };
        let card = commands
            .spawn((
                Card(node.id),
                Node {
                    position_type: PositionType::Absolute,
                    width: px(WIDTH),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(px(BORDER)),
                    padding: UiRect::bottom(px(PAD)),
                    border_radius: BorderRadius::all(px(6.0)),
                    ..Node::default()
                },
                BackgroundColor(palette.card),
                BorderColor::all(palette.seam),
                Hovered::default(),
                EntityCursor(SystemCursorIcon::Grab),
                ChildOf(sheet),
            ))
            .observe(select)
            .observe(open_on_double)
            .observe(grab)
            .observe(drag_card)
            .observe(gesture_end)
            .id();
        let header = commands
            .spawn((
                Node {
                    height: px(HEADER),
                    padding: UiRect::horizontal(px(8.0)),
                    margin: UiRect::bottom(px(PAD)),
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::top(px(6.0)),
                    ..Node::default()
                },
                BackgroundColor(header_fill),
                ChildOf(card),
            ))
            .id();
        commands.spawn((
            Text::new(title),
            TextFont::from_font_size(13.0),
            TextColor(title_colour),
            Pickable::IGNORE,
            ChildOf(header),
        ));
        let specs = specs(&editor, node.id);
        for dot in inputs.into_iter().chain(outputs) {
            let wired = editor
                .sheet
                .graph
                .wires
                .iter()
                .any(|wire| wire.to == dot.port);
            let row = commands
                .spawn((
                    Node {
                        height: px(ROW),
                        padding: UiRect::horizontal(px(10.0)),
                        align_items: AlignItems::Center,
                        column_gap: px(3.0),
                        justify_content: if dot.output {
                            JustifyContent::FlexEnd
                        } else {
                            JustifyContent::FlexStart
                        },
                        ..Node::default()
                    },
                    ChildOf(card),
                ))
                .id();
            commands.spawn((
                Text::new(dot.port.name.replace('_', " ")),
                TextFont::from_font_size(11.0),
                TextColor(palette.grey_1),
                Node {
                    flex_grow: if dot.output { 0.0 } else { 1.0 },
                    ..Node::default()
                },
                Pickable::IGNORE,
                ChildOf(row),
            ));
            let spec =
                specs.iter().find(|spec| spec.name == dot.port.name);
            if let (false, false, Some(spec)) =
                (dot.output, wired, spec)
            {
                let setting = node.settings.get(&spec.name);
                for piece in chips(spec, setting) {
                    match piece {
                        Ok(part) => {
                            let chip = Chip {
                                node: node.id,
                                spec: spec.clone(),
                                part,
                            };
                            let mut spawned = commands.spawn((
                                Text::new(shown(setting, spec, part)),
                                TextFont::from_font_size(11.0),
                                TextColor(palette.brass_2),
                                Node {
                                    padding: UiRect::axes(
                                        px(5.0),
                                        px(1.0),
                                    ),
                                    border_radius: BorderRadius::all(
                                        px(3.0),
                                    ),
                                    ..Node::default()
                                },
                                BackgroundColor(palette.well),
                                Hovered::default(),
                                EntityCursor(if part.is_number() {
                                    SystemCursorIcon::EwResize
                                } else {
                                    SystemCursorIcon::Pointer
                                }),
                                ChildOf(row),
                            ));
                            if part.is_number() {
                                spawned
                                    .observe(scrub_start)
                                    .observe(scrub)
                                    .observe(gesture_end);
                            } else {
                                spawned.observe(flip);
                            }
                            spawned.insert(chip);
                        }
                        Err(words) => {
                            commands.spawn((
                                Text::new(words.replace('_', " ")),
                                TextFont::from_font_size(11.0),
                                TextColor(palette.brass_5),
                                Pickable::IGNORE,
                                ChildOf(row),
                            ));
                        }
                    }
                }
            }
            // The dot sits on the card's edge, half outside it.
            let edge = px(-DOT * 0.5 - BORDER);
            let mut spot = commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: px((ROW - DOT) * 0.5),
                    left: if dot.output { Val::Auto } else { edge },
                    right: if dot.output { edge } else { Val::Auto },
                    width: px(DOT),
                    height: px(DOT),
                    border: UiRect::all(px(1.5)),
                    border_radius: BorderRadius::MAX,
                    ..Node::default()
                },
                BackgroundColor(tint(palette, dot.value_type)),
                BorderColor::all(palette.well),
                Hovered::default(),
                EntityCursor(SystemCursorIcon::Crosshair),
                ChildOf(row),
            ));
            if dot.output {
                spot.observe(start_link)
                    .observe(drag_link)
                    .observe(gesture_end);
            } else {
                spot.observe(link).observe(cut);
            }
            spot.insert(dot);
        }
    }
}

/// Moves the sheet and the cards, and draws each wire where it now
/// runs: a wire's node is kept, and only moved or given a new shape.
fn place(
    mut commands: Commands,
    editor: Res<Editor>,
    sight: Res<Sight>,
    gesture: Res<Gesture>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    mut sheets: Query<(Entity, &mut UiTransform), With<Sheet>>,
    mut cards: Query<(&Card, &mut Node), Without<Drawn>>,
    mut drawn: Query<
        (Entity, &Drawn, &MaterialNode<WireMaterial>, &mut Node),
        Without<Card>,
    >,
    mut materials: ResMut<Assets<WireMaterial>>,
    added: Query<(), bevy::ecs::query::Added<Card>>,
) {
    let Some((sheet, mut transform)) = sheets.iter_mut().next()
    else {
        return;
    };
    if !editor.is_changed()
        && !sight.is_changed()
        && !gesture.is_changed()
        && added.is_empty()
    {
        return;
    }
    *transform = UiTransform {
        translation: Val2::px(sight.pan.x, sight.pan.y),
        scale: Vec2::splat(sight.zoom),
        ..UiTransform::default()
    };
    for (card, mut node) in &mut cards {
        let at = card_at(&editor, card.0);
        node.left = px(at.x);
        node.top = px(at.y);
    }
    let palette = &theme.0.palette;
    let graph = &editor.sheet.graph;
    let find = |port: &Port, output: bool| {
        let node = graph.node(port.node)?;
        let (inputs, outputs) = rows(&editor, node);
        let (list, before) = if output {
            (&outputs, inputs.len())
        } else {
            (&inputs, 0)
        };
        let row = list.iter().position(|dot| dot.port == *port)?;
        Some((
            dot_at(card_at(&editor, port.node), before + row, output),
            list[row].value_type,
        ))
    };
    let mut wanted = Vec::new();
    for wire in &graph.wires {
        if let (Some((a, value_type)), Some((b, _))) =
            (find(&wire.from, true), find(&wire.to, false))
        {
            let shape = Shape::of(a, b, tint(palette, value_type));
            wanted.push((Some(wire.clone()), shape));
        }
    }
    if let Gesture::Link(_, value_type, start, gone) = &*gesture {
        let shape = Shape::of(
            *start,
            *start + *gone,
            tint(palette, *value_type),
        );
        wanted.push((None, shape));
    }
    for (entity, Drawn(key), material, mut node) in &mut drawn {
        let Some(at) =
            wanted.iter().position(|(other, _)| other == key)
        else {
            commands.entity(entity).despawn();
            continue;
        };
        let (_, shape) = wanted.swap_remove(at);
        set_place(&mut node, &shape);
        if materials
            .get(&material.0)
            .is_some_and(|now| !now.same_as(&shape.material))
            && let Some(mut now) = materials.get_mut(&material.0)
        {
            *now = shape.material;
        }
    }
    for (key, shape) in wanted {
        let mut node = Node {
            position_type: PositionType::Absolute,
            ..Node::default()
        };
        set_place(&mut node, &shape);
        commands.spawn((
            Drawn(key),
            node,
            MaterialNode(materials.add(shape.material)),
            Pickable::IGNORE,
            ZIndex(-1),
            ChildOf(sheet),
        ));
    }
}

/// Shows the value each chip holds now.
fn relabel(
    editor: Res<Editor>,
    mut chips: Query<(&Chip, &mut Text)>,
) {
    if !editor.is_changed() {
        return;
    }
    for (chip, mut text) in &mut chips {
        let setting = editor
            .sheet
            .graph
            .node(chip.node)
            .and_then(|node| node.settings.get(&chip.spec.name));
        let now = shown(setting, &chip.spec, chip.part);
        if text.0 != now {
            text.0 = now;
        }
    }
}

/// Lights what the pointer is on, the selected card, and while a wire
/// is dragged, the inputs it can go to.
fn paint(
    editor: Res<Editor>,
    gesture: Res<Gesture>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    mut cards: Query<
        (&Card, &Hovered, &mut BorderColor),
        Without<Dot>,
    >,
    mut dots: Query<
        (&Dot, &Hovered, &mut BorderColor, &mut BackgroundColor),
        Without<Card>,
    >,
    mut chips: Query<
        (&Hovered, &mut BackgroundColor, &mut TextColor),
        (With<Chip>, Without<Dot>),
    >,
    changed: Query<
        (),
        Or<(Changed<Hovered>, bevy::ecs::query::Added<Hovered>)>,
    >,
) {
    if !editor.is_changed()
        && !gesture.is_changed()
        && changed.is_empty()
    {
        return;
    }
    let palette = &theme.0.palette;
    for (card, hovered, mut border) in &mut cards {
        let colour = if editor.selected == Some(card.0) {
            palette.lamp
        } else if hovered.get() {
            palette.brass_5
        } else {
            palette.seam
        };
        *border = BorderColor::all(colour);
    }
    let linking = match &*gesture {
        Gesture::Link(from, value_type, ..) => {
            Some((from, *value_type))
        }
        _ => None,
    };
    for (dot, hovered, mut border, mut fill) in &mut dots {
        let open = match linking {
            Some((from, value_type)) => {
                (!dot.output && dot.value_type.accepts(value_type))
                    || dot.port == *from
            }
            None => true,
        };
        let colour = tint(palette, dot.value_type);
        fill.0 = if open { colour } else { colour.with_alpha(0.2) };
        *border = BorderColor::all(if hovered.get() && open {
            palette.flame
        } else {
            palette.well
        });
    }
    for (hovered, mut fill, mut text) in &mut chips {
        fill.0 = if hovered.get() {
            palette.lamp_low
        } else {
            palette.well
        };
        text.0 = if hovered.get() {
            palette.flame
        } else {
            palette.brass_2
        };
    }
}

/// Places a wire's node over the curve it draws.
fn set_place(node: &mut Node, shape: &Shape) {
    node.left = px(shape.corner.x);
    node.top = px(shape.corner.y);
    node.width = px(shape.size.x);
    node.height = px(shape.size.y);
}

/// Ends any drag: the cursor goes back, and a moved card's move is
/// kept for undo.
fn gesture_end(
    mut end: On<PointerDragEnd>,
    mut gesture: ResMut<Gesture>,
    mut editor: ResMut<Editor>,
    mut cursor: ResMut<OverrideCursor>,
) {
    end.propagate(false);
    if let Gesture::Card(node, from) = &*gesture
        && editor.sheet.layout.get(node) != Some(from)
    {
        editor.dropped(*node, *from);
    }
    *gesture = Gesture::None;
    cursor.0 = None;
}

fn select(
    mut click: On<PointerClick>,
    cards: Query<&Card>,
    mut editor: ResMut<Editor>,
) {
    click.propagate(false);
    if let Ok(card) = cards.get(click.entity)
        && editor.selected != Some(card.0)
    {
        editor.selected = Some(card.0);
    }
}

/// Opens the graph a card runs on a double press.
fn open_on_double(
    press: On<PointerPress>,
    cards: Query<&Card>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary || press.count != 2 {
        return;
    }
    if let Ok(card) = cards.get(press.entity) {
        let node = card.0;
        commands.queue(move |world: &mut World| {
            super::menu::open_graph(world, node);
        });
    }
}

fn deselect(click: On<PointerClick>, mut editor: ResMut<Editor>) {
    if click.entity == click.original_event_target()
        && editor.selected.is_some()
    {
        editor.selected = None;
    }
}

fn grab(
    mut start: On<PointerDragStart>,
    cards: Query<&Card>,
    mut gesture: ResMut<Gesture>,
    mut editor: ResMut<Editor>,
    mut cursor: ResMut<OverrideCursor>,
) {
    start.propagate(false);
    if start.button != PointerButton::Primary {
        return;
    }
    if let Ok(card) = cards.get(start.entity) {
        *gesture = Gesture::Card(card.0, card_at(&editor, card.0));
        editor.selected = Some(card.0);
        cursor.0 = Some(SystemCursorIcon::Grabbing);
    }
}

fn drag_card(
    mut drag: On<PointerDrag>,
    gesture: Res<Gesture>,
    sight: Res<Sight>,
    mut editor: ResMut<Editor>,
) {
    drag.propagate(false);
    if let Gesture::Card(node, from) = &*gesture {
        editor.drag(*node, *from + drag.distance / sight.zoom);
    }
}

fn pan_start(
    start: On<PointerDragStart>,
    mut gesture: ResMut<Gesture>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if start.entity == start.original_event_target() {
        *gesture = Gesture::Pan;
        cursor.0 = Some(SystemCursorIcon::Move);
    }
}

fn pan(
    drag: On<PointerDrag>,
    gesture: Res<Gesture>,
    mut sight: ResMut<Sight>,
) {
    if matches!(*gesture, Gesture::Pan) {
        sight.pan += drag.delta;
    }
}

/// Zooms about the pointer.
fn zoom(
    scroll: On<PointerScroll>,
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<Canvas>>,
    mut sight: ResMut<Sight>,
) {
    let Ok((node, transform)) = canvas.get(scroll.entity) else {
        return;
    };
    let notch = match scroll.unit {
        MouseScrollUnit::Line => scroll.y * 0.12,
        MouseScrollUnit::Pixel => scroll.y * 0.002,
    };
    let zoom =
        (sight.zoom * notch.exp()).clamp(LEAST_ZOOM, MOST_ZOOM);
    let at = scroll.pointer.position - corner(node, transform);
    let on_sheet = (at - sight.pan) / sight.zoom;
    sight.pan = at - on_sheet * zoom;
    sight.zoom = zoom;
}

/// Opens the menu of the card a right click is on, or the list of
/// kinds where it is on the bare canvas.
fn ask(
    press: On<PointerPress>,
    cards: Query<&Card>,
    parents: Query<&ChildOf>,
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<Canvas>>,
    sight: Res<Sight>,
    mut editor: ResMut<Editor>,
    mut menu: ResMut<Menu>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let at = press.pointer.position;
    let hit = press.original_event_target();
    let card = std::iter::once(hit)
        .chain(parents.iter_ancestors(hit))
        .find_map(|entity| cards.get(entity).ok());
    *menu = match card {
        Some(card) => {
            editor.selected = Some(card.0);
            Menu::Card { node: card.0, at }
        }
        None => {
            let Ok((node, transform)) = canvas.get(press.entity)
            else {
                return;
            };
            let on_sheet = (at - corner(node, transform) - sight.pan)
                / sight.zoom;
            Menu::Create { at, on_sheet }
        }
    };
}

fn start_link(
    mut start: On<PointerDragStart>,
    dots: Query<&Dot>,
    editor: Res<Editor>,
    mut gesture: ResMut<Gesture>,
    mut cursor: ResMut<OverrideCursor>,
) {
    start.propagate(false);
    let Ok(dot) = dots.get(start.entity) else {
        return;
    };
    let Some(node) = editor.sheet.graph.node(dot.port.node) else {
        return;
    };
    let (inputs, outputs) = rows(&editor, node);
    let Some(row) =
        outputs.iter().position(|other| other.port == dot.port)
    else {
        return;
    };
    let at = dot_at(
        card_at(&editor, dot.port.node),
        inputs.len() + row,
        true,
    );
    *gesture = Gesture::Link(
        dot.port.clone(),
        dot.value_type,
        at,
        Vec2::ZERO,
    );
    cursor.0 = Some(SystemCursorIcon::Crosshair);
}

fn drag_link(
    mut drag: On<PointerDrag>,
    sight: Res<Sight>,
    mut gesture: ResMut<Gesture>,
) {
    drag.propagate(false);
    if let Gesture::Link(_, _, _, gone) = &mut *gesture {
        *gone = drag.distance / sight.zoom;
    }
}

/// Wires the output dropped on an input's dot to it, in place of any
/// wire it had.
fn link(
    mut drop: On<PointerDragDrop>,
    dots: Query<&Dot>,
    mut editor: ResMut<Editor>,
) {
    drop.propagate(false);
    let (Ok(to), Ok(from)) =
        (dots.get(drop.entity), dots.get(drop.dropped))
    else {
        return;
    };
    if !from.output
        || to.output
        || !to.value_type.accepts(from.value_type)
    {
        return;
    }
    let mut edits = editor
        .sheet
        .graph
        .wires
        .iter()
        .filter(|wire| wire.to == to.port)
        .cloned()
        .map(Edit::Disconnect)
        .collect::<Vec<_>>();
    edits.push(Edit::Connect(Wire {
        from: from.port.clone(),
        to: to.port.clone(),
    }));
    editor.apply(Edit::Batch(edits));
}

/// Cuts the wire into an input whose dot is clicked.
fn cut(
    mut click: On<PointerClick>,
    dots: Query<&Dot>,
    mut editor: ResMut<Editor>,
) {
    click.propagate(false);
    let Ok(dot) = dots.get(click.entity) else {
        return;
    };
    let wires = editor
        .sheet
        .graph
        .wires
        .iter()
        .filter(|wire| wire.to == dot.port)
        .cloned()
        .map(Edit::Disconnect)
        .collect::<Vec<_>>();
    if !wires.is_empty() {
        editor.apply(Edit::Batch(wires));
    }
}

fn setting_of<'a>(
    editor: &'a Editor,
    chip: &Chip,
) -> Option<&'a Setting> {
    editor
        .sheet
        .graph
        .node(chip.node)
        .and_then(|node| node.settings.get(&chip.spec.name))
}

fn put(editor: &mut Editor, chip: &Chip, setting: Setting) {
    editor.apply(Edit::Set {
        node: chip.node,
        input: chip.spec.name.clone(),
        setting: Some(setting),
    });
}

fn scrub_start(
    mut start: On<PointerDragStart>,
    chips: Query<&Chip>,
    editor: Res<Editor>,
    mut gesture: ResMut<Gesture>,
    mut cursor: ResMut<OverrideCursor>,
) {
    start.propagate(false);
    let Ok(chip) = chips.get(start.entity) else {
        return;
    };
    if let Some(value) =
        number(chip.part, setting_of(&editor, chip), &chip.spec)
    {
        *gesture = Gesture::Scrub(chip.clone(), value);
        cursor.0 = Some(SystemCursorIcon::EwResize);
    }
}

/// Moves a number by how far the pointer went sideways: a tenth as
/// far with Shift held, ten times with Alt.
fn scrub(
    mut drag: On<PointerDrag>,
    keys: Res<ButtonInput<KeyCode>>,
    gesture: Res<Gesture>,
    mut editor: ResMut<Editor>,
) {
    drag.propagate(false);
    let Gesture::Scrub(chip, from) = &*gesture else {
        return;
    };
    let rate = if keys
        .any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    {
        0.1
    } else if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
    {
        10.0
    } else {
        1.0
    };
    let grain = if rate < 1.0 { 1000.0 } else { 100.0 };
    let value = ((from + drag.distance.x * STEP * rate) * grain)
        .round()
        / grain;
    if let Some(setting) = with_number(
        chip.part,
        setting_of(&editor, chip),
        &chip.spec,
        value,
    ) && Some(&setting) != setting_of(&editor, chip)
    {
        put(&mut editor, chip, setting);
    }
}

/// Turns a switch over, or moves a choice to the next: the one before
/// with Shift held.
fn flip(
    mut click: On<PointerClick>,
    keys: Res<ButtonInput<KeyCode>>,
    chips: Query<&Chip>,
    mut editor: ResMut<Editor>,
) {
    click.propagate(false);
    let Ok(chip) = chips.get(click.entity) else {
        return;
    };
    let current = setting_of(&editor, chip).cloned();
    let setting = match chip.part {
        Part::Flag => {
            let on = match (&current, &chip.spec.initial) {
                (Some(Setting::Fixed(Constant::Bool(on))), _)
                | (None, Some(Constant::Bool(on))) => *on,
                _ => true,
            };
            Setting::Fixed(Constant::Bool(!on))
        }
        Part::Choice => {
            let choices = chip.spec.choices;
            let now = match (&current, &chip.spec.initial) {
                (
                    Some(Setting::Fixed(Constant::Choice(choice))),
                    _,
                )
                | (None, Some(Constant::Choice(choice))) => {
                    choice.clone()
                }
                _ => String::new(),
            };
            let at =
                choices.iter().position(|c| *c == now).unwrap_or(0);
            let back = keys.any_pressed([
                KeyCode::ShiftLeft,
                KeyCode::ShiftRight,
            ]);
            let next =
                if back { at + choices.len() - 1 } else { at + 1 }
                    % choices.len();
            Setting::Fixed(Constant::Choice(choices[next].to_owned()))
        }
        _ => return,
    };
    put(&mut editor, chip, setting);
}
