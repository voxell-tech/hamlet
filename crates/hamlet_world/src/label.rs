//! A UI node kept over a point of the world.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::WorldCamera;

/// The world as its transforms have it now. A label over the world
/// is placed from it before the layout, which is ahead of the
/// frame's propagation.
#[derive(SystemParam)]
pub struct Lens<'w, 's> {
    camera: Single<
        'w,
        's,
        (Entity, &'static Camera, &'static GlobalTransform),
        With<WorldCamera>,
    >,
    now: TransformHelper<'w, 's>,
}

impl Lens<'_, '_> {
    /// The world's camera and where it stands.
    pub(crate) fn camera(&self) -> (&Camera, GlobalTransform) {
        let (entity, camera, was) = *self.camera;
        (camera, self.stands(entity, was))
    }

    /// The transform of `entity` as of this frame. `was` is the one
    /// it was last drawn at.
    pub(crate) fn stands(
        &self,
        entity: Entity,
        was: &GlobalTransform,
    ) -> GlobalTransform {
        self.now.compute_global_transform(entity).unwrap_or(*was)
    }
}

/// The top left of a label `computed` big that hangs `gap` pixels
/// over `point`, on whole pixels so that its text stays sharp.
pub(crate) fn above(
    computed: &ComputedNode,
    point: Vec2,
    gap: f32,
) -> Vec2 {
    let size = computed.size() * computed.inverse_scale_factor();
    (point - Vec2::new(size.x * 0.5, size.y + gap)).round()
}

/// Moves a label's corner to `corner`.
pub(crate) fn put(node: &mut Mut<Node>, corner: Vec2) {
    let (left, top) = (px(corner.x), px(corner.y));
    if (node.left, node.top) != (left, top) {
        node.left = left;
        node.top = top;
    }
}
