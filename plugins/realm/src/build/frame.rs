//! A place, turn and size in the realm, as glam has them, and as the
//! engine's scene takes them.

use glam::{Affine3A, Mat3, Quat, Vec3};
use hamlet_engine_sdk::scene::Transform;

/// A place, turn and size, applied in the order size, turn, place.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Frame {
    pub at: Vec3,
    pub turn: Quat,
    pub size: Vec3,
}

impl Default for Frame {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Frame {
    pub const IDENTITY: Self = Self {
        at: Vec3::ZERO,
        turn: Quat::IDENTITY,
        size: Vec3::ONE,
    };

    pub fn at(at: Vec3) -> Self {
        Self {
            at,
            ..Self::IDENTITY
        }
    }

    pub fn with_turn(self, turn: Quat) -> Self {
        Self { turn, ..self }
    }

    pub fn with_size(self, size: Vec3) -> Self {
        Self { size, ..self }
    }

    /// At `at`, its forward (-z) along `dir` and its up toward +y.
    pub fn looking_to(at: Vec3, dir: Vec3) -> Self {
        let back = -dir.normalize_or(Vec3::NEG_Z);
        let up = if back.cross(Vec3::Y).length_squared() < 1e-8 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let right = up.cross(back).normalize();
        let up = back.cross(right);
        Self::at(at).with_turn(Quat::from_mat3(&Mat3::from_cols(
            right, up, back,
        )))
    }

    /// At `eye`, looking at `target`.
    pub fn looking_at(eye: Vec3, target: Vec3) -> Self {
        Self::looking_to(eye, target - eye)
    }

    pub fn affine(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(
            self.size, self.turn, self.at,
        )
    }

    /// `point`, given in this frame, in the frame it stands in.
    pub fn point(&self, point: Vec3) -> Vec3 {
        self.at + self.turn * (self.size * point)
    }

    /// The transform the engine's scene takes.
    pub fn scene(&self) -> Transform {
        Transform {
            translation: self.at.into(),
            rotation: self.turn.into(),
            scale: self.size.into(),
        }
    }
}
