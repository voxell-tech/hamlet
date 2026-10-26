//! The canvas: each node a card, each wire a curve of short segments,
//! placed from the sheet's layout. Cards are dragged to move them, an
//! output's dot is dragged onto an input's to wire them, and an
//! input's dot is clicked to cut its wire.

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, If, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Rot2, Vec2};
use bevy::picking::Pickable;
use bevy::picking::events::{
    PointerClick, PointerDrag, PointerDragDrop, PointerDragEnd,
    PointerDragStart,
};
use bevy::picking::pointer::PointerButton;
use bevy::text::{TextColor, TextFont};
use bevy::ui::widget::Text;
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius,
    ComputedNode, FlexDirection, JustifyContent, Node, Overflow,
    PositionType, UiRect, UiTransform, Val, ZIndex, px,
};
use bevy_fynix::Theme;
use bevy_fynix::views::{BehaviorExt, frame};
use hamlet_graph::{
    Constant, Does, Edit, NodeId, Port, Ruleset, Setting, Spread,
    ValueType, Wire, ports,
};

use super::Editor;
use crate::looks::Fills;
use crate::theme::{HamletTheme, Palette};
use crate::{View, with_cx};

const WIDTH: f32 = 176.0;
const HEADER: f32 = 26.0;
const ROW: f32 = 20.0;
const PAD: f32 = 4.0;
const BORDER: f32 = 1.0;
const DOT: f32 = 10.0;
const WIRE: f32 = 2.0;
/// Straight pieces a wire is drawn with.
const PIECES: usize = 18;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Pan>()
        .init_resource::<Linking>()
        .add_systems(Update, (build, place).chain());
}

/// Marks the frame the canvas is drawn in.
#[derive(Component)]
pub(super) struct Canvas;

/// A node's card.
#[derive(Component)]
struct Card(NodeId);

/// A piece of a drawn wire.
#[derive(Component)]
struct Piece;

/// A port's dot on a card.
#[derive(Component, Clone)]
struct Dot {
    port: Port,
    output: bool,
    value_type: ValueType,
}

/// How far the canvas is panned.
#[derive(Resource, Default)]
struct Pan(Vec2);

/// A wire being dragged out of an output: its port, where it starts
/// and how far the pointer has gone.
#[derive(Resource, Default)]
struct Linking(Option<(Port, ValueType, Vec2, Vec2)>);

/// Where a card was when its drag began.
#[derive(Component)]
struct Grabbed(Vec2);

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

/// The middle of the canvas in the sheet's layout, for a new card.
pub(super) fn middle(world: &mut World) -> Vec2 {
    let pan = world.resource::<Pan>().0;
    let size = world
        .query_filtered::<&ComputedNode, With<Canvas>>()
        .iter(world)
        .next()
        .map_or(Vec2::splat(400.0), |node| {
            node.size() * node.inverse_scale_factor()
        });
    size * 0.5 - pan - Vec2::new(WIDTH * 0.5, HEADER)
}

/// The inputs and outputs of each node, in the order drawn.
fn rows(
    editor: &Editor,
    node: &hamlet_graph::Node,
) -> (Vec<Dot>, Vec<Dot>) {
    let Ok((inputs, outputs)) =
        ports(&editor.registry, &Ruleset::default(), node)
    else {
        return (Vec::new(), Vec::new());
    };
    let dot = |output: bool| {
        move |info: hamlet_graph::PortInfo| Dot {
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

/// Where a port's dot is, from its card's place and its row.
fn dot_at(card: Vec2, row: usize, output: bool) -> Vec2 {
    Vec2::new(
        card.x + if output { WIDTH } else { 0.0 },
        card.y + BORDER + HEADER + PAD + row as f32 * ROW + ROW * 0.5,
    )
}

fn card_at(editor: &Editor, pan: Vec2, node: NodeId) -> Vec2 {
    editor.sheet.layout.get(&node).copied().unwrap_or_default() + pan
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

/// A few words for how an unwired input is set.
fn summary(setting: Option<&Setting>) -> String {
    let number = |n: f32| {
        let text = format!("{n:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    match setting {
        None => String::new(),
        Some(Setting::Fixed(Constant::Number(n))) => number(*n),
        Some(Setting::Fixed(Constant::Bool(on))) => {
            if *on { "on" } else { "off" }.to_owned()
        }
        Some(Setting::Fixed(Constant::Choice(choice))) => {
            choice.replace('_', " ")
        }
        Some(Setting::Fixed(Constant::Point(p))) => {
            format!(
                "{}, {}, {}",
                number(p.x),
                number(p.y),
                number(p.z)
            )
        }
        Some(Setting::Random(roll)) => match &roll.spread {
            Spread::Range(low, high) => {
                format!("{} to {}", number(*low), number(*high))
            }
            Spread::Chance(chance) => {
                format!("{}% chance", number(chance * 100.0))
            }
            Spread::Weights(_) => "any".to_owned(),
        },
        Some(Setting::FromParent(name)) => format!("from {name}"),
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
    mut built: Local<Option<(Entity, u64)>>,
) {
    let Some(canvas) = canvas.iter().next() else {
        return;
    };
    if *built == Some((canvas, editor.shape)) {
        return;
    }
    if built.is_none_or(|(was, _)| was != canvas) {
        commands.entity(canvas).observe(pan).observe(deselect);
    }
    *built = Some((canvas, editor.shape));
    for card in &cards {
        commands.entity(card).despawn();
    }
    let palette = &theme.0.palette;
    for node in &editor.sheet.graph.nodes {
        let (inputs, outputs) = rows(&editor, node);
        let title = match &node.does {
            Does::Kind(kind) => kind.replace('_', " "),
            Does::Graph(graph) => graph.clone(),
        };
        let wired = |port: &Port| {
            editor
                .sheet
                .graph
                .wires
                .iter()
                .any(|wire| wire.to == *port)
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
                ChildOf(canvas),
            ))
            .observe(select)
            .observe(grab)
            .observe(drag_card)
            .observe(drop_card)
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
                BackgroundColor(palette.plate),
                ChildOf(card),
            ))
            .id();
        commands.spawn((
            Text::new(title),
            TextFont::from_font_size(13.0),
            TextColor(palette.brass_1),
            Pickable::IGNORE,
            ChildOf(header),
        ));
        for dot in inputs.into_iter().chain(outputs) {
            let note = if dot.output || wired(&dot.port) {
                String::new()
            } else {
                summary(node.settings.get(&dot.port.name))
            };
            let row = commands
                .spawn((
                    Node {
                        height: px(ROW),
                        padding: UiRect::horizontal(px(10.0)),
                        align_items: AlignItems::Center,
                        justify_content: if dot.output {
                            JustifyContent::FlexEnd
                        } else {
                            JustifyContent::SpaceBetween
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
                Pickable::IGNORE,
                ChildOf(row),
            ));
            if !note.is_empty() {
                commands.spawn((
                    Text::new(note),
                    TextFont::from_font_size(11.0),
                    TextColor(palette.brass_4),
                    Pickable::IGNORE,
                    ChildOf(row),
                ));
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
                ChildOf(row),
            ));
            if dot.output {
                spot.observe(start_link)
                    .observe(drag_link)
                    .observe(end_link);
            } else {
                spot.observe(link).observe(cut);
            }
            spot.insert(dot);
        }
    }
}

/// Places every card from the layout and draws every wire again.
fn place(
    mut commands: Commands,
    editor: Res<Editor>,
    pan: Res<Pan>,
    linking: Res<Linking>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    canvas: Query<Entity, With<Canvas>>,
    mut cards: Query<(&Card, &mut Node, &mut BorderColor)>,
    pieces: Query<Entity, With<Piece>>,
    added: Query<(), bevy::ecs::query::Added<Card>>,
) {
    let Some(canvas) = canvas.iter().next() else {
        return;
    };
    if !editor.is_changed()
        && !pan.is_changed()
        && !linking.is_changed()
        && added.is_empty()
    {
        return;
    }
    let palette = &theme.0.palette;
    for (card, mut node, mut border) in &mut cards {
        let at = card_at(&editor, pan.0, card.0);
        node.left = px(at.x);
        node.top = px(at.y);
        let colour = if editor.selected == Some(card.0) {
            palette.lamp
        } else {
            palette.seam
        };
        *border = BorderColor::all(colour);
    }
    for piece in &pieces {
        commands.entity(piece).despawn();
    }
    let graph = &editor.sheet.graph;
    let find = |port: &Port, output: bool| {
        let node = graph.node(port.node)?;
        let (inputs, outputs) = rows(&editor, node);
        let row = if output {
            inputs.len()
                + outputs.iter().position(|dot| dot.port == *port)?
        } else {
            inputs.iter().position(|dot| dot.port == *port)?
        };
        let kind = if output { &outputs } else { &inputs };
        let value_type =
            kind.iter().find(|dot| dot.port == *port)?.value_type;
        Some((
            dot_at(card_at(&editor, pan.0, port.node), row, output),
            value_type,
        ))
    };
    for Wire { from, to } in &graph.wires {
        if let (Some((a, value_type)), Some((b, _))) =
            (find(from, true), find(to, false))
        {
            curve(
                &mut commands,
                canvas,
                a,
                b,
                tint(palette, value_type),
            );
        }
    }
    if let Some((_, value_type, start, gone)) = &linking.0 {
        curve(
            &mut commands,
            canvas,
            *start,
            *start + *gone,
            tint(palette, *value_type),
        );
    }
}

/// A wire from `a` to `b`, leaving and arriving level.
fn curve(
    commands: &mut Commands,
    canvas: Entity,
    a: Vec2,
    b: Vec2,
    colour: Color,
) {
    let reach = ((b.x - a.x).abs() * 0.5).max(40.0);
    let (p1, p2) =
        (a + Vec2::new(reach, 0.0), b - Vec2::new(reach, 0.0));
    let at = |t: f32| {
        let u = 1.0 - t;
        a * (u * u * u)
            + p1 * (3.0 * u * u * t)
            + p2 * (3.0 * u * t * t)
            + b * (t * t * t)
    };
    for i in 0..PIECES {
        let (from, to) = (
            at(i as f32 / PIECES as f32),
            at((i + 1) as f32 / PIECES as f32),
        );
        let span = to - from;
        let length = span.length();
        if length < 0.01 {
            continue;
        }
        let middle = (from + to) * 0.5;
        commands.spawn((
            Piece,
            Node {
                position_type: PositionType::Absolute,
                left: px(middle.x - length * 0.5),
                top: px(middle.y - WIRE * 0.5),
                // A little over, so the pieces meet at the bends.
                width: px(length + 1.0),
                height: px(WIRE),
                ..Node::default()
            },
            UiTransform::from_rotation(Rot2::radians(
                span.y.atan2(span.x),
            )),
            BackgroundColor(colour),
            Pickable::IGNORE,
            ZIndex(-1),
            ChildOf(canvas),
        ));
    }
}

fn select(
    mut click: On<PointerClick>,
    cards: Query<&Card>,
    mut editor: ResMut<Editor>,
) {
    click.propagate(false);
    if let Ok(card) = cards.get(click.entity) {
        editor.selected = Some(card.0);
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
    mut commands: Commands,
    cards: Query<&Card>,
    editor: Res<Editor>,
) {
    start.propagate(false);
    if let Ok(card) = cards.get(start.entity) {
        let at = editor
            .sheet
            .layout
            .get(&card.0)
            .copied()
            .unwrap_or_default();
        commands.entity(start.entity).insert(Grabbed(at));
    }
}

fn drag_card(
    mut drag: On<PointerDrag>,
    cards: Query<(&Card, &Grabbed)>,
    mut editor: ResMut<Editor>,
) {
    drag.propagate(false);
    if drag.button != PointerButton::Primary {
        return;
    }
    if let Ok((card, grabbed)) = cards.get(drag.entity) {
        editor.drag(card.0, grabbed.0 + drag.distance);
    }
}

fn drop_card(
    mut end: On<PointerDragEnd>,
    mut commands: Commands,
    cards: Query<(&Card, &Grabbed)>,
    mut editor: ResMut<Editor>,
) {
    end.propagate(false);
    if let Ok((card, grabbed)) = cards.get(end.entity) {
        editor.dropped(card.0, grabbed.0);
        commands.entity(end.entity).remove::<Grabbed>();
    }
}

fn pan(drag: On<PointerDrag>, mut pan: ResMut<Pan>) {
    if drag.entity == drag.original_event_target() {
        pan.0 += drag.delta;
    }
}

fn start_link(
    mut start: On<PointerDragStart>,
    dots: Query<&Dot>,
    editor: Res<Editor>,
    pan: Res<Pan>,
    mut linking: ResMut<Linking>,
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
        card_at(&editor, pan.0, dot.port.node),
        inputs.len() + row,
        true,
    );
    linking.0 =
        Some((dot.port.clone(), dot.value_type, at, Vec2::ZERO));
}

fn drag_link(
    mut drag: On<PointerDrag>,
    mut linking: ResMut<Linking>,
) {
    drag.propagate(false);
    if let Some((_, _, _, gone)) = &mut linking.0 {
        *gone = drag.distance;
    }
}

fn end_link(
    mut end: On<PointerDragEnd>,
    mut linking: ResMut<Linking>,
) {
    end.propagate(false);
    linking.0 = None;
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
