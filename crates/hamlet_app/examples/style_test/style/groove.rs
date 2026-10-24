//! The style test's groove along its path: the ribbon meshes and
//! the uniforms for each state it can show.

use bevy::prelude::*;
use hamlet_realm::meshes::Meshing;
use hamlet_realm::sky::Looks;
use hamlet_vfx::inlay::{
    InlayState, InlayUniform, LIT, current, fault, signal,
};

use super::scene::Terrain;

/// The uniforms for a state at time `t`. `wash` is 0 for the groove
/// itself. `gain` lifts the light for bright surroundings: by day the
/// groove competes with sunlit stone.
pub fn uniform(
    state: InlayState,
    t: f32,
    terrain: &Terrain,
    wash: f32,
    gain: f32,
    looks: &Looks,
) -> InlayUniform {
    let total = terrain.path.total();
    let (speed, from) =
        (terrain.at.mote_speed, terrain.at.visible_from);
    let strip = looks.strip_share;
    let (hue, mode, range, front) = match state {
        InlayState::Unlit => (current(), 0.0, (0.0, total), 0.0),
        InlayState::Current => {
            let period = total / speed + 9.0;
            (current(), 1.0, (0.0, total), (t % period) * speed)
        }
        InlayState::Fill => {
            let span = total - from;
            (
                current(),
                2.0,
                (from, total),
                from + (t % (span + 2.0)).min(span + 0.4),
            )
        }
        InlayState::Signal => {
            (signal(), 3.0, (total - 4.2, total + 1.0), 0.0)
        }
        InlayState::Fault => {
            (fault(), 4.0, (total - 7.5, total + 1.0), 0.0)
        }
    };
    InlayUniform {
        hue: hue.extend(LIT * gain),
        range: Vec4::new(range.0, range.1, 0.0, total),
        anim: Vec4::new(mode, t, front, looks.trace_half_life),
        shape: Vec4::new(strip, wash, speed, 0.0),
    }
}

fn ribbon(
    terrain: &Terrain,
    width: f32,
    lift: f32,
    skirt: f32,
) -> Mesh {
    let mut meshing = Meshing::new();
    let total = terrain.path.total();
    let mut prev: Option<[u32; 4]> = None;
    let steps = (total / 0.05).ceil() as usize;
    for i in 0..=steps {
        let s = total * i as f32 / steps as f32;
        let (p, tan) = terrain.on_path(s);
        let n = Vec3::new(-tan.z, 0.0, tan.x);
        let up = Vec3::Y * lift;
        let (l, r) =
            (p - n * width * 0.5 + up, p + n * width * 0.5 + up);
        let row = [
            meshing.vuv(
                l - Vec3::Y * skirt,
                -n,
                Vec3::splat(0.6),
                Vec2::new(s, -1.0),
            ),
            meshing.vuv(l, Vec3::Y, Vec3::ONE, Vec2::new(s, -1.0)),
            meshing.vuv(r, Vec3::Y, Vec3::ONE, Vec2::new(s, 1.0)),
            meshing.vuv(
                r - Vec3::Y * skirt,
                n,
                Vec3::splat(0.6),
                Vec2::new(s, 1.0),
            ),
        ];
        if let Some(q) = prev {
            meshing.quad(q[1], q[2], row[2], row[1]);
            if skirt > 0.0 {
                meshing.quad(q[0], q[1], row[1], row[0]);
                meshing.quad(q[2], q[3], row[3], row[2]);
            }
        }
        prev = Some(row);
    }
    meshing.build()
}

/// The groove itself: bed and strip in one ribbon along the path.
pub fn groove_mesh(terrain: &Terrain) -> Mesh {
    ribbon(terrain, terrain.at.groove_width, 0.022, 0.06)
}

/// The wider ribbon that carries the night wash.
pub fn wash_mesh(terrain: &Terrain) -> Mesh {
    ribbon(terrain, terrain.at.wash_width, 0.034, 0.0)
}
