//! The Graph panel's layout: the line between the canvas and the
//! preview, dragged to size them, and the cards floating over the
//! canvas: the parameters, and a preview that floats. A card is
//! dragged by its header; while it is, an outline shows the corner it
//! will settle in when let go, and it glides there. Its left edge is
//! dragged to size it, and its chevron folds it to its header.

use std::marker::PhantomData;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::math::Vec2;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::time::Time;
use bevy::ui::{
    AlignItems, ComputedNode, Display, Overflow, PositionType,
    UiRect, Val, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{Motion, SpacingTokens, SurfaceTokens};
use bevy_fynix::views::{
    self, Axis, BehaviorExt, FrameProps, TooltipExt, button, column,
    frame, icon_button, row,
};
use bevy_fynix::{
    EntityCursor, OverrideCursor, ScopedExt, Signal, ViewExt,
    resource,
};
use hamlet_ui::styles::{ICON, note, section};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, View, with_cx};

use super::canvas::Canvas;
use super::setup::EditorKey;

/// The least the preview is wide, in logical pixels.
const PREVIEW: f32 = 120.0;
/// The least the canvas is wide, in logical pixels.
const CANVAS: f32 = 240.0;

/// How far a card sits from the canvas's edges.
const MARGIN: f32 = 8.0;

/// The least a card is wide.
const NARROWEST: f32 = 180.0;
/// The most a card is wide.
const WIDEST: f32 = 640.0;

/// How fast a card closes on where it is going, per second.
const GLIDE: f32 = 14.0;

pub(super) fn plugin<K: EditorKey>(app: &mut App) {
    app.init_resource::<Layout<K>>()
        .init_resource::<Shown<K>>()
        .add_observer(grab::<K>)
        .add_observer(drag::<K>)
        .add_observer(drop::<K>)
        .add_systems(Update, (glide::<K>, tabs::mark::<Shown<K>>));
}

/// A corner of the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// The corner of an area half `half` in size nearest `middle`.
    fn nearest(middle: Vec2, half: Vec2) -> Self {
        match (middle.x < half.x, middle.y < half.y) {
            (true, true) => Corner::TopLeft,
            (false, true) => Corner::TopRight,
            (true, false) => Corner::BottomLeft,
            (false, false) => Corner::BottomRight,
        }
    }

    /// The top left of a box of `size` in this corner of `area`.
    fn place(self, area: Vec2, size: Vec2) -> Vec2 {
        let far = (area - size - Vec2::splat(MARGIN))
            .max(Vec2::splat(MARGIN));
        match self {
            Corner::TopLeft => Vec2::splat(MARGIN),
            Corner::TopRight => Vec2::new(far.x, MARGIN),
            Corner::BottomLeft => Vec2::new(MARGIN, far.y),
            Corner::BottomRight => far,
        }
    }
}

/// A card floating over the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Pane {
    Parameters,
    Preview,
}

impl Pane {
    fn at(self) -> usize {
        self as usize
    }
}

/// Where a card floats and how. Places are in logical pixels from the
/// canvas's top left corner.
pub(super) struct Card {
    /// Whether it is shown at all, or hidden by the toolbar.
    pub visible: bool,
    /// Whether it is open, or folded to its header.
    pub open: bool,
    /// How wide it is.
    pub width: f32,
    /// The corner it settles in.
    corner: Corner,
    /// Where it is drawn.
    pub shown: Vec2,
    /// Where it would settle if let go now, while dragged.
    pub ghost: Option<Vec2>,
    /// Its size as drawn.
    pub size: Vec2,
    /// Where its top left is while it is dragged.
    held: Option<Vec2>,
    /// Whether it has been placed once, after which it glides.
    placed: bool,
}

impl Card {
    fn new(corner: Corner, width: f32) -> Self {
        Self {
            visible: true,
            open: true,
            width,
            corner,
            shown: Vec2::splat(MARGIN),
            ghost: None,
            size: Vec2::ZERO,
            held: None,
            placed: false,
        }
    }
}

/// Which tab of the card with tabs, the parameters, is shown.
#[derive(Resource)]
pub(super) struct Shown<K: EditorKey>(usize, PhantomData<K>);

impl<K: EditorKey> Shown<K> {
    fn at(at: usize) -> Self {
        Self(at, PhantomData)
    }
}

impl<K: EditorKey> Default for Shown<K> {
    fn default() -> Self {
        Self::at(0)
    }
}

// By hand, as derived ones would ask the same of `K`.
impl<K: EditorKey> Clone for Shown<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: EditorKey> Copy for Shown<K> {}

impl<K: EditorKey> PartialEq for Shown<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

/// The Graph panel's layout.
#[derive(Resource)]
pub(super) struct Layout<K: EditorKey> {
    /// The width of the preview beside the canvas, when it does not
    /// float.
    pub preview: f32,
    /// The cards, by [`Pane`].
    cards: [Card; 2],
    /// The canvas's size, as last laid out. Zero until it is.
    area: Vec2,
    key: PhantomData<K>,
}

impl<K: EditorKey> Default for Layout<K> {
    fn default() -> Self {
        Self {
            preview: 360.0,
            cards: [
                Card::new(Corner::TopRight, 300.0),
                Card::new(Corner::BottomRight, 280.0),
            ],
            area: Vec2::ZERO,
            key: PhantomData,
        }
    }
}

impl<K: EditorKey> Layout<K> {
    /// The tallest a card is drawn: the canvas less a margin above
    /// and below.
    fn tallest(&self) -> Val {
        if self.area.y > 0.0 {
            px((self.area.y - 2.0 * MARGIN).max(0.0))
        } else {
            percent(100.0)
        }
    }

    pub(super) fn card(&self, pane: Pane) -> &Card {
        &self.cards[pane.at()]
    }

    /// Shows the card `pane` if it is hidden, else hides it.
    pub(super) fn toggle(&mut self, pane: Pane) {
        let card = self.card_mut(pane);
        card.visible = !card.visible;
    }

    /// The display that shows while the card `pane` is visible.
    pub(super) fn shows(pane: Pane) -> Signal<Display> {
        resource::<Self, _>(move |layout| {
            if layout.card(pane).visible {
                Display::Flex
            } else {
                Display::None
            }
        })
    }

    fn card_mut(&mut self, pane: Pane) -> &mut Card {
        &mut self.cards[pane.at()]
    }

    /// Where the card being dragged would settle, if one is.
    fn ghost(&self) -> Option<(Vec2, Vec2)> {
        self.cards
            .iter()
            .find_map(|card| card.ghost.map(|at| (at, card.size)))
    }
}

/// The inset that puts a box's top left at `at`.
fn inset_at(at: Vec2) -> UiRect {
    UiRect::new(px(at.x), Val::Auto, px(at.y), Val::Auto)
}

impl Card {
    /// Where it is drawn: where it was placed, or in its corner by
    /// the corner's edges until its size is known to place it by.
    fn inset(&self) -> UiRect {
        if self.placed {
            return inset_at(self.shown);
        }
        let (edge, free) = (px(MARGIN), Val::Auto);
        match self.corner {
            Corner::TopLeft => UiRect::new(edge, free, edge, free),
            Corner::TopRight => UiRect::new(free, edge, edge, free),
            Corner::BottomLeft => UiRect::new(edge, free, free, edge),
            Corner::BottomRight => {
                UiRect::new(free, edge, free, edge)
            }
        }
    }
}

/// Marks the line between the canvas and the preview.
#[derive(Component)]
struct Divider<K: EditorKey>(PhantomData<K>);

/// Marks a card floating over the canvas.
#[derive(Component)]
struct Floating<K: EditorKey>(Pane, PhantomData<K>);

/// Marks the header a card is dragged by.
#[derive(Component)]
struct Handle<K: EditorKey>(Pane, PhantomData<K>);

/// Marks the left edge a card is sized by.
#[derive(Component)]
struct Grip<K: EditorKey>(Pane, PhantomData<K>);

/// The line the dock's panels are parted by, grabbed the same way,
/// shown with the preview it parts from the canvas.
pub(super) fn divider<K: EditorKey>() -> impl View {
    views::divider(Axis::Vertical)
        .display(Layout::<K>::shows(Pane::Preview))
        .tagged(Divider::<K>(PhantomData))
}

/// The card `pane` floating over the canvas: a header of the chevron
/// that folds it, and the body of the tab shown under it. With one
/// tab the header names it beside `icon`; with more, it is the strip
/// of the inspector's tabs, choosing [`Shown`].
pub(super) fn floating<K: EditorKey>(
    pane: Pane,
    icon: Glyph,
    tabs: Vec<(Glyph, &'static str, AnyView)>,
) -> AnyView {
    with_cx(move |cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        let open = move |layout: &Layout<K>| layout.card(pane).open;
        let title = tabs.first().map_or("", |(_, name, _)| name);
        // The chevron points down while open and left while folded.
        let fold = button(
            glyph(Glyph::Chevron, ICON)
                .rotation(resource::<Layout<K>, _>(move |layout| {
                    if open(layout) { 180.0 } else { 270.0 }
                }))
                .transition(Motion::Interact),
        )
        .rules(icon_button)
        .tooltip(move || {
            note(format!("Fold or open the {}", title.to_lowercase()))
        })
        .on_activate(move |world| {
            let mut layout = world.resource_mut::<Layout<K>>();
            let card = layout.card_mut(pane);
            card.open = !card.open;
        });
        let single = tabs.len() == 1;
        let names: AnyView = if single {
            row((glyph(icon, ICON), section(title)))
                .gap(gap / 2.0)
                .align(AlignItems::Center)
                .boxed()
        } else {
            let strip = tabs
                .iter()
                .enumerate()
                .map(|(at, (which, name, _))| {
                    (Shown::<K>::at(at), *which, *name)
                })
                .collect::<Vec<_>>();
            tabs::strip(&strip, |world, shown| {
                *world.resource_mut::<Shown<K>>() = shown;
            })
            .boxed()
        };
        // A tab pressed and dragged drags the card too: the press
        // reaches the header under it.
        let header = row((
            column((names,)).grow(1.0).min_width(px(0.0)),
            fold,
        ))
        .gap(gap / 2.0)
        .align(AlignItems::Center)
        .tagged(Handle::<K>(pane, PhantomData))
        .tagged(EntityCursor(SystemCursorIcon::Grab));
        let bodies = tabs
            .into_iter()
            .enumerate()
            .map(|(at, (_, _, body))| {
                if single {
                    return body;
                }
                let chosen = Shown::<K>::at(at);
                column((body,))
                    .min_height(px(0.0))
                    .display(resource::<Shown<K>, _>(move |shown| {
                        if *shown == chosen {
                            Display::Flex
                        } else {
                            Display::None
                        }
                    }))
                    .boxed()
            })
            .collect::<Vec<_>>();
        let display = resource::<Layout<K>, _>(move |layout| {
            if open(layout) {
                Display::Flex
            } else {
                Display::None
            }
        });
        // It can shrink below the body, for a body that scrolls.
        let shown = column(bodies)
            .min_height(px(0.0))
            .gap(0.0)
            .display(display);
        let grip = frame()
            .position(PositionType::Absolute)
            .inset(UiRect::new(px(-3.0), Val::Auto, px(0.0), px(0.0)))
            .width(px(6.0))
            .tagged(Grip::<K>(pane, PhantomData))
            .tagged(EntityCursor(SystemCursorIcon::EwResize));
        column((header, shown, grip))
            .position(PositionType::Absolute)
            .inset(resource::<Layout<K>, _>(move |layout| {
                layout.card(pane).inset()
            }))
            .width(resource::<Layout<K>, _>(move |layout| {
                px(layout.card(pane).width)
            }))
            .display(Layout::<K>::shows(pane))
            .max_height(resource::<Layout<K>, _>(Layout::tallest))
            .overflow(Overflow::clip())
            .padding(UiRect::all(px(gap)))
            .gap(gap / 2.0)
            .fill(theme.palette.card)
            .border(theme.space.border)
            .border_color(theme.palette.border)
            .radius(theme.space.round)
            .tagged(Floating::<K>(pane, PhantomData))
    })
    .boxed()
}

/// The hint of where a dragged card will settle, drawn as the dock
/// hints where a dragged tab will go.
pub(super) fn outline<K: EditorKey>() -> AnyView {
    with_cx(|cx| {
        let theme = cx.theme();
        frame()
            .position(PositionType::Absolute)
            .inset(resource::<Layout<K>, _>(|layout| {
                inset_at(
                    layout.ghost().map_or(Vec2::ZERO, |(at, _)| at),
                )
            }))
            .width(resource::<Layout<K>, _>(|layout| {
                px(layout.ghost().map_or(0.0, |(_, size)| size.x))
            }))
            .height(resource::<Layout<K>, _>(|layout| {
                px(layout.ghost().map_or(0.0, |(_, size)| size.y))
            }))
            .display(resource::<Layout<K>, _>(|layout| {
                if layout.ghost().is_some() {
                    Display::Flex
                } else {
                    Display::None
                }
            }))
            .fill(theme.selection())
            .radius(theme.radius())
    })
    .boxed()
}

fn logical(node: &ComputedNode) -> Vec2 {
    node.size() * node.inverse_scale_factor()
}

/// Moves each card toward where it belongs: under the pointer while
/// dragged, else its corner, easing there from where it is.
fn glide<K: EditorKey>(
    time: Res<Time>,
    floating: Query<(&Floating<K>, &ComputedNode)>,
    canvas: Query<&ComputedNode, With<Canvas<K>>>,
    mut layout: ResMut<Layout<K>>,
) {
    let Some(area) = canvas.iter().next() else {
        return;
    };
    let area = logical(area);
    // A canvas not laid out, as one in a tab not shown, keeps its
    // cards where they are, so they do not glide back from its
    // corner when it shows again.
    if area.cmple(Vec2::ZERO).any() {
        return;
    }
    if layout.area != area {
        layout.area = area;
    }
    let step = 1.0 - (-GLIDE * time.delta_secs()).exp();
    for (Floating(pane, _), node) in &floating {
        let size = logical(node);
        if size.cmple(Vec2::ZERO).any() {
            continue;
        }
        let now = layout.bypass_change_detection().card(*pane);
        let ghost = now.held.map(|at| {
            Corner::nearest(at + size * 0.5, area * 0.5)
                .place(area, size)
        });
        let target =
            now.held.unwrap_or_else(|| now.corner.place(area, size));
        let shown = if now.placed {
            let next = now.shown + (target - now.shown) * step;
            if next.distance(target) < 0.25 {
                target
            } else {
                next
            }
        } else {
            target
        };
        if now.shown != shown
            || now.ghost != ghost
            || now.size != size
            || !now.placed
        {
            let card = layout.card_mut(*pane);
            card.shown = shown;
            card.ghost = ghost;
            card.size = size;
            card.placed = true;
        }
    }
}

fn grab<K: EditorKey>(
    start: On<PointerDragStart>,
    dividers: Query<(), With<Divider<K>>>,
    handles: Query<&Handle<K>>,
    grips: Query<(), With<Grip<K>>>,
    mut layout: ResMut<Layout<K>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if dividers.contains(start.entity) || grips.contains(start.entity)
    {
        cursor.0 = Some(SystemCursorIcon::EwResize);
    } else if let Ok(Handle(pane, _)) = handles.get(start.entity) {
        let card = layout.card_mut(*pane);
        card.held = Some(card.shown);
        cursor.0 = Some(SystemCursorIcon::Grabbing);
    }
}

fn drag<K: EditorKey>(
    drag: On<PointerDrag>,
    dividers: Query<(), With<Divider<K>>>,
    handles: Query<&Handle<K>>,
    grips: Query<&Grip<K>>,
    canvas: Query<&ComputedNode, With<Canvas<K>>>,
    mut layout: ResMut<Layout<K>>,
) {
    if dividers.contains(drag.entity) {
        let canvas = canvas
            .iter()
            .next()
            .map_or(CANVAS, |node| logical(node).x);
        let most = (layout.preview + canvas - CANVAS).max(PREVIEW);
        layout.preview =
            (layout.preview - drag.delta.x).clamp(PREVIEW, most);
    } else if let Ok(Handle(pane, _)) = handles.get(drag.entity)
        && let Some(at) = &mut layout.card_mut(*pane).held
    {
        *at += drag.delta;
    } else if let Ok(Grip(pane, _)) = grips.get(drag.entity) {
        // The left edge: dragged left, the card grows.
        let card = layout.card_mut(*pane);
        card.width =
            (card.width - drag.delta.x).clamp(NARROWEST, WIDEST);
    }
}

/// Settles a dragged card in the corner of the canvas nearest its
/// middle, and puts the cursor back. A card held is settled whatever
/// the drag ends on, so a drag started on something in its header
/// never leaves it held.
fn drop<K: EditorKey>(
    end: On<PointerDragEnd>,
    dividers: Query<(), With<Divider<K>>>,
    grips: Query<(), With<Grip<K>>>,
    canvas: Query<&ComputedNode, With<Canvas<K>>>,
    mut layout: ResMut<Layout<K>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    let area = canvas.iter().next().map(logical);
    let mut settled = false;
    for card in &mut layout.bypass_change_detection().cards {
        if let Some(at) = card.held.take() {
            if let Some(area) = area {
                card.corner =
                    Corner::nearest(at + card.size * 0.5, area * 0.5);
            }
            card.ghost = None;
            settled = true;
        }
    }
    if settled {
        layout.set_changed();
    }
    if settled
        || dividers.contains(end.entity)
        || grips.contains(end.entity)
    {
        cursor.0 = None;
    }
}
