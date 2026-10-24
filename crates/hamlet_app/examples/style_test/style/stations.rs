//! The style test's own robot: its station for each state, and its
//! walks between them.

use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_realm::Clock;
use hamlet_realm::keeper::{
    Act, Keeper, KeeperState, Motion, Stride, Trip,
};

use super::demo::Demo;
use super::scene::Terrain;

/// A robot that keeps the stations of the style test's scene.
#[derive(Component)]
pub struct Stationed {
    /// The state it was last placed for.
    state: Option<KeeperState>,
    /// A walk to its next station, and the motion it plays there.
    errand: Option<(Trip, Act)>,
    /// The spot it stopped at, when a state holds it in place.
    spot: Option<(Vec3, f32)>,
    /// Clock time it took its station up.
    since: f32,
    /// Clock time it was last placed.
    seen: f32,
    /// Its walk, as last set.
    stride: Stride,
}

impl Default for Stationed {
    fn default() -> Self {
        Self {
            state: None,
            errand: None,
            spot: None,
            since: 0.0,
            seen: f32::NAN,
            stride: Stride::default(),
        }
    }
}

/// The robot's place and facing for `state`, and the distance it has
/// walked if the state keeps it walking. `x` is seconds since it took
/// the state up.
fn station(
    state: KeeperState,
    x: f32,
    terrain: &Terrain,
    motion: &Motion,
) -> (Vec3, f32, Option<f32>) {
    let total = terrain.path.total();
    let ground =
        |x: f32, z: f32| Vec3::new(x, terrain.ground_y(x, z), z);
    match state {
        KeeperState::Rest | KeeperState::Failed => {
            (ground(2.05, -1.55), 0.35, None)
        }
        KeeperState::Working => {
            // Out to the far end and back, turning at each.
            let (far, near) = (4.0, total - 1.0);
            let span = near - far;
            let walked = x.max(0.0) * motion.walk_speed;
            let k = walked % (2.0 * span);
            let (s, out) = if k < span {
                (near - k, true)
            } else {
                (far + (k - span), false)
            };
            let (p, tan) = terrain.on_path(s);
            let tan = if out { -tan } else { tan };
            (p, tan.x.atan2(tan.z), Some(walked))
        }
        KeeperState::Waiting => (
            ground(
                terrain.at.signal_post.x - 0.85,
                terrain.at.signal_post.z + 0.45,
            ),
            0.0,
            None,
        ),
        KeeperState::Fault => {
            let (p, tan) = terrain.on_path(total - 3.2);
            (p, tan.x.atan2(tan.z) + 2.4, None)
        }
    }
}

/// The way from one place to another: by the path where the path
/// leads there, and over the grass otherwise.
fn route(from: Vec3, to: Vec3, terrain: &Terrain) -> Vec<Vec3> {
    let pace = 0.4;
    let mut way = vec![from];
    let (off, s0) = terrain.path.nearest(from.x, from.z);
    let (_, s1) = terrain.path.nearest(to.x, to.z);
    if off < 0.6 && (s1 - s0).abs() > 2.0 {
        let n = ((s1 - s0).abs() / pace).ceil() as usize;
        way.extend((1..=n).map(|i| {
            terrain.on_path(s0 + (s1 - s0) * i as f32 / n as f32).0
        }));
    }
    let last = way[way.len() - 1];
    let n = (last.distance(to) / pace).ceil().max(1.0) as usize;
    way.extend((1..n).map(|i| {
        let p = last.lerp(to, i as f32 / n as f32);
        Vec3::new(p.x, terrain.ground_y(p.x, p.z), p.z)
    }));
    way.push(to);
    way
}

/// Takes up the demo's state: at once where it stands for a fault,
/// after a walk to the state's station otherwise.
fn take_up(
    keeper: &mut Keeper,
    stationed: &mut Stationed,
    root: &Transform,
    state: KeeperState,
    now: f32,
    frozen: bool,
    terrain: &Terrain,
    motion: &Motion,
) {
    let first = stationed.state.is_none();
    let act = Act::of(state);
    stationed.state = Some(state);
    stationed.errand = None;
    stationed.spot = None;
    // A frozen clock shows the settled moment.
    if frozen || first {
        keeper.settle(act);
        stationed.since = 0.0;
        return;
    }
    stationed.since = now;
    if matches!(state, KeeperState::Fault | KeeperState::Failed) {
        // It breaks down where it stands.
        let facing = root.rotation.to_euler(EulerRot::YXZ).0;
        stationed.spot = Some((root.translation, facing));
        keeper.play(act, now);
        return;
    }
    let (to, ..) = station(state, 0.0, terrain, motion);
    if root.translation.distance(to) < 0.15 {
        keeper.play(act, now);
        return;
    }
    let mut trip =
        Trip::new(route(root.translation, to, terrain), now, motion);
    trip.speed *= 1.5;
    keeper.play(Act::Walk, now);
    stationed.errand = Some((trip, act));
}

/// Moves each stationed robot to where the demo's state has it.
pub fn walk(
    clock: Res<Clock>,
    demo: Res<Demo>,
    terrain: Res<Terrain>,
    motion: Res<Motion>,
    mut robots: Query<(&mut Keeper, &mut Transform, &mut Stationed)>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let t = clock.t;
    for (mut keeper, mut root, mut stationed) in &mut robots {
        let (keeper, stationed) = (&mut *keeper, &mut *stationed);
        if stationed.state != Some(demo.keeper_state) {
            take_up(
                keeper,
                stationed,
                &root,
                demo.keeper_state,
                t,
                clock.frozen,
                &terrain,
                &motion,
            );
        }
        let (at, yaw) = if let Some((trip, _)) = &stationed.errand {
            let s = trip.at(t);
            stationed.stride = s.stride();
            (s.at, s.yaw)
        } else if let Some(spot) = stationed.spot {
            spot
        } else {
            let (at, yaw, walked) = station(
                demo.keeper_state,
                t - stationed.since,
                &terrain,
                &motion,
            );
            stationed.stride = match walked {
                Some(gone) => Stride {
                    gone,
                    pace: 1.0,
                    speed: motion.walk_speed,
                },
                None => Stride {
                    gone: stationed.stride.gone,
                    ..default()
                },
            };
            (at, yaw)
        };
        keeper.stride(Some(stationed.stride));
        root.translation = at;
        // Turns take a moment, unless the clock jumped.
        let dt = t - stationed.seen;
        stationed.seen = t;
        let to = Quat::from_rotation_y(yaw);
        root.rotation = if dt > 0.0 && dt < 0.5 {
            root.rotation.slerp(to, 1.0 - (-dt * 10.0).exp())
        } else {
            to
        };
        if let Some((trip, then)) = &stationed.errand {
            if trip.has_arrived(t) {
                let then = *then;
                stationed.errand = None;
                stationed.since = t;
                keeper.play(then, t);
            } else {
                redraw.write(RequestRedraw);
            }
        }
    }
}
