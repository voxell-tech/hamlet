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

/// How a label `far` from the eye is shown, where labels are read up
/// to `farthest`: a far one smaller and fainter, and a near one drawn
/// over it, so that they read as standing apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Depth {
    pub scale: f32,
    /// The share of its plate's, edge's and words' strength it has.
    pub shown: f32,
    pub order: ZIndex,
}

/// The share of `farthest` a label is shown in full up to, the share
/// of its size it shrinks to at `farthest`, and the share of its
/// strength it fades to there.
const CLOSE: f32 = 0.2;
const SMALLEST: f32 = 0.75;
const FAINTEST: f32 = 0.4;

pub(crate) fn depth(far: f32, farthest: f32) -> Depth {
    let near = 1.0
        - ((far - farthest * CLOSE) / (farthest * (1.0 - CLOSE)))
            .clamp(0.0, 1.0);
    Depth {
        scale: SMALLEST + (1.0 - SMALLEST) * near,
        shown: FAINTEST + (1.0 - FAINTEST) * near,
        // Ahead of nothing but farther labels, all under what else
        // hangs over the world.
        order: ZIndex(-1 - (far * 10.0) as i32),
    }
}

/// Moves a label's corner to `corner`.
pub(crate) fn put(node: &mut Mut<Node>, corner: Vec2) {
    let (left, top) = (px(corner.x), px(corner.y));
    if (node.left, node.top) != (left, top) {
        node.left = left;
        node.top = top;
    }
}
