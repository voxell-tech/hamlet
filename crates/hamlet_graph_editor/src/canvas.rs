//! The canvas: each node a card, each wire a curve of short segments,
//! all on a sheet that pans and zooms. A card is dragged to move it,
//! an output's dot onto an input's to wire them, and an input's dot
//! is clicked to cut its wire. An unwired input's row holds the
//! controls for its value, and a right click on it asks what sets it.
//! A group is a titled frame around its cards, dragged by its title
//! bar to move them all.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use bevy::app::{App, Update};
use bevy::asset::Assets;
use bevy::color::{Alpha, Color};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Changed, Has, Or, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, If, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::{MouseButton, MouseScrollUnit};
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::Vec2;
use bevy::picking::Pickable;
use bevy::picking::events::{
    PointerClick, PointerDrag, PointerDragDrop, PointerDragEnd,
    PointerDragStart, PointerPress, PointerScroll,
};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::text::{
    EditableText, FontSize, TextColor, TextEdit, TextFont, TextLayout,
};
use bevy::time::Time;
use bevy::ui::widget::Text;
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius,
    ComputedNode, Display, FlexDirection, JustifyContent, Node,
    Outline, Overflow, PositionType, UiGlobalTransform, UiRect,
    UiTransform,
    Val, Val2, ZIndex, px,
};
use bevy::ui_render::ui_material::MaterialNode;
use bevy::window::{RequestRedraw, SystemCursorIcon, Window};
use bevy_fynix::views::{BehaviorExt, frame};
use bevy_fynix::{EntityCursor, OverrideCursor, Theme};
use hamlet_graph::{
    Constant, Does, Edit, GraphOutput, Listed, NodeId, Port,
    PortSpec, ValueType, Wire, ports,
};
use hamlet_ui::styles::Fills;
use hamlet_ui::theme::{HamletTheme, Palette};
use hamlet_ui::{View, with_cx};

use super::menu::Which;
use super::setup::EditorKey;
use super::wire::{Shape, WireMaterial};
use super::{Editor, OUTPUTS, controls};
use crate::library::Item;
use crate::library::carry::{Carried, tile_of};

/// The gap between two columns of cards laid out, and between the
/// last card and the Outputs card.
const ACROSS: f32 = 72.0;
const WIDTH: f32 = 236.0;
const HEADER: f32 = 26.0;
const ROW: f32 = 28.0;
const PAD: f32 = 4.0;
const BORDER: f32 = 1.0;
const DOT: f32 = 20.0;
/// The size of the area a dot is pointed at in, larger than the dot
/// so it is easy to hit.
const DOT_HIT: f32 = DOT * 2.0;
/// How far a group's frame stands off its cards, and its title bar's
/// height.
const MARGIN: f32 = 16.0;
const TITLE: f32 = 28.0;
/// The size a group's title is drawn at on screen, however far the
/// sheet is zoomed out.
const TITLE_FONT: f32 = 15.0;
const LEAST_ZOOM: f32 = 0.3;
const MOST_ZOOM: f32 = 2.0;

/// How quickly the zoom goes to the one a scroll asked for, per
/// second.
const ZOOM_EASE: f32 = 16.0;

/// How far, in logical pixels, a right press moves before it pans
/// rather than opening a menu.
const PAN_SLOP: f32 = 4.0;

pub(super) fn plugin<K: EditorKey>(app: &mut App) {
    app.init_resource::<Dragging>()
        .init_resource::<Sight<K>>()
        .init_resource::<Held<K>>()
        .init_resource::<RightPress<K>>()
        .add_systems(
            Update,
            (
                (right_drag::<K>, ease_zoom::<K>),
                build::<K>,
                place::<K>,
                paint::<K>,
                lasso::<K>,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (forget_ended::<K>, cancel::<K>, faint::<K>).chain(),
        )
        .add_systems(
            Update,
            (titles::<K>, retitled::<K>, drop_hint::<K>),
        );
}

/// Outlines the canvas while a carried asset is over it, where letting
/// it go would put it down.
fn drop_hint<K: EditorKey>(
    mut commands: Commands,
    carried: Res<Carried>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    canvas: Query<(Entity, &Hovered, Has<Outline>), With<Canvas<K>>>,
) {
    for (canvas, hovered, outlined) in &canvas {
        let wanted = carried.item.is_some() && hovered.get();
        if wanted && !outlined {
            commands.entity(canvas).insert(Outline::new(
                px(2.0),
                px(-2.0),
                theme.0.palette.glow,
            ));
        } else if !wanted && outlined {
            commands.entity(canvas).remove::<Outline>();
        }
    }
}

/// Set while any canvas drags a wire.
#[derive(Resource, Default)]
pub struct Dragging(pub bool);

/// The frame the canvas is drawn in.
#[derive(Component)]
pub(super) struct Canvas<K: EditorKey>(PhantomData<K>);

impl<K: EditorKey> Canvas<K> {
    pub(super) fn new() -> Self {
        Self(PhantomData)
    }
}

/// The node every card and wire hangs off, moved and scaled to pan
/// and zoom them as one.
#[derive(Component)]
struct Sheet<K: EditorKey>(PhantomData<K>);

/// The box a Shift-drag on the bare canvas draws to select the cards
/// it touches.
#[derive(Component)]
struct Lasso<K: EditorKey>(PhantomData<K>);

/// A node's card.
#[derive(Component)]
struct Card<K: EditorKey>(NodeId, PhantomData<K>);

/// The room under a card's header, filled by whatever draws there.
#[derive(Component)]
pub struct Room<K: EditorKey>(pub NodeId, pub PhantomData<K>);

/// A group's frame, by its place among the sheet's groups.
#[derive(Component)]
struct Frame<K: EditorKey>(usize, PhantomData<K>);

/// A group's title bar, which drags its cards.
#[derive(Component)]
struct TitleBar<K: EditorKey>(usize, PhantomData<K>);

/// A group's title, drawn on its bar.
#[derive(Component)]
struct TitleText<K: EditorKey>(usize, PhantomData<K>);

/// The group whose title is being written, and the field it is
/// written in.
#[derive(Resource)]
struct Retitling<K: EditorKey> {
    text: Entity,
    field: Entity,
    focused: bool,
    key: PhantomData<K>,
}

/// The node a wire is drawn on: the wire's, or none for one being
/// dragged out of a port.
#[derive(Component)]
struct Drawn<K: EditorKey>(Option<Wire>, PhantomData<K>);

/// The edge of a card a port's dot sits on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Input,
    Output,
}

/// A port's dot on a card: the area it is pointed at in.
#[derive(Component)]
struct Dot<K: EditorKey> {
    port: Port,
    side: Side,
    value_type: ValueType,
    key: PhantomData<K>,
}

impl<K: EditorKey> Dot<K> {
    fn is_output(&self) -> bool {
        self.side == Side::Output
    }

    /// Whether this is a row of the Outputs card.
    fn is_on_outputs(&self) -> bool {
        self.port.node == OUTPUTS
    }

    /// Whether this is the empty row of the Outputs card, which a
    /// wire into adds an output.
    fn is_blank(&self) -> bool {
        self.is_on_outputs() && self.port.name.is_empty()
    }

    /// Whether a wire carrying `value_type` can end here. A row of
    /// the Outputs card takes any.
    fn takes(&self, value_type: ValueType) -> bool {
        self.is_on_outputs() || self.value_type.accepts(value_type)
    }
}

/// The drawn dot inside a dot's area, which paint colours.
#[derive(Component)]
struct Disc<K: EditorKey>(Entity, PhantomData<K>);

/// An input's row on a card, which a right click asks about.
#[derive(Component)]
struct InputRow<K: EditorKey> {
    node: NodeId,
    input: Input,
    key: PhantomData<K>,
}

/// An input as a row shows it: a kind's port, or an input of the
/// graph a node runs.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Input {
    pub(super) name: String,
    pub(super) value_type: ValueType,
    pub(super) initial: Option<Constant>,
    /// The choices a choice takes. Empty for any text.
    pub(super) choices: &'static [&'static str],
    /// The catalogue the choices are taken from.
    pub(super) listed: Option<Listed>,
}

impl Input {
    fn of_port(spec: &PortSpec) -> Self {
        Self {
            name: spec.name.to_owned(),
            value_type: spec.value_type,
            initial: spec.initial.map(Constant::from),
            choices: spec.choices,
            listed: spec.listed,
        }
    }
}

/// How far the sheet is panned, in canvas pixels, and how much it is
/// zoomed.
#[derive(Resource)]
struct Sight<K: EditorKey> {
    pan: Vec2,
    zoom: f32,
    /// The zoom a scroll asked for, which [`ease_zoom`] goes to.
    goal: Option<ZoomGoal>,
    key: PhantomData<K>,
}

impl<K: EditorKey> Sight<K> {
    fn new(pan: Vec2, zoom: f32) -> Self {
        Self {
            pan,
            zoom,
            goal: None,
            key: PhantomData,
        }
    }
}

/// A zoom to go to, about a point of the canvas that stays put.
#[derive(Clone, Copy)]
struct ZoomGoal {
    zoom: f32,
    about: Vec2,
}

/// A press of the right button on the canvas, until it is let go:
/// the menu it opens if it does not move, or the sheet panning once
/// it has.
#[derive(Resource)]
struct RightPress<K: EditorKey> {
    held: Option<RightHeld>,
    key: PhantomData<K>,
}

impl<K: EditorKey> Default for RightPress<K> {
    fn default() -> Self {
        Self {
            held: None,
            key: PhantomData,
        }
    }
}

/// What a right press holds while it is down. Places are in the
/// window's logical pixels.
struct RightHeld {
    menu: Which,
    from: Vec2,
    last: Vec2,
    panning: bool,
}

impl<K: EditorKey> Default for Sight<K> {
    fn default() -> Self {
        Self::new(Vec2::splat(16.0), 1.0)
    }
}

/// The gesture a drag on one canvas is doing.
#[derive(Resource)]
struct Held<K: EditorKey>(Gesture, PhantomData<K>);

impl<K: EditorKey> Default for Held<K> {
    fn default() -> Self {
        Self(Gesture::None, PhantomData)
    }
}

impl<K: EditorKey> Deref for Held<K> {
    type Target = Gesture;

    fn deref(&self) -> &Gesture {
        &self.0
    }
}

impl<K: EditorKey> DerefMut for Held<K> {
    fn deref_mut(&mut self) -> &mut Gesture {
        &mut self.0
    }
}

/// What a drag on the canvas is doing.
#[derive(Default, Clone)]
enum Gesture {
    #[default]
    None,
    Pan,
    /// A box from one corner on the sheet to the other, and what was
    /// selected before it.
    Lasso(Vec2, Vec2, BTreeSet<NodeId>),
    /// A box or a wire let go of this frame, so the click its release
    /// makes neither clears what the box selected nor cuts the wire.
    Ended,
    /// The cards dragged, each with where it was when the drag began.
    Cards(Vec<(NodeId, Vec2)>),
    /// A wire out of an output's port, dragged by its free end.
    Link(Link),
}

/// A wire being dragged: the output it comes from, where on the sheet
/// that port is and where its free end was taken hold of, how far the
/// pointer has gone since, and the wire it was taken off an input as,
/// which is hidden until it lands.
#[derive(Clone)]
struct Link {
    from: Port,
    value_type: ValueType,
    start: Vec2,
    grabbed: Vec2,
    gone: Vec2,
    lifted: Option<Wire>,
}

impl Link {
    /// Where the free end is on the sheet.
    fn end(&self) -> Vec2 {
        self.grabbed + self.gone
    }
}

pub(super) fn view<K: EditorKey>() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        frame()
            .fills_width()
            .fills_height()
            .overflow(Overflow::clip())
            .fill(theme.palette.input)
            .radius(theme.space.round)
            .tagged(Canvas::<K>::new())
    })
}

/// The middle of what is in sight, in the sheet's layout, for a new
/// card.
pub(super) fn middle<K: EditorKey>(world: &mut World) -> Vec2 {
    let size = canvas_size::<K>(world);
    let sight = world.resource::<Sight<K>>();
    (size * 0.5 - sight.pan) / sight.zoom
        - Vec2::new(WIDTH * 0.5, HEADER)
}

/// Pans and zooms so that every card is in sight.
pub fn fit<K: EditorKey>(world: &mut World) {
    let size = canvas_size::<K>(world);
    let editor = world.resource::<Editor<K>>();
    let (mut low, mut high) =
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for id in card_ids(editor) {
        let at = card_at(editor, id);
        low = low.min(at);
        high = high.max(at + card_size(editor, id));
    }
    for at in 0..editor.sheet.groups.len() {
        if let Some((near, far)) = frame_bounds(editor, at) {
            low = low.min(near);
            high = high.max(far);
        }
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
    *world.resource_mut::<Sight<K>>() = Sight::new(pan, zoom);
}

/// Where the window's point `at` is on the sheet, if it is over the
/// canvas.
pub(super) fn on_sheet<K: EditorKey>(
    world: &mut World,
    at: Vec2,
) -> Option<Vec2> {
    let corner = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Canvas<K>>>()
        .iter(world)
        .next()
        .map(|(node, transform)| corner(node, transform))?;
    let size = canvas_size::<K>(world);
    let inside = at - corner;
    if inside.cmplt(Vec2::ZERO).any() || inside.cmpgt(size).any() {
        return None;
    }
    let sight = world.resource::<Sight<K>>();
    Some((inside - sight.pan) / sight.zoom)
}

pub(super) fn is_hovered<K: EditorKey>(world: &mut World) -> bool {
    world
        .query_filtered::<&Hovered, With<Canvas<K>>>()
        .iter(world)
        .any(Hovered::get)
}

/// A node's top left corner in the window, in logical pixels.
pub fn corner(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Vec2 {
    (transform.translation - node.size() * 0.5)
        * node.inverse_scale_factor()
}

fn canvas_size<K: EditorKey>(world: &mut World) -> Vec2 {
    world
        .query_filtered::<&ComputedNode, With<Canvas<K>>>()
        .iter(world)
        .next()
        .map_or(Vec2::splat(400.0), |node| {
            node.size() * node.inverse_scale_factor()
        })
}

/// The room under the header of the card `node`: the setup's for a
/// node's, none for the Outputs card.
fn room_of<K: EditorKey>(editor: &Editor<K>, node: NodeId) -> f32 {
    if node == OUTPUTS {
        0.0
    } else {
        editor.setup.room
    }
}

/// Every card of the open sheet: its nodes, then the Outputs card.
fn card_ids<K: EditorKey>(editor: &Editor<K>) -> Vec<NodeId> {
    editor
        .sheet
        .graph
        .nodes
        .iter()
        .map(|node| node.id)
        .chain(editor.setup.outputs.then_some(OUTPUTS))
        .collect()
}

/// The rows of the Outputs card: one for each output, named after
/// it, and an empty one after them to wire into.
fn output_rows<K: EditorKey>(editor: &Editor<K>) -> Vec<Dot<K>> {
    editor
        .sheet
        .graph
        .outputs
        .iter()
        .map(|output| (output.name.as_str(), output.value_type))
        .chain([("", ValueType::Objects)])
        .map(|(name, value_type)| Dot {
            port: Port {
                node: OUTPUTS,
                name: name.to_owned(),
            },
            side: Side::Input,
            value_type,
            key: PhantomData,
        })
        .collect()
}

/// The input and output rows of the card `id`, if there is one.
fn card_rows<K: EditorKey>(
    editor: &Editor<K>,
    id: NodeId,
) -> Option<(Vec<Dot<K>>, Vec<Dot<K>>)> {
    if id == OUTPUTS {
        return editor
            .setup
            .outputs
            .then(|| (output_rows(editor), Vec::new()));
    }
    editor.sheet.graph.node(id).map(|node| rows(editor, node))
}

/// The size of the card `id` on the sheet.
fn card_size<K: EditorKey>(editor: &Editor<K>, id: NodeId) -> Vec2 {
    let rows = card_rows(editor, id)
        .map_or(0, |(inputs, outputs)| inputs.len() + outputs.len());
    Vec2::new(
        WIDTH,
        BORDER * 2.0
            + HEADER
            + PAD * 2.0
            + room_of(editor, id)
            + rows as f32 * ROW,
    )
}

/// Every wire drawn: those of the graph, and one from each output's
/// port to its row on the Outputs card.
fn drawn_wires<K: EditorKey>(editor: &Editor<K>) -> Vec<Wire> {
    let outputs = editor
        .setup
        .outputs
        .then_some(&editor.sheet.graph.outputs)
        .into_iter()
        .flatten()
        .map(|output| Wire {
            from: output.from.clone(),
            to: Port {
                node: OUTPUTS,
                name: output.name.clone(),
            },
        });
    editor
        .sheet
        .graph
        .wires
        .iter()
        .cloned()
        .chain(outputs)
        .collect()
}

/// The edit that takes `wire` away: a wire of the graph is cut, a
/// wire into the Outputs card removes its output.
fn unwire(wire: Wire) -> Edit {
    if wire.to.node == OUTPUTS {
        Edit::RemoveOutput(wire.to.name)
    } else {
        Edit::Disconnect(wire)
    }
}

/// The inputs and outputs of `node` in the order drawn. The input
/// that names the node is its title and is left out, as is an input
/// the name does not use unless it is wired.
fn rows<K: EditorKey>(
    editor: &Editor<K>,
    node: &hamlet_graph::Node,
) -> (Vec<Dot<K>>, Vec<Dot<K>>) {
    let Ok((inputs, outputs)) = ports(
        &editor.registry,
        &editor.rules,
        &editor.sheet.graph,
        node,
    ) else {
        return (Vec::new(), Vec::new());
    };
    let dot = |side: Side| {
        move |info: hamlet_graph::PortInfo| Dot {
            port: Port {
                node: node.id,
                name: info.name,
            },
            side,
            value_type: info.value_type,
            key: PhantomData,
        }
    };
    let named = super::naming(&editor.registry, node);
    let kind = match &node.does {
        Does::Kind(kind) => editor.registry.get(kind),
        Does::Graph(_) | Does::Input(_) => None,
    };
    let drawn = |name: &str| {
        let Some((naming, chosen)) = &named else {
            return true;
        };
        let used = kind.and_then(|kind| kind.input(name)).is_none_or(
            |spec| {
                spec.used_with.is_empty()
                    || spec.used_with.contains(&chosen.as_str())
            },
        );
        let wired = editor.sheet.graph.wires.iter().any(|wire| {
            wire.to.node == node.id && wire.to.name == name
        });
        name != naming.name && (used || wired)
    };
    (
        inputs
            .into_iter()
            .filter(|info| drawn(&info.name))
            .map(dot(Side::Input))
            .collect(),
        outputs.into_iter().map(dot(Side::Output)).collect(),
    )
}

/// The inputs of `node` as rows show them: its kind's ports, each
/// port of a growing one too, or the inputs of the graph it runs.
fn specs<K: EditorKey>(
    editor: &Editor<K>,
    node: NodeId,
) -> Vec<Input> {
    let found = editor.sheet.graph.node(node);
    match found.map(|node| &node.does) {
        Some(Does::Kind(kind)) => {
            let (Some(kind), Some(found)) =
                (editor.registry.get(kind), found)
            else {
                return Vec::new();
            };
            let Ok((inputs, _)) = ports(
                &editor.registry,
                &editor.rules,
                &editor.sheet.graph,
                found,
            ) else {
                return Vec::new();
            };
            inputs
                .into_iter()
                .filter_map(|info| {
                    Some(Input {
                        name: info.name.clone(),
                        ..Input::of_port(kind.input(&info.name)?)
                    })
                })
                .collect()
        }
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
                        listed: None,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Some(Does::Input(_)) | None => Vec::new(),
    }
}

/// Where a port's dot is on the sheet, from its card's place, the
/// room under its header and its row.
fn dot_at(card: Vec2, room: f32, row: usize, side: Side) -> Vec2 {
    Vec2::new(
        card.x
            + match side {
                Side::Input => 0.0,
                Side::Output => WIDTH,
            },
        card.y
            + BORDER
            + HEADER
            + PAD
            + room
            + row as f32 * ROW
            + ROW * 0.5,
    )
}

/// Where the card `node` is on the sheet. The Outputs card is where
/// the sheet puts it, or beside the other cards.
fn card_at<K: EditorKey>(editor: &Editor<K>, node: NodeId) -> Vec2 {
    if node != OUTPUTS {
        return editor
            .sheet
            .layout
            .get(&node)
            .copied()
            .unwrap_or_default();
    }
    editor.sheet.outputs_at.unwrap_or_else(|| {
        let places = editor
            .sheet
            .graph
            .nodes
            .iter()
            .map(|found| card_at(editor, found.id))
            .collect::<Vec<_>>();
        let right =
            places.iter().map(|at| at.x + WIDTH).reduce(f32::max);
        let top = places.iter().map(|at| at.y).reduce(f32::min);
        Vec2::new(
            right.map_or(0.0, |right| right + ACROSS),
            top.unwrap_or_default(),
        )
    })
}

/// The corners of the frame of the group at `at` on the sheet, around
/// its cards and its title bar above them; none for a group with no
/// cards.
fn frame_bounds<K: EditorKey>(
    editor: &Editor<K>,
    at: usize,
) -> Option<(Vec2, Vec2)> {
    let group = editor.sheet.groups.get(at)?;
    let (mut low, mut high) =
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for node in &group.nodes {
        let Some(found) = editor.sheet.graph.node(*node) else {
            continue;
        };
        let corner = card_at(editor, found.id);
        low = low.min(corner);
        high = high.max(corner + card_size(editor, found.id));
    }
    (low.x <= high.x).then(|| {
        (
            low - Vec2::new(MARGIN, MARGIN + TITLE),
            high + Vec2::splat(MARGIN),
        )
    })
}

/// A place for every card of the open sheet: a column for each step
/// from the graph's start, each card beside the cards that feed it,
/// and each column stacked by the cards' heights. Each group is a
/// band of its own, below the one before, so no two frames overlap;
/// the cards in no group are the last band.
pub(super) fn arrange<K: EditorKey>(
    editor: &Editor<K>,
) -> BTreeMap<NodeId, Vec2> {
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
    let groups = &editor.sheet.groups;
    let mut bands = groups
        .iter()
        .map(|group| {
            let nodes = graph
                .nodes
                .iter()
                .map(|node| node.id)
                .filter(|id| group.nodes.contains(id))
                .collect::<Vec<_>>();
            (true, nodes)
        })
        .collect::<Vec<_>>();
    bands.push((
        false,
        graph
            .nodes
            .iter()
            .map(|node| node.id)
            .filter(|id| {
                !groups.iter().any(|group| group.nodes.contains(id))
            })
            .collect(),
    ));
    let mut placed = BTreeMap::<NodeId, Vec2>::new();
    let mut top = 0.0_f32;
    for (framed, band) in bands {
        if band.is_empty() {
            continue;
        }
        // A frame stands off its cards, its title above them.
        let start = if framed { top + MARGIN + TITLE } else { top };
        let mut bottom = start;
        let mut columns = BTreeMap::<usize, Vec<NodeId>>::new();
        for node in band {
            columns
                .entry(step.get(&node).copied().unwrap_or_default())
                .or_default()
                .push(node);
        }
        for (column, nodes) in columns {
            // Each card goes as near as it can to the middle of the
            // cards that feed it, all in columns already placed.
            let mut wanted = nodes
                .into_iter()
                .map(|node| {
                    let feeders = graph
                        .wires
                        .iter()
                        .filter(|wire| wire.to.node == node)
                        .filter_map(|wire| {
                            placed.get(&wire.from.node)
                        })
                        .map(|at| at.y)
                        .collect::<Vec<_>>();
                    let y = if feeders.is_empty() {
                        f32::MAX
                    } else {
                        feeders.iter().sum::<f32>()
                            / feeders.len() as f32
                    };
                    (node, y)
                })
                .collect::<Vec<_>>();
            wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
            let x = column as f32 * (WIDTH + ACROSS);
            let mut y = start;
            for (id, want) in wanted {
                if graph.node(id).is_none() {
                    continue;
                }
                if want < f32::MAX {
                    y = y.max(want);
                }
                placed.insert(id, Vec2::new(x, y));
                y += card_size(editor, id).y;
                bottom = bottom.max(y);
                y += DOWN;
            }
        }
        top = bottom + if framed { MARGIN } else { 0.0 } + DOWN * 2.0;
    }
    placed
}

/// The colour of the selected card at `index` in the selection, and
/// of what it gives in the preview.
pub fn picked(palette: &Palette, index: usize) -> Color {
    let colours = [
        palette.glow,
        palette.gold_3,
        palette.green,
        palette.agent,
        palette.particle,
        palette.glow_core,
    ];
    colours[index % colours.len()]
}

/// The colour of a wire or a dot of `value_type`.
fn tint(palette: &Palette, value_type: ValueType) -> Color {
    match value_type {
        ValueType::Number => palette.gold_3,
        ValueType::Integer => palette.gold_1,
        ValueType::Bool => palette.green,
        ValueType::Point
        | ValueType::Curve
        | ValueType::Path
        | ValueType::Vector3 => palette.agent,
        ValueType::Choice => palette.grey_1,
        ValueType::Transform => palette.glow,
        ValueType::Field => palette.particle,
        ValueType::Objects => palette.glow_core,
        ValueType::Vector2 => palette.primary_high,
        ValueType::Color => palette.red,
    }
}

/// The colour of the dot `dot`: its type's, or grey for the empty
/// row of the Outputs card.
fn dot_tint<K: EditorKey>(palette: &Palette, dot: &Dot<K>) -> Color {
    if dot.is_blank() {
        palette.grey_1
    } else {
        tint(palette, dot.value_type)
    }
}

/// Builds every card again when the nodes, the wires or how an input
/// is set change, and when the canvas is new.
fn build<K: EditorKey>(
    mut commands: Commands,
    editor: Res<Editor<K>>,
    // The theme loads after the first frames.
    If(theme): If<Res<Theme<HamletTheme>>>,
    canvas: Query<Entity, With<Canvas<K>>>,
    cards: Query<Entity, Or<(With<Card<K>>, With<Frame<K>>)>>,
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
                .observe(ask::<K>)
                .observe(received::<K>)
                .observe(pan_start::<K>)
                .observe(pan::<K>)
                .observe(gesture_end::<K>)
                .observe(zoom::<K>)
                .observe(deselect::<K>);
            let sheet = commands
                .spawn((
                    Sheet::<K>(PhantomData),
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
                Lasso::<K>(PhantomData),
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
    for at in 0..editor.sheet.groups.len() {
        let frame = commands
            .spawn((
                Frame::<K>(at, PhantomData),
                Node {
                    position_type: PositionType::Absolute,
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(px(BORDER)),
                    border_radius: BorderRadius::all(px(8.0)),
                    ..Node::default()
                },
                BackgroundColor(palette.glow.with_alpha(0.04)),
                BorderColor::all(palette.border),
                // Under the wires, which are under the cards.
                ZIndex(-2),
                Pickable::IGNORE,
                ChildOf(sheet),
            ))
            .id();
        let bar = commands
            .spawn((
                TitleBar::<K>(at, PhantomData),
                Node {
                    height: px(TITLE),
                    padding: UiRect::horizontal(px(6.0)),
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::top(px(8.0)),
                    // A title no wider than its group; as it grows
                    // when zoomed out, it still rises above the bar.
                    overflow: Overflow::clip_x(),
                    ..Node::default()
                },
                BackgroundColor(palette.border.with_alpha(0.5)),
                Hovered::default(),
                EntityCursor(SystemCursorIcon::Grab),
                ChildOf(frame),
            ))
            .observe(select_group::<K>)
            .observe(grab_group::<K>)
            .observe(drag_card::<K>)
            .observe(gesture_end::<K>)
            .observe(retitle::<K>)
            .id();
        // Its foot on the bar, so as it grows it grows up, off the
        // cards; never cut short or wrapped.
        commands.spawn((
            TitleText::<K>(at, PhantomData),
            Text::new(
                editor
                    .sheet
                    .groups
                    .get(at)
                    .map(|group| group.title.clone())
                    .unwrap_or_default(),
            ),
            TextFont::from_font_size(TITLE_FONT),
            TextColor(palette.gold_1),
            TextLayout::no_wrap(),
            Node {
                position_type: PositionType::Absolute,
                left: px(8.0),
                bottom: px(5.0),
                ..Node::default()
            },
            Pickable::IGNORE,
            ChildOf(bar),
        ));
    }
    for id in card_ids(&editor) {
        let Some((inputs, outputs)) = card_rows(&editor, id) else {
            continue;
        };
        let node = editor.sheet.graph.node(id);
        // A graph run as a node, a parameter and the outputs are
        // marked by their headers' colours.
        let (title, header_fill, title_colour) = match node {
            None => {
                ("Outputs".to_owned(), palette.gold_6, palette.gold_2)
            }
            Some(node) => match &node.does {
                Does::Kind(kind) => (
                    super::naming(&editor.registry, node)
                        .map_or_else(
                            || kind.clone(),
                            |(_, chosen)| chosen,
                        )
                        .replace('_', " "),
                    palette.code,
                    palette.gold_1,
                ),
                Does::Graph(graph) => (
                    graph.clone(),
                    palette.primary_low,
                    palette.glow_core,
                ),
                Does::Input(name) => (
                    name.replace('_', " "),
                    palette.green_deep,
                    palette.green,
                ),
            },
        };
        let card = commands
            .spawn((
                Card::<K>(id, PhantomData),
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
            .observe(select::<K>)
            .observe(open_on_double::<K>)
            .observe(grab::<K>)
            .observe(drag_card::<K>)
            .observe(gesture_end::<K>)
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
        let room = room_of(&editor, id);
        if room > 0.0 {
            commands.spawn((
                Room::<K>(id, PhantomData),
                Node {
                    height: px(room),
                    flex_shrink: 0.0,
                    ..Node::default()
                },
                ChildOf(card),
            ));
        }
        let specs = specs(&editor, id);
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
                        justify_content: if dot.is_output() {
                            JustifyContent::FlexEnd
                        } else {
                            JustifyContent::FlexStart
                        },
                        ..Node::default()
                    },
                    ChildOf(card),
                ))
                .id();
            if dot.is_on_outputs() && !dot.is_blank() {
                // An output's name is written on its row.
                let name = dot.port.name.clone();
                commands.queue(move |world: &mut World| {
                    let field = bevy_fynix::mount::<HamletTheme>(
                        world,
                        controls::output_name::<K>(name),
                    );
                    world
                        .entity_mut(field)
                        .insert(ChildOf(row))
                        .observe(hold_drag_start)
                        .observe(hold_drag)
                        .observe(hold_drag_end);
                });
            } else {
                let words = if dot.is_blank() {
                    "wire here".to_owned()
                } else {
                    dot.port.name.replace('_', " ")
                };
                commands.spawn((
                    Text::new(words),
                    TextFont::from_font_size(11.0),
                    TextColor(palette.grey_1),
                    Node {
                        flex_grow: if dot.is_output() {
                            0.0
                        } else {
                            1.0
                        },
                        ..Node::default()
                    },
                    Pickable::IGNORE,
                    ChildOf(row),
                ));
            }
            let spec =
                specs.iter().find(|spec| spec.name == dot.port.name);
            if let (false, Some(spec)) = (dot.is_output(), spec) {
                commands.entity(row).insert(InputRow::<K> {
                    node: id,
                    input: spec.clone(),
                    key: PhantomData,
                });
            }
            if let (false, false, Some(spec), Some(node)) =
                (dot.is_output(), wired, spec, node)
            {
                let spec = spec.clone();
                let setting = node.settings.get(&spec.name).cloned();
                commands.queue(move |world: &mut World| {
                    if let Some(control) = controls::control::<K>(
                        id,
                        spec,
                        setting.as_ref(),
                    ) {
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
            // The dot sits on the card's edge, half outside it, in
            // the middle of the larger area it is pointed at in.
            let edge = px(-DOT_HIT * 0.5 - BORDER);
            let area = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: px((ROW - DOT_HIT) * 0.5),
                        left: if dot.is_output() {
                            Val::Auto
                        } else {
                            edge
                        },
                        right: if dot.is_output() {
                            edge
                        } else {
                            Val::Auto
                        },
                        width: px(DOT_HIT),
                        height: px(DOT_HIT),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..Node::default()
                    },
                    Hovered::default(),
                    EntityCursor(SystemCursorIcon::Grab),
                    ChildOf(row),
                ))
                .id();
            let disc = commands
                .spawn((
                    Node {
                        width: px(DOT),
                        height: px(DOT),
                        border: UiRect::all(px(3.0)),
                        border_radius: BorderRadius::MAX,
                        ..Node::default()
                    },
                    BackgroundColor(dot_tint(palette, &dot)),
                    BorderColor::all(palette.input),
                    Pickable::IGNORE,
                    ChildOf(area),
                ))
                .id();
            let mut hit = commands.entity(area);
            hit.insert(Disc::<K>(disc, PhantomData));
            // A drag from either end holds a wire: a new one out of
            // an output, or an input's own taken off by its end.
            hit.observe(start_link::<K>)
                .observe(drag_link::<K>)
                .observe(gesture_end::<K>);
            if !dot.is_output() {
                hit.observe(link::<K>).observe(cut::<K>);
            }
            hit.insert(dot);
        }
    }
}

/// Moves the sheet and the cards and draws each wire; a wire's node
/// is kept and only moved or reshaped.
fn place<K: EditorKey>(
    mut commands: Commands,
    editor: Res<Editor<K>>,
    sight: Res<Sight<K>>,
    gesture: Res<Held<K>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    mut sheets: Query<(Entity, &mut UiTransform), With<Sheet<K>>>,
    mut cards: Query<(&Card<K>, &mut Node), Without<Drawn<K>>>,
    mut frames: Query<
        (&Frame<K>, &mut Node),
        (Without<Card<K>>, Without<Drawn<K>>),
    >,
    mut drawn: Query<
        (Entity, &Drawn<K>, &MaterialNode<WireMaterial>, &mut Node),
        (Without<Card<K>>, Without<Frame<K>>),
    >,
    mut materials: ResMut<Assets<WireMaterial>>,
    added: Query<(), Added<Card<K>>>,
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
    for (frame, mut node) in &mut frames {
        let Some((low, high)) = frame_bounds(&editor, frame.0) else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        node.left = px(low.x);
        node.top = px(low.y);
        node.width = px(high.x - low.x);
        node.height = px(high.y - low.y);
    }
    let palette = &theme.0.palette;
    let find = |port: &Port, side: Side| port_at(&editor, port, side);
    let lifted = match &**gesture {
        Gesture::Link(link) => link.lifted.as_ref(),
        _ => None,
    };
    let mut wanted = Vec::new();
    for wire in drawn_wires(&editor) {
        if lifted == Some(&wire) {
            continue;
        }
        if let (Some((start, value_type)), Some((end, _))) = (
            find(&wire.from, Side::Output),
            find(&wire.to, Side::Input),
        ) {
            let shape =
                Shape::of(start, end, tint(palette, value_type));
            wanted.push((Some(wire), shape));
        }
    }
    if let Gesture::Link(link) = &**gesture {
        let shape = Shape::of(
            link.start,
            link.end(),
            tint(palette, link.value_type),
        );
        wanted.push((None, shape));
    }
    for (entity, Drawn(key, _), material, mut node) in &mut drawn {
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
            .is_some_and(|now| !now.is_same_as(&shape.material))
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
            Drawn::<K>(key, PhantomData),
            node,
            MaterialNode(materials.add(shape.material)),
            Pickable::IGNORE,
            ZIndex(-1),
            ChildOf(sheet),
        ));
    }
}

/// Colours what the pointer is on, the selected cards, and, while a
/// wire is dragged, the inputs it can go to.
fn paint<K: EditorKey>(
    editor: Res<Editor<K>>,
    gesture: Res<Held<K>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    mut cards: Query<
        (&Card<K>, &Hovered, &mut BorderColor),
        Without<Dot<K>>,
    >,
    dots: Query<(&Dot<K>, &Hovered, &Disc<K>)>,
    mut discs: Query<
        (&mut BorderColor, &mut BackgroundColor),
        (Without<Card<K>>, Without<TitleBar<K>>),
    >,
    mut bars: Query<
        (&TitleBar<K>, &Hovered, &mut BackgroundColor),
        (Without<Card<K>>, Without<Dot<K>>),
    >,
    changed: Query<(), Or<(Changed<Hovered>, Added<Hovered>)>>,
) {
    if !editor.is_changed()
        && !gesture.is_changed()
        && changed.is_empty()
    {
        return;
    }
    let palette = &theme.0.palette;
    // The cards that did not run in the last run are marked as
    // errors.
    let failed = editor
        .errors
        .iter()
        .filter(|found| found.graph == editor.name)
        .filter_map(|found| found.error.node())
        .collect::<BTreeSet<_>>();
    for (card, hovered, mut border) in &mut cards {
        let at =
            editor.selected.iter().position(|node| *node == card.0);
        let colour = if let Some(at) = at {
            picked(palette, at)
        } else if failed.contains(&card.0) {
            palette.danger
        } else if hovered.get() {
            palette.gold_5
        } else {
            palette.border
        };
        *border = BorderColor::all(colour);
    }
    for (bar, hovered, mut fill) in &mut bars {
        let chosen =
            editor.sheet.groups.get(bar.0).is_some_and(|group| {
                !group.nodes.is_empty()
                    && group.nodes == editor.selected
            });
        fill.0 = if chosen {
            palette.glow.with_alpha(0.35)
        } else if hovered.get() {
            palette.gold_5.with_alpha(0.5)
        } else {
            palette.border.with_alpha(0.5)
        };
    }
    let linking = match &**gesture {
        Gesture::Link(link) => Some((&link.from, link.value_type)),
        _ => None,
    };
    // A wire taken off an input no longer counts for it.
    let lifted = match &**gesture {
        Gesture::Link(link) => link.lifted.clone(),
        _ => None,
    };
    let wires = drawn_wires(&editor)
        .into_iter()
        .filter(|wire| lifted.as_ref() != Some(wire))
        .collect::<Vec<_>>();
    let is_wired = |port: &Port| {
        wires
            .iter()
            .any(|wire| wire.from == *port || wire.to == *port)
    };
    for (dot, hovered, disc) in &dots {
        let Ok((mut border, mut fill)) = discs.get_mut(disc.0) else {
            continue;
        };
        let open = match linking {
            Some((from, value_type)) => {
                (!dot.is_output() && dot.takes(value_type))
                    || dot.port == *from
            }
            None => true,
        };
        let tint = dot_tint(palette, dot);
        let colour = if open { tint } else { tint.with_alpha(0.2) };
        // Filled where a wire is joined, a ring of its colour where
        // none is, so a wire passing by is not taken for one.
        let (inside, rim) = if is_wired(&dot.port) {
            (colour, palette.input)
        } else {
            (palette.input, colour)
        };
        fill.0 = inside;
        *border = BorderColor::all(if hovered.get() && open {
            palette.glow_core
        } else {
            rim
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
fn gesture_end<K: EditorKey>(
    mut end: On<PointerDragEnd>,
    mut gesture: ResMut<Held<K>>,
    mut editor: ResMut<Editor<K>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    end.propagate(false);
    if let Gesture::Cards(cards) = &**gesture {
        let moved = cards
            .iter()
            .filter(|(node, from)| card_at(&editor, *node) != *from)
            .copied()
            .collect::<Vec<_>>();
        if !moved.is_empty() {
            editor.dropped(moved);
        }
    }
    // A wire taken off an input, or off a row of the Outputs card,
    // and let go anywhere but on another is cut; one let go where it
    // was taken stays.
    if let Gesture::Link(link) = &**gesture
        && let Some(lifted) = &link.lifted
        && link.gone.length() > PUT_BACK
    {
        editor.apply(unwire(lifted.clone()));
    }
    **gesture = match **gesture {
        Gesture::Lasso(..) | Gesture::Link(..) => Gesture::Ended,
        _ => Gesture::None,
    };
    cursor.0 = None;
}

/// How far, on the sheet, a wire taken off an input may be moved and
/// still be put back as it was when let go.
const PUT_BACK: f32 = 6.0;

/// Forgets a drag let go of last frame, once its release's click has
/// been seen.
fn forget_ended<K: EditorKey>(mut gesture: ResMut<Held<K>>) {
    if matches!(**gesture, Gesture::Ended) {
        **gesture = Gesture::None;
    }
}

/// Selects a card alone, or with Shift adds it to the selection or
/// takes it out.
fn select<K: EditorKey>(
    mut click: On<PointerClick>,
    cards: Query<&Card<K>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut editor: ResMut<Editor<K>>,
) {
    click.propagate(false);
    // A right click opens a menu on the selection as it is.
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(card) = cards.get(click.entity) else {
        return;
    };
    if is_shift_held(keys.as_deref()) {
        if !editor.selected.remove(&card.0) {
            editor.selected.insert(card.0);
        }
    } else if editor.selected != BTreeSet::from([card.0]) {
        editor.selected = BTreeSet::from([card.0]);
    }
}

/// Selects a group's cards, or with Shift adds them to the selection.
fn select_group<K: EditorKey>(
    mut click: On<PointerClick>,
    bars: Query<&TitleBar<K>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut editor: ResMut<Editor<K>>,
) {
    click.propagate(false);
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(bar) = bars.get(click.entity) else {
        return;
    };
    let Some(nodes) = editor
        .sheet
        .groups
        .get(bar.0)
        .map(|group| group.nodes.clone())
    else {
        return;
    };
    if is_shift_held(keys.as_deref()) {
        editor.selected.extend(nodes);
    } else if editor.selected != nodes {
        editor.selected = nodes;
    }
}

/// Drags a group's cards by its title bar.
fn grab_group<K: EditorKey>(
    mut start: On<PointerDragStart>,
    bars: Query<&TitleBar<K>>,
    mut gesture: ResMut<Held<K>>,
    mut editor: ResMut<Editor<K>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    start.propagate(false);
    if start.button != PointerButton::Primary {
        return;
    }
    let Ok(bar) = bars.get(start.entity) else {
        return;
    };
    let Some(nodes) = editor
        .sheet
        .groups
        .get(bar.0)
        .map(|group| group.nodes.clone())
    else {
        return;
    };
    **gesture = Gesture::Cards(
        nodes
            .iter()
            .map(|node| (*node, card_at(&editor, *node)))
            .collect(),
    );
    editor.selected = nodes;
    cursor.0 = Some(SystemCursorIcon::Grabbing);
}

fn is_shift_held(keys: Option<&ButtonInput<KeyCode>>) -> bool {
    keys.is_some_and(|keys| {
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    })
}

/// Swaps a group's title for a text field on a double press of its
/// bar.
fn retitle<K: EditorKey>(
    press: On<PointerPress>,
    bars: Query<(&TitleBar<K>, &Children)>,
    texts: Query<(), With<TitleText<K>>>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary || press.count != 2 {
        return;
    }
    let Ok((bar, children)) = bars.get(press.entity) else {
        return;
    };
    let Some(text) = children
        .iter()
        .find(|child| texts.contains(**child))
        .copied()
    else {
        return;
    };
    let (at, bar) = (bar.0, press.entity);
    commands.queue(move |world: &mut World| {
        if world.contains_resource::<Retitling<K>>() {
            return;
        }
        let field = bevy_fynix::mount::<HamletTheme>(
            world,
            controls::title::<K>(at),
        );
        world
            .entity_mut(field)
            .insert(ChildOf(bar))
            .observe(hold_drag_start)
            .observe(hold_drag)
            .observe(hold_drag_end);
        if let Some(mut node) = world.get_mut::<Node>(text) {
            node.display = Display::None;
        }
        world.insert_resource(Retitling::<K> {
            text,
            field,
            focused: false,
            key: PhantomData,
        });
    });
}

/// Focuses the title's field once it is built, and puts the title
/// back when the field loses focus or Enter or Escape is pressed.
fn retitled<K: EditorKey>(world: &mut World) {
    let Some(&Retitling::<K> {
        text,
        field,
        focused,
        ..
    }) = world.get_resource::<Retitling<K>>()
    else {
        return;
    };
    let input = editable_under(world, field);
    if !focused {
        if let Some(input) = input {
            world
                .resource_mut::<InputFocus>()
                .set(input, FocusCause::Pressed);
            if let Some(mut edit) =
                world.get_mut::<EditableText>(input)
            {
                edit.queue_edit(TextEdit::SelectAll);
            }
            world.resource_mut::<Retitling<K>>().focused = true;
        }
        return;
    }
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let done = keys.any_just_pressed([
        KeyCode::Enter,
        KeyCode::NumpadEnter,
        KeyCode::Escape,
    ]);
    let held = input.is_some()
        && world.resource::<InputFocus>().get() == input;
    if held && !done {
        return;
    }
    if held {
        world.resource_mut::<InputFocus>().clear();
    }
    if let Ok(field) = world.get_entity_mut(field) {
        field.despawn();
    }
    if let Some(mut node) = world.get_mut::<Node>(text) {
        node.display = Display::Flex;
    }
    world.remove_resource::<Retitling<K>>();
}

/// The text field under `root`, if it is built.
fn editable_under(world: &mut World, root: Entity) -> Option<Entity> {
    let mut found = None;
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world.get::<EditableText>(entity).is_some() {
            found = Some(entity);
            break;
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    found
}

/// Keeps each group's title what the group says, and as large on
/// screen however far the sheet is zoomed out.
fn titles<K: EditorKey>(
    editor: Res<Editor<K>>,
    sight: Res<Sight<K>>,
    mut texts: Query<(&TitleText<K>, &mut Text, &mut TextFont)>,
    added: Query<(), Added<TitleText<K>>>,
) {
    if !editor.is_changed() && !sight.is_changed() && added.is_empty()
    {
        return;
    }
    let size = TITLE_FONT / sight.zoom.min(1.0);
    for (title, mut text, mut font) in &mut texts {
        if let Some(group) = editor.sheet.groups.get(title.0)
            && text.0 != group.title
        {
            text.0.clone_from(&group.title);
        }
        if font.font_size != FontSize::Px(size) {
            font.font_size = FontSize::Px(size);
        }
    }
}

/// Opens the graph a card runs on a double press.
fn open_on_double<K: EditorKey>(
    press: On<PointerPress>,
    cards: Query<&Card<K>>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary || press.count != 2 {
        return;
    }
    if let Ok(card) = cards.get(press.entity) {
        let node = card.0;
        commands.queue(move |world: &mut World| {
            super::menu::open_graph::<K>(world, node);
        });
    }
}

fn deselect<K: EditorKey>(
    click: On<PointerClick>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    gesture: Res<Held<K>>,
    mut editor: ResMut<Editor<K>>,
) {
    // The release that ends a box clicks the canvas too.
    let boxing =
        matches!(**gesture, Gesture::Lasso(..) | Gesture::Ended);
    if !boxing
        && click.entity == click.original_event_target()
        && click.button == PointerButton::Primary
        && !is_shift_held(keys.as_deref())
        && !editor.selected.is_empty()
    {
        editor.selected.clear();
    }
}

fn grab<K: EditorKey>(
    mut start: On<PointerDragStart>,
    cards: Query<&Card<K>>,
    mut gesture: ResMut<Held<K>>,
    mut editor: ResMut<Editor<K>>,
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
        **gesture = Gesture::Cards(
            editor
                .selected
                .iter()
                .map(|node| (*node, card_at(&editor, *node)))
                .collect(),
        );
        cursor.0 = Some(SystemCursorIcon::Grabbing);
    }
}

fn drag_card<K: EditorKey>(
    mut drag: On<PointerDrag>,
    gesture: Res<Held<K>>,
    sight: Res<Sight<K>>,
    mut editor: ResMut<Editor<K>>,
) {
    drag.propagate(false);
    if let Gesture::Cards(cards) = &**gesture {
        for (node, from) in cards {
            editor.drag(*node, *from + drag.distance / sight.zoom);
        }
    }
}

/// Draws a box to select by on a drag of the bare canvas, adding to
/// the selection with Shift, or pans on a drag with the middle
/// button. The right button pans in [`right_drag`].
fn pan_start<K: EditorKey>(
    start: On<PointerDragStart>,
    canvas: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<Canvas<K>>,
    >,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    sight: Res<Sight<K>>,
    editor: Res<Editor<K>>,
    mut gesture: ResMut<Held<K>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    match start.button {
        PointerButton::Primary => {
            if start.entity != start.original_event_target() {
                return;
            }
            let Ok((node, transform)) = canvas.get(start.entity)
            else {
                return;
            };
            let at = (start.pointer.position
                - corner(node, transform)
                - sight.pan)
                / sight.zoom;
            let kept = if is_shift_held(keys.as_deref()) {
                editor.selected.clone()
            } else {
                BTreeSet::new()
            };
            **gesture = Gesture::Lasso(at, at, kept);
        }
        PointerButton::Middle => {
            **gesture = Gesture::Pan;
            cursor.0 = Some(SystemCursorIcon::Move);
        }
        PointerButton::Secondary => {}
    }
}

fn pan<K: EditorKey>(
    drag: On<PointerDrag>,
    mut gesture: ResMut<Held<K>>,
    mut editor: ResMut<Editor<K>>,
    mut sight: ResMut<Sight<K>>,
) {
    match &mut **gesture {
        Gesture::Pan => sight.pan += drag.delta,
        Gesture::Lasso(from, to, before) => {
            *to = *from + drag.distance / sight.zoom;
            let (low, high) = (from.min(*to), from.max(*to));
            let mut selected = before.clone();
            for id in card_ids(&editor) {
                let at = card_at(&editor, id);
                let far = at + card_size(&editor, id);
                if at.cmplt(high).all() && far.cmpgt(low).all() {
                    selected.insert(id);
                }
            }
            if editor.selected != selected {
                editor.selected = selected;
            }
        }
        _ => {}
    }
}

/// Lets Escape take back a drag: a wire is let go unwired, cards go
/// back where they were, and a box gives back the selection it began
/// with. The rest of the drag then does nothing.
fn cancel<K: EditorKey>(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut gesture: ResMut<Held<K>>,
    mut editor: ResMut<Editor<K>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if matches!(**gesture, Gesture::None)
        || !keys
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
    {
        return;
    }
    match std::mem::take(&mut **gesture) {
        Gesture::Cards(cards) => {
            for (node, from) in cards {
                editor.drag(node, from);
            }
        }
        Gesture::Lasso(_, _, before) => editor.selected = before,
        _ => {}
    }
    cursor.0 = None;
}

/// Sets [`Dragging`] while this canvas drags a wire.
fn faint<K: EditorKey>(
    gesture: Res<Held<K>>,
    mut dragging: ResMut<Dragging>,
    mut was: Local<bool>,
) {
    // Only a change of this canvas's own is passed on, as another
    // canvas may be the one linking.
    let linking = matches!(**gesture, Gesture::Link(..));
    if *was != linking {
        *was = linking;
        dragging.0 = linking;
    }
}

/// Shows the box of a drag that selects where it is, and hides it
/// after.
fn lasso<K: EditorKey>(
    gesture: Res<Held<K>>,
    mut boxes: Query<&mut Node, With<Lasso<K>>>,
) {
    if !gesture.is_changed() {
        return;
    }
    for mut node in &mut boxes {
        match &**gesture {
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

/// Asks for a zoom about the pointer, which [`ease_zoom`] goes to.
fn zoom<K: EditorKey>(
    scroll: On<PointerScroll>,
    canvas: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<Canvas<K>>,
    >,
    mut sight: ResMut<Sight<K>>,
) {
    let Ok((node, transform)) = canvas.get(scroll.entity) else {
        return;
    };
    let notch = match scroll.unit {
        MouseScrollUnit::Line => scroll.y * 0.12,
        MouseScrollUnit::Pixel => scroll.y * 0.002,
    };
    let from = sight.goal.map_or(sight.zoom, |goal| goal.zoom);
    sight.goal = Some(ZoomGoal {
        zoom: (from * notch.exp()).clamp(LEAST_ZOOM, MOST_ZOOM),
        about: scroll.pointer.position - corner(node, transform),
    });
}

/// Eases the zoom toward the one asked for, keeping the point it is
/// about where it is on the canvas.
fn ease_zoom<K: EditorKey>(
    time: Res<Time>,
    mut sight: ResMut<Sight<K>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let Some(goal) = sight.goal else {
        return;
    };
    let step = 1.0 - (-ZOOM_EASE * time.delta_secs()).exp();
    // In equal ratios, so a zoom in and a zoom out ease alike.
    let zoom = if (goal.zoom / sight.zoom - 1.0).abs() < 1e-3 {
        goal.zoom
    } else {
        sight.zoom * (goal.zoom / sight.zoom).powf(step)
    };
    let on_sheet = (goal.about - sight.pan) / sight.zoom;
    sight.pan = goal.about - on_sheet * zoom;
    sight.zoom = zoom;
    if zoom == goal.zoom {
        sight.goal = None;
    } else {
        redraw.write(RequestRedraw);
    }
}

/// Pans the sheet while the right button is dragged from a press on
/// the canvas, and opens the press's menu where it is let go without
/// having moved.
fn right_drag<K: EditorKey>(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut right: ResMut<RightPress<K>>,
    mut sight: ResMut<Sight<K>>,
    mut cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    let right = right.bypass_change_detection();
    let Some(held) = &mut right.held else {
        return;
    };
    if buttons.pressed(MouseButton::Right) {
        let Some(at) =
            windows.iter().find_map(Window::cursor_position)
        else {
            return;
        };
        if !held.panning && at.distance(held.from) > PAN_SLOP {
            held.panning = true;
            cursor.0 = Some(SystemCursorIcon::Move);
        }
        if held.panning && at != held.last {
            sight.pan += at - held.last;
        }
        held.last = at;
        return;
    }
    let Some(held) = right.held.take() else {
        return;
    };
    if held.panning {
        cursor.0 = None;
    } else {
        // Through `open`, which clears what was searched for.
        commands.queue(move |world: &mut World| {
            super::menu::open::<K>(world, held.menu);
        });
    }
}

/// Holds a right press on the canvas with the menu it opens if let go
/// where it is: the menu of the card or row it is on, or the list of
/// kinds on the bare canvas. [`right_drag`] opens it or pans.
fn ask<K: EditorKey>(
    press: On<PointerPress>,
    cards: Query<&Card<K>>,
    rows: Query<&InputRow<K>>,
    bars: Query<&TitleBar<K>>,
    parents: Query<&ChildOf>,
    canvas: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<Canvas<K>>,
    >,
    sight: Res<Sight<K>>,
    mut editor: ResMut<Editor<K>>,
    mut right: ResMut<RightPress<K>>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let at = press.pointer.position;
    let hit = press.original_event_target();
    let up =
        || std::iter::once(hit).chain(parents.iter_ancestors(hit));
    let row = up().find_map(|entity| rows.get(entity).ok());
    // The Outputs card has no actions of its own, so a press on it
    // asks what to create, as on the bare canvas.
    let card = up()
        .find_map(|entity| cards.get(entity).ok())
        .filter(|card| card.0 != OUTPUTS);
    let bar = up().find_map(|entity| bars.get(entity).ok());
    let menu = match (row, card, bar) {
        (Some(row), ..) => {
            if !editor.selected.contains(&row.node) {
                editor.selected = BTreeSet::from([row.node]);
            }
            Which::Input {
                node: row.node,
                input: row.input.clone(),
                at,
            }
        }
        (None, Some(card), _) => {
            if !editor.selected.contains(&card.0) {
                editor.selected = BTreeSet::from([card.0]);
            }
            Which::Card { node: card.0, at }
        }
        (None, None, Some(bar)) => Which::Group { group: bar.0, at },
        (None, None, None) => {
            let Ok((node, transform)) = canvas.get(press.entity)
            else {
                return;
            };
            let on_sheet = (at - corner(node, transform) - sight.pan)
                / sight.zoom;
            Which::Create { at, on_sheet }
        }
    };
    right.held = Some(RightHeld {
        menu,
        from: at,
        last: at,
        panning: false,
    });
}

fn start_link<K: EditorKey>(
    mut start: On<PointerDragStart>,
    dots: Query<&Dot<K>>,
    editor: Res<Editor<K>>,
    mut gesture: ResMut<Held<K>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    start.propagate(false);
    if start.button != PointerButton::Primary {
        return;
    }
    let Ok(dot) = dots.get(start.entity) else {
        return;
    };
    let link = if dot.is_output() {
        let Some((at, value_type)) =
            port_at(&editor, &dot.port, Side::Output)
        else {
            return;
        };
        Link {
            from: dot.port.clone(),
            value_type,
            start: at,
            grabbed: at,
            gone: Vec2::ZERO,
            lifted: None,
        }
    } else {
        // An input's wire is taken off by its end, still out of the
        // same output, to be dropped on another input or let go.
        let Some(wire) = drawn_wires(&editor)
            .into_iter()
            .find(|wire| wire.to == dot.port)
        else {
            return;
        };
        let (Some((start, value_type)), Some((grabbed, _))) = (
            port_at(&editor, &wire.from, Side::Output),
            port_at(&editor, &dot.port, Side::Input),
        ) else {
            return;
        };
        Link {
            from: wire.from.clone(),
            value_type,
            start,
            grabbed,
            gone: Vec2::ZERO,
            lifted: Some(wire),
        }
    };
    **gesture = Gesture::Link(link);
    // The wire is held, as a card is.
    cursor.0 = Some(SystemCursorIcon::Grabbing);
}

/// Where on the sheet the dot of `port` on `side` is, and its type.
fn port_at<K: EditorKey>(
    editor: &Editor<K>,
    port: &Port,
    side: Side,
) -> Option<(Vec2, ValueType)> {
    let (inputs, outputs) = card_rows(editor, port.node)?;
    let (list, before) = match side {
        Side::Input => (&inputs, 0),
        Side::Output => (&outputs, inputs.len()),
    };
    let (row, dot) =
        list.iter().enumerate().find(|(_, dot)| dot.port == *port)?;
    Some((
        dot_at(
            card_at(editor, port.node),
            room_of(editor, port.node),
            before + row,
            side,
        ),
        dot.value_type,
    ))
}

fn drag_link<K: EditorKey>(
    mut drag: On<PointerDrag>,
    sight: Res<Sight<K>>,
    mut gesture: ResMut<Held<K>>,
) {
    drag.propagate(false);
    if let Gesture::Link(link) = &mut **gesture {
        link.gone = drag.distance / sight.zoom;
    }
}

/// A library tile dropped on the canvas of the editor `K`, `at` on
/// its sheet. What it adds to the graph, if anything, is up to
/// whoever observes it.
#[derive(Event)]
pub struct Dropped<K: EditorKey> {
    pub item: Item,
    pub at: Vec2,
    key: PhantomData<K>,
}

/// Tells what a library tile dropped on the canvas is, and where. The
/// drag may have begun on something inside the tile, such as its
/// picture.
fn received<K: EditorKey>(
    drop: On<PointerDragDrop>,
    mut commands: Commands,
) {
    let dropped = drop.dropped;
    commands.queue(move |world: &mut World| {
        let Some(item) = tile_of(world, dropped) else {
            return;
        };
        let at = world
            .query::<&Window>()
            .iter(world)
            .find_map(Window::cursor_position)
            .unwrap_or_default();
        let Some(at) = on_sheet::<K>(world, at) else {
            return;
        };
        world.trigger(Dropped::<K> {
            item,
            at,
            key: PhantomData,
        });
    });
}

/// Wires the output of the wire dropped on an input's dot to it, in
/// place of any wire it had and of the wire it was taken off, as one
/// step. On a row of the Outputs card, the output of the wire is
/// given to the graph under that row's name, or a new one on the
/// empty row.
fn link<K: EditorKey>(
    mut drop: On<PointerDragDrop>,
    dots: Query<&Dot<K>>,
    mut gesture: ResMut<Held<K>>,
    mut editor: ResMut<Editor<K>>,
) {
    drop.propagate(false);
    let (Ok(to), Gesture::Link(link)) =
        (dots.get(drop.entity), &**gesture)
    else {
        return;
    };
    if to.is_output() || !to.takes(link.value_type) {
        return;
    }
    let wire = Wire {
        from: link.from.clone(),
        to: to.port.clone(),
    };
    let lifted = link.lifted.clone();
    let (from, value_type) = (link.from.clone(), link.value_type);
    if lifted.as_ref() == Some(&wire)
        || (to.is_on_outputs()
            && lifted
                .as_ref()
                .is_some_and(|lifted| lifted.to.node == OUTPUTS))
    {
        // Put back where it was taken from.
        **gesture = Gesture::None;
        return;
    }
    let mut edits = Vec::new();
    if to.is_on_outputs() {
        edits.extend(lifted.map(unwire));
        let named = |name: String| GraphOutput {
            name,
            value_type,
            from: from.clone(),
        };
        edits.push(if to.is_blank() {
            Edit::AddOutput(named(
                editor.free_output_name(&from.name),
            ))
        } else {
            Edit::ChangeOutput {
                name: to.port.name.clone(),
                output: named(to.port.name.clone()),
            }
        });
    } else {
        edits.extend(
            editor
                .sheet
                .graph
                .wires
                .iter()
                .filter(|other| {
                    other.to == to.port
                        || lifted.as_ref() == Some(*other)
                })
                .cloned()
                .map(Edit::Disconnect),
        );
        // A wire off the Outputs card is not one of the graph's.
        edits.extend(
            lifted
                .filter(|lifted| lifted.to.node == OUTPUTS)
                .map(unwire),
        );
        edits.push(Edit::Connect(wire));
    }
    editor.apply(Edit::Batch(edits));
    // Landed, so letting go does not cut it.
    **gesture = Gesture::None;
}

/// Cuts the wire into an input whose dot is clicked, or removes the
/// output whose row on the Outputs card is.
fn cut<K: EditorKey>(
    mut click: On<PointerClick>,
    dots: Query<&Dot<K>>,
    gesture: Res<Held<K>>,
    mut editor: ResMut<Editor<K>>,
) {
    click.propagate(false);
    // The release of a wire dragged off this dot clicks it too.
    if matches!(**gesture, Gesture::Link(..) | Gesture::Ended) {
        return;
    }
    let Ok(dot) = dots.get(click.entity) else {
        return;
    };
    let wires = drawn_wires(&editor)
        .into_iter()
        .filter(|wire| wire.to == dot.port)
        .map(unwire)
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
