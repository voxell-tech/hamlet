//! What every panel is built from.

pub mod dial;
pub mod disclosure;
pub mod edges;
pub mod effects;
pub mod ellipsis;
pub mod floats;
pub mod forms;
pub mod hint;
pub mod icons;
pub mod keys;
pub mod lasting;
pub mod pixels;
pub mod platform;
pub mod providers;
pub mod rainbow;
pub mod range;
pub mod search;
pub mod secret;
pub mod shimmer;
pub mod spoken;
pub mod switch;
pub mod tabs;
pub mod trail;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::math::{Affine2, Rect, Vec2};
use bevy::ui::{ComputedNode, UiGlobalTransform};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        tabs::plugin,
        platform::plugin,
        icons::plugin,
        effects::plugin,
        switch::plugin,
        edges::plugin,
        floats::FloatsPlugin,
        pixels::plugin,
        ellipsis::plugin,
        hint::plugin,
        providers::plugin,
        rainbow::plugin,
        shimmer::plugin,
        spoken::plugin,
    ))
    .add_plugins((
        forms::plugin,
        lasting::plugin,
        search::plugin,
        secret::plugin,
    ));
}

/// The rect of `title`, in the window's logical pixels.
pub fn rect_of(world: &World, title: Entity) -> Option<Rect> {
    let (node, at) = world
        .get::<ComputedNode>(title)
        .zip(world.get::<UiGlobalTransform>(title))?;
    let scale = node.inverse_scale_factor();
    let (_, _, centre) = at.to_scale_angle_translation();
    Some(Rect::from_center_size(centre * scale, node.size() * scale))
}

/// Moves the laid-out node `at` and all inside it by `by`, in
/// physical pixels.
pub fn shift(
    at: Entity,
    by: Vec2,
    transforms: &mut Query<&mut UiGlobalTransform>,
    inside: &Query<&Children>,
) {
    if let Ok(mut transform) = transforms.get_mut(at) {
        let mut moved = Affine2::from(*transform);
        moved.translation += by;
        *transform = moved.into();
    }
    if let Ok(kids) = inside.get(at) {
        for &kid in kids {
            shift(kid, by, transforms, inside);
        }
    }
}
