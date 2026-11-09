//! An asset being carried: while a tile is dragged, a card of what it
//! is follows the pointer, which grabs, until it is let go. The
//! canvas lights up under it where a drop would land.

use bevy::app::App;
use bevy::color::Alpha;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::picking::Pickable;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::picking::pointer::PointerButton;
use bevy::ui::{AlignItems, Node, PositionType, UiRect, UiScale, px};
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{BehaviorExt, FrameProps, MENU_Z, row};
use bevy_fynix::{OverrideCursor, ViewExt};
use hamlet_ui::styles::note;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::glyph;
use hamlet_ui::{AnyView, with_cx};

use super::picker::picture;
use super::{Item, Kind, Tile};

/// How far below and right of the pointer the card is held, so that
/// what is under the pointer is still what a drop lands on.
const HELD: Vec2 = Vec2::new(14.0, 14.0);

/// The side of the card's picture.
const PICTURE: f32 = 32.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Carried>()
        .add_observer(lift)
        .add_observer(follow)
        .add_observer(set_down);
}

/// The asset being carried and the card that follows the pointer.
#[derive(Resource, Default)]
pub struct Carried {
    card: Option<Entity>,
    /// What is carried, while a tile is dragged.
    pub item: Option<Item>,
}

/// Marks the card that follows the pointer.
#[derive(Component)]
struct Card;

/// The asset of the tile that is `entity` or holds it.
pub fn tile_of(world: &World, mut entity: Entity) -> Option<Item> {
    loop {
        if let Some(tile) = world.get::<Tile>(entity) {
            return Some(tile.0.clone());
        }
        entity = world.get::<ChildOf>(entity)?.parent();
    }
}

/// Picks up the tile a drag begins on, or inside, and shows its card.
fn lift(start: On<PointerDragStart>, mut commands: Commands) {
    // Once for each drag, where it began, not again on each node it
    // passes up through.
    if start.event_target() != start.original_event_target()
        || start.button != PointerButton::Primary
    {
        return;
    }
    let (from, at) = (start.entity, start.pointer.position);
    commands.queue(move |world: &mut World| {
        let Some(item) = tile_of(world, from) else {
            return;
        };
        drop_card(world);
        let card =
            bevy_fynix::mount::<HamletTheme>(world, card(&item));
        world.entity_mut(card).insert(Card);
        place(world, card, at);
        if let Some(mut cursor) =
            world.get_resource_mut::<OverrideCursor>()
        {
            cursor.0 = Some(SystemCursorIcon::Grabbing);
        }
        let mut carried = world.resource_mut::<Carried>();
        carried.card = Some(card);
        carried.item = Some(item);
    });
}

/// Keeps the card by the pointer.
fn follow(
    drag: On<PointerDrag>,
    carried: Res<Carried>,
    scale: Option<Res<UiScale>>,
    mut cards: Query<&mut Node, With<Card>>,
) {
    if drag.event_target() != drag.original_event_target() {
        return;
    }
    let Some(mut node) =
        carried.card.and_then(|card| cards.get_mut(card).ok())
    else {
        return;
    };
    let at =
        held_at(drag.pointer.position, scale.map_or(1.0, |s| s.0));
    node.left = px(at.x);
    node.top = px(at.y);
}

/// Takes the card away once the tile is let go, wherever it lands.
fn set_down(end: On<PointerDragEnd>, mut commands: Commands) {
    if end.event_target() != end.original_event_target() {
        return;
    }
    commands.queue(drop_card);
}

fn drop_card(world: &mut World) {
    let mut carried = world.resource_mut::<Carried>();
    carried.item = None;
    let Some(card) = carried.card.take() else {
        return;
    };
    if let Ok(entity) = world.get_entity_mut(card) {
        entity.despawn();
    }
    if let Some(mut cursor) =
        world.get_resource_mut::<OverrideCursor>()
    {
        cursor.0 = None;
    }
}

/// Puts `card` by the pointer at `at`, in the window's pixels.
fn place(world: &mut World, card: Entity, at: Vec2) {
    let scale = world.get_resource::<UiScale>().map_or(1.0, |s| s.0);
    let at = held_at(at, scale);
    if let Some(mut node) = world.get_mut::<Node>(card) {
        node.position_type = PositionType::Absolute;
        node.left = px(at.x);
        node.top = px(at.y);
    }
}

/// Where the card's corner goes for the pointer at `at`, in the
/// interface's pixels.
fn held_at(at: Vec2, scale: f32) -> Vec2 {
    at / scale + HELD
}

/// The card: the asset's picture, or its kind's glyph, and its name.
fn card(item: &Item) -> AnyView {
    let item = item.clone();
    with_cx(move |cx| {
        let theme = cx.theme();
        let (gap, round) = (theme.space.gap, theme.space.round);
        let palette = theme.palette.clone();
        let thumb = match item.kind {
            Kind::Mesh | Kind::Blueprint | Kind::Shader => {
                picture(item.kind, &item.name, PICTURE)
            }
            kind => glyph(kind.glyph(), 20.0).boxed(),
        };
        row((thumb, note(item.name.replace('_', " "))))
            .gap(gap / 2.0)
            .padding(UiRect::all(px(gap / 2.0)))
            .align(AlignItems::Center)
            .fill(palette.card.with_alpha(0.92))
            .border(1.0)
            .border_color(palette.glow)
            .radius(round)
            .z(Some(MENU_Z))
            .tagged(Pickable::IGNORE)
    })
    .boxed()
}
