//! Words selected where they are drawn, in any text of the panels: a
//! press and a drag, a double-click for a word or a triple-click for
//! a line. An [`Area`], such as the words of a message or a Markdown
//! page, is selected across all its text; any other text on its own.
//! Bevy's text layout and parley's selection find the words and the
//! boxes to light. What is selected is lit on a layer over the
//! panels, copied with Cmd+C and, in the chat, quoted in the field
//! with the button at its end. Buttons and fields keep their own
//! presses.

use std::ops::Range;

use bevy::app::{App, PostUpdate, Startup, Update};
use bevy::camera::visibility::InheritedVisibility;
use bevy::clipboard::Clipboard;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::message::MessageReader;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Or, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{
    Commands, Local, Query, Res, ResMut, Single, SystemParam,
};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::math::{Rect, Vec2};
use bevy::picking::Pickable;
use bevy::picking::events::{Drag, DragEnd, Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::text::{
    ComputedTextBlock, EditableText, LineBreak, TextBrush,
    TextLayout, TextSpan,
};
use bevy::ui::widget::Text;
use bevy::ui::{
    BackgroundColor, CalculatedClip, ComputedNode,
    ComputedUiRenderTargetInfo, GlobalZIndex, Node, PositionType,
    UiGlobalTransform, UiRect, UiScale, UiSystems, Val, percent, px,
};
use bevy::ui_widgets::Button;
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use bevy_fynix::{EntityCursor, ScopedExt, ViewExt, keyed, resource};
use parley::{Affinity, AlignmentOptions, Cursor, Layout, Selection};

use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{brass, marked};
use crate::me::Me;
use crate::theme::Themed;
use crate::{View, chat};

/// The layer the light is drawn on: over the panels, under the drag
/// of a tab, the menus and the tooltips.
const LAYER: i32 = 100;

/// The room the quote button is given before the end of the area.
const BUTTON: f32 = 86.0;

/// The room between what is selected and the button under it.
const UNDER: f32 = 4.0;

/// The space a bubble keeps at the end of its words for the hour.
const RESERVE: char = '\u{a0}';

pub fn plugin(app: &mut App) {
    app.init_resource::<Selecting>()
        .add_observer(press)
        .add_observer(drag)
        .add_observer(drag_end)
        .add_systems(Startup, layer)
        .add_systems(Update, (keys, forget))
        .add_systems(
            PostUpdate,
            (mark, light).after(UiSystems::PostLayout),
        );
}

/// How far a press reaches: a letter, a word or a line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Grain {
    #[default]
    Letter,
    Word,
    Line,
}

/// A place in selectable words: a text node and the bytes of it that
/// were pressed on, empty for a letter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Spot {
    node: Entity,
    start: usize,
    end: usize,
}

/// What is selected and where.
#[derive(Resource, Default)]
pub struct Selecting {
    /// The area, or the text node, the words are selected in.
    area: Option<Entity>,
    /// The message of the chat they are in, by its conversation and
    /// entry, so they can be quoted.
    quote: Option<(Entity, String)>,
    anchor: Option<Spot>,
    focus: Option<Spot>,
    grain: Grain,
    /// The pointer is held down after a press on the words.
    dragging: bool,
    /// Every word of the area is selected.
    whole: bool,
    /// The words selected.
    pub chosen: String,
    /// Where the quote button stands in its area, in pixels.
    spot: Vec2,
}

impl Selecting {
    fn clear(&mut self) {
        *self = Selecting::default();
    }
}

/// Text whose words are selected as one, across every text node in
/// it.
#[derive(Component)]
pub struct Area;

/// On a row of the chat whose words can be quoted.
#[derive(Component, Clone, Debug)]
pub struct Quotable {
    pub session: Entity,
    pub entry: String,
}

/// The layer over the panels that what is selected is lit on.
#[derive(Component)]
struct Lit;

/// A box of light on the [`Lit`] layer.
#[derive(Component)]
struct Glow;

/// What a press on keeps the selection, such as its quote button.
#[derive(Component)]
struct Keeps;

/// The text nodes of the panels and where the pointer is over them.
#[derive(SystemParam)]
struct Texts<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    parents: Query<'w, 's, &'static ChildOf>,
    blocks: Query<
        'w,
        's,
        (
            &'static ComputedTextBlock,
            &'static TextLayout,
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static ComputedUiRenderTargetInfo,
        ),
    >,
    words: Query<
        'w,
        's,
        (Option<&'static Text>, Option<&'static TextSpan>),
    >,
    controls:
        Query<'w, 's, (), Or<(With<Button>, With<EditableText>)>>,
    cursors: Query<'w, 's, &'static EntityCursor>,
    seen: Query<
        'w,
        's,
        (
            Option<&'static InheritedVisibility>,
            Option<&'static Pickable>,
        ),
    >,
    clips: Query<'w, 's, &'static CalculatedClip>,
    scale: Res<'w, UiScale>,
}

impl Texts<'_, '_> {
    /// True when the words of the text node `node` can be selected:
    /// it is seen, the pointer reaches it, and it is not part of a
    /// button, a field or anything else that has a hand of its own.
    fn selectable(&self, node: Entity) -> bool {
        let seen = self.seen.get(node).is_ok_and(|(shown, pick)| {
            shown.is_none_or(|s| s.get())
                && pick.is_none_or(|p| p.is_hoverable)
        });
        seen && std::iter::once(node)
            .chain(self.parents.iter_ancestors(node))
            .all(|e| {
                !self.controls.contains(e)
                    && self
                        .cursors
                        .get(e)
                        .ok()
                        .is_none_or(|c| c.0 == SystemCursorIcon::Text)
            })
    }

    /// The text nodes of `area` that can be selected, in the order
    /// they are read. A text node is an area of its own.
    fn nodes(&self, area: Entity) -> Vec<Entity> {
        if self.blocks.contains(area) {
            return vec![area];
        }
        self.children
            .iter_descendants_depth_first(area)
            .filter(|node| {
                self.blocks.contains(*node) && self.selectable(*node)
            })
            .collect()
    }

    /// The words of the text node `node`, as its layout has them.
    fn text(&self, node: Entity) -> String {
        let Ok((block, ..)) = self.blocks.get(node) else {
            return String::new();
        };
        block
            .entities()
            .iter()
            .filter_map(|part| self.words.get(part.entity).ok())
            .map(|(text, span)| match (text, span) {
                (Some(text), _) => text.0.as_str(),
                (_, Some(span)) => span.0.as_str(),
                _ => "",
            })
            .collect()
    }

    /// The layout of the text node `node` as it is drawn. Bevy
    /// measures a node by breaking its layout at other widths too, so
    /// the one it keeps is broken again here as Bevy draws it.
    fn laid(&self, node: Entity) -> Option<Layout<TextBrush>> {
        let (block, text_layout, computed, ..) =
            self.blocks.get(node).ok()?;
        let mut layout = block.buffer().clone();
        let width = match text_layout.linebreak {
            LineBreak::NoWrap => None,
            _ => Some(computed.content_box().width()),
        };
        layout.break_all_lines(width);
        layout.align(
            text_layout.justify.into(),
            AlignmentOptions::default(),
        );
        Some(layout)
    }

    /// The top and the bottom of the text node `node` in the window,
    /// in physical pixels.
    fn level(&self, node: Entity) -> Option<(f32, f32)> {
        let (_, _, computed, transform, _) =
            self.blocks.get(node).ok()?;
        let content = computed.content_box();
        let top = transform.transform_point2(content.min).y;
        Some((top, top + content.height()))
    }

    /// The window's `point` in the content of the text node `node`,
    /// in physical pixels, and the size of that content.
    fn local(
        &self,
        node: Entity,
        point: Vec2,
    ) -> Option<(Vec2, Vec2)> {
        let (_, _, computed, transform, target) =
            self.blocks.get(node).ok()?;
        let physical = point * target.scale_factor() / self.scale.0;
        let content = computed.content_box();
        let local =
            transform.try_inverse()?.transform_point2(physical)
                - content.min;
        Some((local, content.size()))
    }

    /// The place the window's `point` falls on among `nodes`, read
    /// by `grain`. Of the nodes on its line, the one under it or the
    /// nearest; between lines, the start of the next; past them all,
    /// the end of the last.
    fn place(
        &self,
        nodes: &[Entity],
        point: Vec2,
        grain: Grain,
    ) -> Option<Spot> {
        let mut line = Vec::new();
        for node in nodes {
            let Some((local, size)) = self.local(*node, point) else {
                continue;
            };
            if local.y < 0.0 && line.is_empty() {
                return Some(Spot {
                    node: *node,
                    start: 0,
                    end: 0,
                });
            }
            if (0.0..=size.y).contains(&local.y) {
                line.push((*node, local, size));
            }
        }
        let away = |(_, local, size): &(Entity, Vec2, Vec2)| {
            if local.x < 0.0 {
                -local.x
            } else {
                (local.x - size.x).max(0.0)
            }
        };
        let Some((node, local, _)) = line
            .iter()
            .min_by(|a, b| away(a).total_cmp(&away(b)))
            .copied()
        else {
            let last = *nodes.last()?;
            let end = self.text(last).len();
            return Some(Spot {
                node: last,
                start: end,
                end,
            });
        };
        let layout = self.laid(node)?;
        let found = match grain {
            Grain::Letter => {
                Selection::from_point(&layout, local.x, local.y)
            }
            Grain::Word => {
                Selection::word_from_point(&layout, local.x, local.y)
            }
            Grain::Line => {
                Selection::line_from_point(&layout, local.x, local.y)
            }
        };
        let range = found.text_range();
        Some(Spot {
            node,
            start: range.start,
            end: range.end,
        })
    }

    /// The boxes of `range` of the text node `node` as they are seen,
    /// cut to what clips it, in logical pixels of the window.
    fn boxes(&self, node: Entity, range: Range<usize>) -> Vec<Rect> {
        let (Some(layout), Ok((_, _, computed, transform, _))) =
            (self.laid(node), self.blocks.get(node))
        else {
            return Vec::new();
        };
        let selection = Selection::new(
            Cursor::from_byte_index(
                &layout,
                range.start,
                Affinity::Downstream,
            ),
            Cursor::from_byte_index(
                &layout,
                range.end,
                Affinity::Upstream,
            ),
        );
        let origin = computed.content_box().min;
        let clip = self.clips.get(node).ok().map(|c| c.clip);
        let scale = computed.inverse_scale_factor();
        selection
            .geometry(&layout)
            .into_iter()
            .filter_map(|(b, _)| {
                let corner = |x: f64, y: f64| {
                    transform.transform_point2(
                        origin + Vec2::new(x as f32, y as f32),
                    )
                };
                let seen = Rect::from_corners(
                    corner(b.x0, b.y0),
                    corner(b.x1, b.y1),
                );
                let seen = clip.map_or(seen, |c| seen.intersect(c));
                (!seen.is_empty()).then(|| Rect {
                    min: seen.min * scale,
                    max: seen.max * scale,
                })
            })
            .collect()
    }
}

/// The length of `text` without the room a bubble keeps for its hour.
fn worded(text: &str) -> usize {
    text.trim_end_matches(RESERVE).len()
}

/// The part of each of `order` that a selection between `a` and `b`
/// covers, the nodes it covers none of left out. `len` is the length
/// of a node's words.
fn covered(
    order: &[Entity],
    a: Spot,
    b: Spot,
    len: impl Fn(Entity) -> usize,
) -> Vec<(Entity, Range<usize>)> {
    let at = |spot: &Spot| order.iter().position(|n| *n == spot.node);
    let (Some(ia), Some(ib)) = (at(&a), at(&b)) else {
        return Vec::new();
    };
    let ((first, from), (last, to)) =
        if (ia, a.start) <= (ib, b.start) {
            ((ia, a), (ib, b))
        } else {
            ((ib, b), (ia, a))
        };
    // A word or a line pressed at both ends is lit whole.
    let end = match first == last {
        true => from.end.max(to.end),
        false => to.end,
    };
    (first..=last)
        .filter_map(|i| {
            let node = order[i];
            let most = len(node);
            let start = if i == first { from.start } else { 0 };
            let stop = if i == last { end } else { most };
            let range = start.min(most)..stop.min(most);
            (!range.is_empty()).then_some((node, range))
        })
        .collect()
}

/// The words `covered` selects: the parts of nodes on one line of
/// the window, such as a bullet and its item, with a space between
/// them, and the parts on different lines with a line between them.
/// `level` is where a node stands, from its top to its bottom.
fn chosen(
    covered: &[(Entity, Range<usize>)],
    text: impl Fn(Entity) -> String,
    level: impl Fn(Entity) -> Option<(f32, f32)>,
) -> String {
    let mut words = String::new();
    let mut was: Option<(f32, f32)> = None;
    for (node, range) in covered {
        let Some(part) =
            text(*node).get(range.clone()).map(str::to_string)
        else {
            continue;
        };
        let now = level(*node);
        if !words.is_empty() {
            let beside = match (was, now) {
                (Some((top, bottom)), Some((at, _))) => {
                    at >= top && at < bottom
                }
                _ => false,
            };
            words.push(if beside { ' ' } else { '\n' });
        }
        words.push_str(&part);
        was = now;
    }
    words
}

/// Spawns the layer over the panels that selections are lit on.
fn layer(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0),
            top: px(0.0),
            width: percent(100.0),
            height: percent(100.0),
            ..Node::default()
        },
        GlobalZIndex(LAYER),
        Pickable::IGNORE,
        Lit,
    ));
}

/// Gives selectable text the cursor that says so.
fn mark(
    added: Query<
        Entity,
        (Added<ComputedTextBlock>, Without<EntityCursor>),
    >,
    texts: Texts,
    mut commands: Commands,
) {
    for node in &added {
        if texts.selectable(node) {
            commands
                .entity(node)
                .try_insert(EntityCursor(SystemCursorIcon::Text));
        }
    }
}

/// Starts a selection on a press on selectable words, and lets it go
/// on a press anywhere else.
fn press(
    press: On<Pointer<Press>>,
    areas: Query<(), With<Area>>,
    rows: Query<&Quotable>,
    keeps: Query<(), With<Keeps>>,
    texts: Texts,
    mut focus: ResMut<InputFocus>,
    mut selecting: ResMut<Selecting>,
) {
    // A press bubbles up through every ancestor; it is read once.
    if press.entity != press.original_event_target()
        || press.button != PointerButton::Primary
    {
        return;
    }
    let chain = std::iter::once(press.entity)
        .chain(texts.parents.iter_ancestors(press.entity))
        .collect::<Vec<_>>();
    if chain.iter().any(|e| keeps.contains(*e)) {
        return;
    }
    // A link is a span of its text: the text is what is selected.
    let text =
        chain.iter().copied().find(|e| texts.blocks.contains(*e));
    let control = chain.iter().any(|e| texts.controls.contains(*e));
    // The outermost area, so a message is one, whatever is in it.
    let area =
        chain.iter().rev().copied().find(|e| areas.contains(*e));
    let scope = match (control, area, text) {
        (false, Some(area), _) => Some(area),
        (false, None, Some(text)) if texts.selectable(text) => {
            Some(text)
        }
        _ => None,
    };
    let Some(scope) = scope else {
        if selecting.area.is_some() {
            selecting.clear();
        }
        return;
    };
    let grain = match press.count {
        0 | 1 => Grain::Letter,
        2 => Grain::Word,
        _ => Grain::Line,
    };
    let nodes = texts.nodes(scope);
    let spot =
        texts.place(&nodes, press.pointer_location.position, grain);
    let quote = area
        .and_then(|_| chain.iter().find_map(|e| rows.get(*e).ok()))
        .map(|row| (row.session, row.entry.clone()));
    *selecting = Selecting {
        area: Some(scope),
        quote,
        anchor: spot,
        focus: spot,
        grain,
        dragging: true,
        ..Selecting::default()
    };
    // Keys go to the selection, not to a field.
    if focus.get().is_some() {
        focus.clear();
    }
}

/// Moves the end of a selection with the pointer as it is dragged.
fn drag(
    drag: On<Pointer<Drag>>,
    texts: Texts,
    mut selecting: ResMut<Selecting>,
) {
    if drag.entity != drag.original_event_target()
        || drag.button != PointerButton::Primary
        || !selecting.dragging
    {
        return;
    }
    let Some(area) = selecting.area else {
        return;
    };
    let nodes = texts.nodes(area);
    let focus = texts.place(
        &nodes,
        drag.pointer_location.position,
        selecting.grain,
    );
    if focus.is_some() && selecting.focus != focus {
        selecting.focus = focus;
    }
}

fn drag_end(
    end: On<Pointer<DragEnd>>,
    mut selecting: ResMut<Selecting>,
) {
    if end.entity == end.original_event_target() && selecting.dragging
    {
        selecting.dragging = false;
    }
}

/// Copies what is selected on Cmd+C, selects the whole area on
/// Cmd+A, and lets it go on Esc, unless a field is typed in.
fn keys(
    mut typed: MessageReader<KeyboardInput>,
    held: Res<ButtonInput<Key>>,
    focus: Res<InputFocus>,
    fields: Query<(), With<EditableText>>,
    clipboard: Option<ResMut<Clipboard>>,
    mut selecting: ResMut<Selecting>,
) {
    let typing =
        focus.get().is_some_and(|node| fields.contains(node));
    let command = if cfg!(target_os = "macos") {
        held.pressed(Key::Super)
    } else {
        held.pressed(Key::Control)
    };
    let mut clipboard = clipboard;
    for key in typed.read() {
        if typing
            || !key.state.is_pressed()
            || selecting.area.is_none()
        {
            continue;
        }
        match &key.logical_key {
            Key::Character(c)
                if command && c.eq_ignore_ascii_case("c") =>
            {
                if let Some(clipboard) = clipboard.as_mut()
                    && !selecting.chosen.is_empty()
                    && let Err(e) =
                        clipboard.set_text(selecting.chosen.clone())
                {
                    bevy::log::warn!(
                        "could not copy the selection: {e:?}"
                    );
                }
            }
            Key::Character(c)
                if command && c.eq_ignore_ascii_case("a") =>
            {
                selecting.whole = true;
            }
            Key::Escape => selecting.clear(),
            _ => {}
        }
    }
}

/// Lets go of words selected in the chat once it is with another
/// agent.
fn forget(
    me: Option<Res<Me>>,
    mut with: Local<Option<Entity>>,
    mut selecting: ResMut<Selecting>,
) {
    let Some(me) = me else {
        return;
    };
    if with.is_some_and(|was| was != me.agent)
        && selecting.quote.is_some()
    {
        selecting.clear();
    }
    *with = Some(me.agent);
}

/// Lights what is selected, keeps its words for a copy or a quote,
/// and puts the quote button at its end.
fn light(
    mut commands: Commands,
    texts: Texts,
    rows: Query<(Entity, &Quotable)>,
    areas: Query<(), With<Area>>,
    alive: Query<(), With<Node>>,
    sizes: Query<(&ComputedNode, &UiGlobalTransform)>,
    layer: Option<Single<(Entity, Option<&Children>), With<Lit>>>,
    glows: Query<(), With<Glow>>,
    theme: Themed,
    mut shown: Local<Vec<Rect>>,
    mut selecting: ResMut<Selecting>,
) {
    // The area, found again in its message if that was built anew.
    let area =
        selecting.area.filter(|a| alive.contains(*a)).or_else(|| {
            let (session, id) = selecting.quote.as_ref()?;
            let row = rows
                .iter()
                .find(|(_, r)| {
                    r.session == *session && &r.entry == id
                })?
                .0;
            texts
                .children
                .iter_descendants(row)
                .find(|n| areas.contains(*n))
        });
    let nodes = area.map(|a| texts.nodes(a)).unwrap_or_default();
    let ends = match (&*selecting, nodes.first(), nodes.last()) {
        (s, Some(first), Some(last)) if s.whole => {
            let end = texts.text(*last).len();
            Some((
                Spot {
                    node: *first,
                    start: 0,
                    end: 0,
                },
                Spot {
                    node: *last,
                    start: end,
                    end,
                },
            ))
        }
        (s, ..) => s.anchor.zip(s.focus),
    };
    let parts = ends
        .map(|(a, b)| {
            covered(&nodes, a, b, |node| worded(&texts.text(node)))
        })
        .unwrap_or_default();
    let boxes = parts
        .iter()
        .flat_map(|(n, range)| texts.boxes(*n, range.clone()))
        .collect::<Vec<_>>();

    if let Some(layer) = layer
        && *shown != boxes
    {
        let (layer, lit) = *layer;
        for glow in lit.into_iter().flatten() {
            if glows.contains(*glow) {
                commands.entity(*glow).despawn();
            }
        }
        let fill = theme.palette.chosen;
        for b in &boxes {
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(b.min.x),
                    top: px(b.min.y),
                    width: px(b.width()),
                    height: px(b.height()),
                    ..Node::default()
                },
                BackgroundColor(fill),
                Pickable::IGNORE,
                Glow,
                ChildOf(layer),
            ));
        }
        // The button stands under the end of what is lit, in its
        // area.
        let end = boxes.iter().max_by(|a, b| {
            (a.max.y, a.max.x)
                .partial_cmp(&(b.max.y, b.max.x))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let spot = area.zip(end).and_then(|(area, end)| {
            let (node, transform) = sizes.get(area).ok()?;
            let scale = node.inverse_scale_factor();
            let corner =
                (transform.translation - node.size() / 2.0) * scale;
            let width = node.size().x * scale;
            let left = (end.max.x - corner.x - BUTTON)
                .clamp(0.0, (width - BUTTON).max(0.0));
            Some(Vec2::new(left, end.max.y - corner.y + UNDER))
        });
        if let Some(spot) = spot
            && selecting.spot != spot
        {
            selecting.spot = spot;
        }
        *shown = boxes;
    }

    let words = chosen(
        &parts,
        |node| texts.text(node),
        |node| texts.level(node),
    );
    if selecting.chosen != words {
        selecting.chosen = words;
    }
    if selecting.area != area && selecting.area.is_some() {
        selecting.area = area;
    }
}

/// Quotes what is selected in the field, and lets the selection go.
fn quote_chosen(world: &mut World) {
    let chosen =
        std::mem::take(&mut world.resource_mut::<Selecting>().chosen);
    world.resource_mut::<Selecting>().clear();
    chat::quote_in_field(world, &chosen);
}

/// The quote button of the entry `id` of `session`, at the end of
/// what is selected in it.
pub fn quote_button(session: Entity, id: String) -> impl View {
    keyed(
        resource::<Selecting, _>(move |s| {
            s.quote.as_ref().is_some_and(|(chat, entry)| {
                *chat == session && entry == &id
            }) && !s.chosen.trim().is_empty()
        }),
        |on| match on {
            true => row((marked(Glyph::Talk, "QUOTE")
                .rules(brass)
                .tagged(Mood::Plain)
                .on_activate(quote_chosen),))
            .position(PositionType::Absolute)
            .inset(resource::<Selecting, _>(|s| {
                UiRect::new(
                    px(s.spot.x),
                    Val::Auto,
                    px(s.spot.y),
                    Val::Auto,
                )
            }))
            .tagged(Keeps)
            .boxed(),
            false => frame().boxed(),
        },
    )
    .within(
        column(()).position(PositionType::Absolute).inset(
            UiRect::new(px(0.0), Val::Auto, px(0.0), Val::Auto),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(node: Entity, start: usize, end: usize) -> Spot {
        Spot { node, start, end }
    }

    #[test]
    fn a_selection_covers_the_paragraphs_between_its_ends() {
        let mut world = World::new();
        let [a, b, c] = [(); 3].map(|_| world.spawn_empty().id());
        let order = [a, b, c];
        let len = |_: Entity| 10;
        // Dragged back from the third paragraph into the first.
        let parts =
            covered(&order, spot(c, 4, 4), spot(a, 6, 6), len);
        assert_eq!(parts, vec![(a, 6..10), (b, 0..10), (c, 0..4)]);
        // A click lights nothing.
        assert!(
            covered(&order, spot(b, 3, 3), spot(b, 3, 3), len)
                .is_empty()
        );
        // A word double-clicked and dragged back over the word
        // before.
        assert_eq!(
            covered(&order, spot(b, 5, 9), spot(b, 0, 4), len),
            vec![(b, 0..9)]
        );
        // The end of a node is its words, not the room for the hour.
        let short = |node: Entity| if node == c { 3 } else { 10 };
        assert_eq!(
            covered(&order, spot(b, 8, 8), spot(c, 9, 9), short),
            vec![(b, 8..10), (c, 0..3)]
        );
    }

    #[test]
    fn the_words_chosen_skip_the_room_for_the_hour() {
        let mut world = World::new();
        let [a, b] = [(); 2].map(|_| world.spawn_empty().id());
        let text = |node: Entity| match node == a {
            true => "First line.".to_string(),
            false => {
                format!("Second.{}", RESERVE.to_string().repeat(4))
            }
        };
        assert_eq!(worded(&text(b)), "Second.".len());
        let parts =
            covered(&[a, b], spot(a, 6, 6), spot(b, 30, 30), |n| {
                worded(&text(n))
            });
        let lines = |n: Entity| match n == a {
            true => Some((0.0, 20.0)),
            false => Some((24.0, 44.0)),
        };
        assert_eq!(chosen(&parts, text, lines), "line.\nSecond.");
    }

    #[test]
    fn a_bullet_and_its_item_are_one_line() {
        let mut world = World::new();
        let [paragraph, bullet, item] =
            [(); 3].map(|_| world.spawn_empty().id());
        let text = |node: Entity| {
            match node {
                n if n == paragraph => "Three things:",
                n if n == bullet => "\u{2022}",
                _ => "straw at the roots",
            }
            .to_string()
        };
        let level = |node: Entity| match node == paragraph {
            true => Some((0.0, 20.0)),
            false => Some((24.0, 44.0)),
        };
        let parts =
            vec![(paragraph, 0..13), (bullet, 0..3), (item, 0..18)];
        assert_eq!(
            chosen(&parts, text, level),
            "Three things:\n\u{2022} straw at the roots"
        );
    }
}
