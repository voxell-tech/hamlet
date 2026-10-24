//! Every robot put in the pose of its motion, each frame it is seen.

use std::f32::consts::TAU;

use bevy::camera::primitives::{Frustum, Sphere};
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_vfx::inlay::LIT;

use super::acts::Act;
use super::pose::{self, Prop, rx, ry, rz};
use super::{Keeper, Motion, Stride, Trip};
use crate::generator::DetailSettings;
use crate::{Clock, Sky, WorldCamera};

/// Picks the plain motion for a changed `state`.
fn follow_state(keeper: &mut Keeper, now: f32, frozen: bool) {
    let act = Act::of(keeper.state);
    // A frozen clock shows the settled moment.
    if frozen || keeper.seen.is_nan() {
        keeper.settle(act);
    } else {
        keeper.synced = keeper.state;
        keeper.start(act, now);
    }
}

pub(crate) fn animate(
    clock: Res<Clock>,
    sky: Res<Sky>,
    detail: Res<DetailSettings>,
    motion: Res<Motion>,
    view: Option<Single<&Frustum, With<WorldCamera>>>,
    mut keepers: Query<(
        &mut Keeper,
        &mut Transform,
        &GlobalTransform,
        &InheritedVisibility,
        Option<&Trip>,
    )>,
    mut parts: Query<&mut Transform, Without<Keeper>>,
    mut visibility: Query<&mut Visibility>,
    mut shapes: Query<&mut Mesh3d>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let t = clock.t;
    // True once any robot needs another frame.
    let mut busy = false;
    for (mut keeper, mut root, place, shown, trip) in &mut keepers {
        let keeper = &mut *keeper;
        if keeper.state != keeper.synced {
            follow_state(keeper, t, clock.frozen);
        }
        let stride = motion.stride * keeper.who.size;
        let first = keeper.seen.is_nan();
        let passed = (t - keeper.seen).abs();
        keeper.seen = t;

        let walked = if let Some(trip) = trip {
            let s = trip.at(t);
            root.translation = s.at;
            root.rotation = ry(s.yaw);
            busy |= !trip.has_arrived(t);
            s.stride()
        } else {
            // Without a stride it walks on the spot.
            keeper.strode.unwrap_or(Stride {
                gone: t * motion.walk_speed,
                pace: 1.0,
                speed: motion.walk_speed,
            })
        };
        keeper.gait = walked.gone / stride * TAU;
        keeper.pace = walked.pace;
        let rolls = f32::from(walked.speed > motion.roll_above);
        keeper.roll = if first || clock.frozen {
            rolls
        } else {
            let most = passed / motion.roll_ease.max(1e-4);
            keeper.roll + (rolls - keeper.roll).clamp(-most, most)
        };

        if let Some(len) = keeper.act.length()
            && t - keeper.since >= len
            && let Some(next) = keeper.act.then()
        {
            let at = keeper.since + len;
            keeper.start(next, at);
        }
        if let Some((_, was)) = &keeper.from
            && t - keeper.since >= keeper.act.easing(*was).0
        {
            keeper.from = None;
        }
        // A pose is a function of the clock alone, so a robot out of
        // sight is left as it was and picks up when it is seen again.
        let seen = first
            || view.as_ref().is_none_or(|frustum| {
                let around = Sphere {
                    center: place.translation_vec3a(),
                    radius: detail.robot_sight,
                };
                shown.get()
                    && frustum.intersects_sphere(&around, false)
            });
        if !seen {
            continue;
        }
        busy |= keeper.is_busy(t) || !keeper.act.is_resting();

        let p = keeper.eval(t);
        let placed = p.place(keeper.who.size);
        let ids = &keeper.parts;
        let mut set = |e: Entity, to: Transform| {
            if let Ok(mut tf) = parts.get_mut(e) {
                *tf = to;
            }
        };
        set(ids.rig, placed.rig);
        set(ids.body, placed.body);
        set(ids.head, placed.head);
        set(ids.hat, placed.hat);
        set(ids.eye_l, placed.eye_l);
        set(ids.eye_r, placed.eye_r);
        set(ids.leg_l, placed.leg_l);
        set(ids.leg_r, placed.leg_r);
        set(ids.pole, placed.pole);
        // The wheels turn by the ground covered, so they never slip.
        let turned = walked.gone / (pose::WHEEL * keeper.who.size);
        for (wheel, ahead) in [(ids.wheel_l, 0.0), (ids.wheel_r, 0.9)]
        {
            set(
                wheel,
                Transform::from_translation(placed.axle)
                    .with_rotation(rx((turned + ahead) % TAU)),
            );
        }
        let mut turn = |e: Entity, to: Quat| {
            if let Ok(mut tf) = parts.get_mut(e) {
                tf.rotation = to;
            }
        };
        turn(ids.arm_l, p.arm_l);
        turn(ids.arm_r, p.arm_r);
        turn(ids.lamp, placed.lamp);
        turn(ids.key, rz(p.key));
        if let Some(flag) = ids.flag {
            // The pennant trails, and droops a little.
            turn(flag, ry(p.flag) * rx(-0.12));
        }
        let mut show = |e: Entity, size: f32| {
            let on = size > 0.02;
            if let Ok(mut tf) = parts.get_mut(e) {
                tf.scale = Vec3::splat(size.max(0.02));
            }
            if let Ok(mut v) = visibility.get_mut(e) {
                let want = if on {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                v.set_if_neq(want);
            }
        };
        show(ids.ring, p.ring);
        show(ids.prop, p.prop_size);
        if p.prop != Prop::None && p.prop != keeper.shown {
            let i = p.prop as usize - 1;
            if let Ok(mut mesh) = shapes.get_mut(ids.prop) {
                mesh.0 = keeper.props[i].clone();
            }
            if let Ok(mut mesh) = shapes.get_mut(ids.glint) {
                mesh.0 = keeper.glints[i].clone().unwrap_or_default();
            }
            keeper.shown = p.prop;
        }
        if let Ok(mut tf) = parts.get_mut(ids.glint)
            && tf.translation.y != -p.bead
        {
            tf.translation.y = -p.bead;
        }

        if (p.glow - keeper.lit_as).abs().max_element() > 2e-3 {
            keeper.lit_as = p.glow;
            let strength = p.glow.max_element();
            let lit = (strength / 2.0).clamp(0.0, 1.0);
            if let Some(mut m) = materials.get_mut(&ids.lit) {
                m.emissive =
                    LinearRgba::rgb(p.glow.x, p.glow.y, p.glow.z);
                let c = Vec3::new(0.16, 0.15, 0.14)
                    .lerp(Vec3::new(0.5, 0.46, 0.38), lit);
                m.base_color = Color::srgb(c.x, c.y, c.z);
            }
            if let Some(light) = ids.light
                && let Ok(mut light) = lights.get_mut(light)
            {
                let hue = p.glow / strength.max(1e-4);
                light.color = Color::linear_rgb(hue.x, hue.y, hue.z);
                light.intensity =
                    strength / LIT * sky.look.lantern_lumens;
            }
        }
    }
    if busy {
        redraw.write(RequestRedraw);
    }
}
