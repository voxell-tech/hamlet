//! Rotations built with libm's trigonometry, which gives the same
//! bits on every platform where the standard library's may not.

use bevy_math::{Quat, Vec3};

/// A turn of `radians` about `axis`, a unit vector.
pub(crate) fn about(axis: Vec3, radians: f32) -> Quat {
    let (sin, cos) =
        (libm::sinf(radians * 0.5), libm::cosf(radians * 0.5));
    Quat::from_xyzw(axis.x * sin, axis.y * sin, axis.z * sin, cos)
}

/// Yaw about +Y, then pitch about +X, then roll about +Z, in degrees.
pub(crate) fn euler(yaw: f32, pitch: f32, roll: f32) -> Quat {
    about(Vec3::Y, yaw.to_radians())
        * about(Vec3::X, pitch.to_radians())
        * about(Vec3::Z, roll.to_radians())
}

/// A turn about +Y that points +X along `direction` seen from above.
pub(crate) fn yaw_toward(direction: Vec3) -> Quat {
    if direction.x == 0.0 && direction.z == 0.0 {
        return Quat::IDENTITY;
    }
    about(Vec3::Y, libm::atan2f(-direction.z, direction.x))
}
