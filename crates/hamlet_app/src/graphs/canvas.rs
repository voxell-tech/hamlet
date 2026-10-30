//! The canvas: each node a card, each wire a curve of short segments,
//! all on a sheet that pans and zooms. A card is dragged to move it,
//! an output's dot onto an input's to wire them, and an input's dot
//! is clicked to cut its wire. An unwired input's row holds the
//! controls for its value, and a right click on it asks what sets it.

use std::collections::{BTreeMap, BTreeSet};

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
    ComputedNode, Display, FlexDirection, JustifyContent, Node,
    Overflow, PositionType, UiGlobalTransform, UiRect, UiTransform,
    Val, Val2, ZIndex, px,
};
use bevy::ui_render::ui_material::MaterialNode;
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{BehaviorExt, frame};
use bevy_fynix::{EntityCursor, OverrideCursor, Theme};
use hamlet_blueprint::{
    Constant, Does, Edit, Initial, NodeId, Port, PortSpec, ValueType,
    Wire, ports,
};
use hamlet_ui::styles::Fills;
use hamlet_ui::theme::{HamletTheme, Palette};
use hamlet_ui::{View, with_cx};

use super::menu::Menu;
use super::wire::{Shape, WireMaterial};
use super::{Editor, controls};

const WIDTH: f32 = 236.0;
const HEADER: f32 = 26.0;
const ROW: f32 = 28.0;
const PAD: f32 = 4.0;
const BORDER: f32 = 1.0;
const DOT: f32 = 10.0;
const LEAST_ZOOM: f32 = 0.3;
const MOST_ZOOM: f32 = 2.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Sight>()
        .init_resource::<Gesture>()
        .add_systems(Update, (build, place, paint, lasso).chain());
}

/// Marks the frame the canvas is drawn in.
#[derive(Component)]
pub(super) struct Canvas;

/// The node every card and wire hangs off, moved and scaled to pan
/// and zoom them as one.
#[derive(Component)]
struct Sheet;

/// The box a Shift-drag on the bare canvas draws to select the cards
/// it touches.
#[derive(Component)]
struct Lasso;

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

/// An input's row on a card, which a right click asks about.
#[derive(Component, Clone)]
struct InputRow {
    node: NodeId,
    input: Input,
}

/// What a row knows of the input it shows: a kind's port, or an
/// input of the graph a node runs.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Input {
    pub(super) name: String,
    pub(super) value_type: ValueType,
    pub(super) initial: Option<Constant>,
    /// The choices a choice takes. Empty for any text.
    pub(super) choices: &'static [&'static str],
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
    /// A box from one corner on the sheet to the other, and what was
    /// selected before it.
    Lasso(Vec2, Vec2, BTreeSet<NodeId>),
    /// The cards dragged, each with where it was when the drag began.
    Cards(Vec<(NodeId, Vec2)>),
    /// A wire out of an output's port, from where on the sheet, and
    /// how far the pointer has gone.
    Link(Port, ValueType, Vec2, Vec2),
}

pub(super) fn view() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        frame()
            .fills_width()
            .fills_height()
            .overflow(Overflow::clip())
            .fill(theme.palette.input)
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

/// What a row needs of each input of `node`: its kind's ports, or
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

/// A place for every card of the open sheet: a column for each step
/// from the graph's start, each card beside the cards that feed it,
/// and each column stacked by the cards' heights.
pub(super) fn arrange(editor: &Editor) -> BTreeMap<NodeId, Vec2> {
    const ACROSS: f32 = 72.0;
    const DOWN: f32 = 24.0;
    let graph = &editor.sheet.graph;
    let mut step = BTreeMap::<NodeId, usize>::new();
    // A wire's far end is a step past its near one; as many passes as
    // there are nodes settle every chain, and stop a loop.
    for _ in 0..graph.nodes.len() {
        let mut moved = false;
        for wire in &graph.wires {
            let next = step.get(&wire.from.node).map_or(1, |s| s + 1);
            if step.get(&wire.to.node).is_none_or(|&s| s < next) {
                step.insert(wire.to.node, next);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let mut columns = BTreeMap::<usize, Vec<NodeId>>::new();
    for node in &graph.nodes {
        columns
            .entry(step.get(&node.id).copied().unwrap_or_default())
            .or_default()
            .push(node.id);
    }
    let mut placed = BTreeMap::new();
    for (column, nodes) in columns {
        // Each card goes as near as it can to the middle of the cards
        // that feed it, all in columns already placed.
        let mut wanted: Vec<(NodeId, f32)> = nodes
            .into_iter()
            .map(|node| {
                let ys: Vec<f32> = graph
                    .wires
                    .iter()
                    .filter(|wire| wire.to.node == node)
                    .filter_map(|wire| placed.get(&wire.from.node))
                    .map(|at: &Vec2| at.y)
                    .collect();
                let y = if ys.is_empty() {
                    f32::MAX
                } else {
                    ys.iter().sum::<f32>() / ys.len() as f32
                };
                (node, y)
            })
            .collect();
        wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
        let x = column as f32 * (WIDTH + ACROSS);
        let mut y = 0.0_f32;
        for (id, want) in wanted {
            let Some(node) = graph.node(id) else {
                continue;
            };
            if want < f32::MAX {
                y = y.max(want);
            }
            placed.insert(id, Vec2::new(x, y));
            let (inputs, outputs) = rows(editor, node);
            y += height(inputs.len() + outputs.len()) + DOWN;
        }
    }
    placed
}

/// The colour of a wire or a dot of `value_type`.
fn tint(palette: &Palette, value_type: ValueType) -> Color {
    match value_type {
        ValueType::Number => palette.gold_3,
        ValueType::Bool => palette.green,
        ValueType::Point | ValueType::Curve => palette.agent,
        ValueType::Choice => palette.grey_1,
        ValueType::Placements => palette.glow,
        ValueType::Field => palette.particle,
        ValueType::Geometry => palette.glow_core,
    }
}

/// Builds every card again when the nodes, the wires or how an input
/// is set change, and when the canvas is new.
fn build(
    mut commands: Commands,
    editor: Res<Editor>,
    // The theme loads after the first frames.
    If(theme): If<Res<Theme<HamletTheme>>>,
    canvas: Query<Entity, With<Canvas>>,
    cards: Query<Entity, With<Card>>,
    mut built: Local<Option<(Entity, Entity, u64)>>,
) {
    let Some(canvas) = canvas.iter().next() else {
        return;
    };
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
            let sheet = commands
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
                .id();
            commands.spawn((
                Lasso,
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    border: UiRect::all(px(1.0)),
                    ..Node::default()
                },
                BackgroundColor(
                    theme.0.palette.glow.with_alpha(0.08),
                ),
                BorderColor::all(theme.0.palette.glow),
                ZIndex(1),
                Pickable::IGNORE,
                ChildOf(sheet),
            ));
            sheet
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
            Does::Kind(kind) => {
                (kind.replace('_', " "), palette.code, palette.gold_1)
            }
            Does::Graph(graph) => (
                graph.clone(),
                palette.primary_low,
                palette.glow_core,
            ),
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
                BorderColor::all(palette.border),
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
            if let (false, Some(spec)) = (dot.output, spec) {
                commands.entity(row).insert(InputRow {
                    node: node.id,
                    input: spec.clone(),
                });
            }
            if let (false, false, Some(spec)) =
                (dot.output, wired, spec)
            {
                let (id, spec) = (node.id, spec.clone());
                let setting = node.settings.get(&spec.name).cloned();
                commands.queue(move |world: &mut World| {
                    if let Some(control) =
                        controls::control(id, spec, setting.as_ref())
                    {
                        let root = bevy_fynix::mount::<HamletTheme>(
                            world, control,
                        );
                        // A drag in a control is the control's, not
                        // the card's.
                        world
                            .entity_mut(root)
                            .insert(ChildOf(row))
                            .observe(hold_drag_start)
                            .observe(hold_drag)
                            .observe(hold_drag_end);
                    }
                });
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
                BorderColor::all(palette.input),
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
        let colour = if editor.selected.contains(&card.0) {
            palette.glow
        } else if hovered.get() {
            palette.gold_5
        } else {
            palette.border
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
            palette.glow_core
        } else {
            palette.input
        });
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
    if let Gesture::Cards(cards) = &*gesture {
        let moved: Vec<_> = cards
            .iter()
            .filter(|(node, from)| {
                editor.sheet.layout.get(node) != Some(from)
            })
            .copied()
            .collect();
        if !moved.is_empty() {
            editor.dropped(moved);
        }
    }
    *gesture = Gesture::None;
    cursor.0 = None;
}

/// Selects a card alone, or with Shift adds it to the selection or
/// takes it out.
fn select(
    mut click: On<PointerClick>,
    cards: Query<&Card>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut editor: ResMut<Editor>,
) {
    click.propagate(false);
    let Ok(card) = cards.get(click.entity) else {
        return;
    };
    if held_shift(keys.as_deref()) {
        if !editor.selected.remove(&card.0) {
            editor.selected.insert(card.0);
        }
    } else if editor.selected != BTreeSet::from([card.0]) {
        editor.selected = BTreeSet::from([card.0]);
    }
}

fn held_shift(keys: Option<&ButtonInput<KeyCode>>) -> bool {
    keys.is_some_and(|keys| {
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    })
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

fn deselect(
    click: On<PointerClick>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut editor: ResMut<Editor>,
) {
    if click.entity == click.original_event_target()
        && !held_shift(keys.as_deref())
        && !editor.selected.is_empty()
    {
        editor.selected.clear();
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
        // A card dragged from outside the selection is dragged alone.
        if !editor.selected.contains(&card.0) {
            editor.selected = BTreeSet::from([card.0]);
        }
        *gesture = Gesture::Cards(
            editor
                .selected
                .iter()
                .map(|node| (*node, card_at(&editor, *node)))
                .collect(),
        );
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
    if let Gesture::Cards(cards) = &*gesture {
        for (node, from) in cards {
            editor.drag(*node, *from + drag.distance / sight.zoom);
        }
    }
}

/// Pans on a drag of the bare canvas, or with Shift draws a box to
/// select by.
fn pan_start(
    start: On<PointerDragStart>,
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<Canvas>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    sight: Res<Sight>,
    editor: Res<Editor>,
    mut gesture: ResMut<Gesture>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if start.entity != start.original_event_target() {
        return;
    }
    if held_shift(keys.as_deref())
        && let Ok((node, transform)) = canvas.get(start.entity)
    {
        let at = (start.pointer.position
            - corner(node, transform)
            - sight.pan)
            / sight.zoom;
        *gesture = Gesture::Lasso(at, at, editor.selected.clone());
        cursor.0 = Some(SystemCursorIcon::Crosshair);
    } else {
        *gesture = Gesture::Pan;
        cursor.0 = Some(SystemCursorIcon::Move);
    }
}

fn pan(
    drag: On<PointerDrag>,
    mut gesture: ResMut<Gesture>,
    mut editor: ResMut<Editor>,
    mut sight: ResMut<Sight>,
) {
    match &mut *gesture {
        Gesture::Pan => sight.pan += drag.delta,
        Gesture::Lasso(from, to, before) => {
            *to = *from + drag.distance / sight.zoom;
            let (low, high) = (from.min(*to), from.max(*to));
            let mut selected = before.clone();
            for node in &editor.sheet.graph.nodes {
                let at = card_at(&editor, node.id);
                let (inputs, outputs) = rows(&editor, node);
                let far = at
                    + Vec2::new(
                        WIDTH,
                        height(inputs.len() + outputs.len()),
                    );
                if at.cmplt(high).all() && far.cmpgt(low).all() {
                    selected.insert(node.id);
                }
            }
            if editor.selected != selected {
                editor.selected = selected;
            }
        }
        _ => {}
    }
}

/// Shows the box of a Shift-drag where it is, and hides it after.
fn lasso(
    gesture: Res<Gesture>,
    mut boxes: Query<&mut Node, With<Lasso>>,
) {
    if !gesture.is_changed() {
        return;
    }
    for mut node in &mut boxes {
        match &*gesture {
            Gesture::Lasso(from, to, _) => {
                let (low, high) = (from.min(*to), from.max(*to));
                node.display = Display::Flex;
                node.left = px(low.x);
                node.top = px(low.y);
                node.width = px(high.x - low.x);
                node.height = px(high.y - low.y);
            }
            _ if node.display != Display::None => {
                node.display = Display::None;
            }
            _ => {}
        }
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
    rows: Query<&InputRow>,
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
    let up =
        || std::iter::once(hit).chain(parents.iter_ancestors(hit));
    let row = up().find_map(|entity| rows.get(entity).ok());
    let card = up().find_map(|entity| cards.get(entity).ok());
    *menu = match (row, card) {
        (Some(row), _) => {
            if !editor.selected.contains(&row.node) {
                editor.selected = BTreeSet::from([row.node]);
            }
            Menu::Input {
                node: row.node,
                input: row.input.clone(),
                at,
            }
        }
        (None, Some(card)) => {
            if !editor.selected.contains(&card.0) {
                editor.selected = BTreeSet::from([card.0]);
            }
            Menu::Card { node: card.0, at }
        }
        (None, None) => {
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

fn hold_drag_start(mut start: On<PointerDragStart>) {
    start.propagate(false);
}

fn hold_drag(mut drag: On<PointerDrag>) {
    drag.propagate(false);
}

fn hold_drag_end(mut end: On<PointerDragEnd>) {
    end.propagate(false);
}
