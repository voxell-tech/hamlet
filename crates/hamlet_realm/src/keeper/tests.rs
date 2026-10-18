use std::f32::consts::TAU;
use std::time::Instant;

use bevy::ecs::system::RunSystemOnce;
use bevy::window::RequestRedraw;

use super::acts::{Ctx, Quirk, pose};
use super::pose::{Pose, ramp};
use super::trip::turn;
use super::*;
use crate::generator::DetailSettings;

fn ctx(t: f32, seed: u32) -> Ctx {
    Ctx {
        t,
        tau: t,
        gait: t * 8.0,
        pace: 1.0,
        // Stepping and rolling by turns.
        roll: ramp(0.3, 0.7, (t * 0.9).sin()),
        q: Quirk::of(seed),
    }
}

fn finite(p: &Pose) -> bool {
    let placed = p.place(1.0);
    [
        placed.rig,
        placed.body,
        placed.head,
        placed.hat,
        placed.eye_l,
        placed.eye_r,
        placed.leg_l,
        placed.leg_r,
        placed.pole,
    ]
    .iter()
    .all(|t| {
        t.translation.is_finite()
            && t.rotation.is_finite()
            && t.scale.is_finite()
    }) && placed.lamp.is_finite()
        && p.glow.is_finite()
        && p.glow.min_element() >= 0.0
}

#[test]
fn every_motion_is_finite_at_every_moment() {
    for act in Act::ALL {
        for i in 0..600 {
            let p = pose(act, &ctx(i as f32 * 0.021, 7));
            assert!(finite(&p), "{act:?} at step {i}");
        }
    }
}

#[test]
fn feet_stay_out_of_the_ground() {
    for act in Act::ALL {
        for i in 0..400 {
            let p = pose(act, &ctx(i as f32 * 0.03, 3));
            let placed = p.place(1.0);
            for leg in [placed.leg_l, placed.leg_r] {
                let pitch = leg.rotation.to_euler(EulerRot::XYZ).0;
                let sole =
                    leg.translation.y - pose::LEG * pitch.cos();
                assert!(sole > -1e-3, "{act:?} at step {i}");
            }
        }
    }
}

#[test]
fn a_rolling_robot_holds_its_legs_still() {
    let legs = |gait: f32| {
        let p = pose(
            Act::Walk,
            &Ctx {
                gait,
                roll: 1.0,
                ..ctx(1.0, 0)
            },
        );
        (p.leg_l, p.leg_r, p.lift_l, p.lift_r)
    };
    assert_eq!(legs(0.4), legs(2.9));
}

#[test]
fn a_trip_starts_still_and_ends_at_its_last_point_and_no_further() {
    let trip = Trip::new(
        vec![
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.5, 0.0, 2.0),
        ],
        10.0,
        &Motion::default(),
    );
    let start = trip.at(10.0);
    assert_eq!((start.gone, start.pace), (0.0, 0.0));
    let mut last = 0.0;
    for i in 0..200 {
        let s = trip.at(10.0 + i as f32 * 0.05);
        assert!(s.gone >= last - 1e-5);
        last = s.gone;
    }
    let end = trip.at(10.0 + trip.duration());
    assert!(end.at.distance(Vec3::new(1.5, 0.0, 2.0)) < 1e-3);
    assert!(end.pace.abs() < 1e-3);
    assert!(trip.arrived(10.0 + trip.duration()));

    // One too short to get going never overshoots.
    let short = Trip::new(
        vec![Vec3::ZERO, Vec3::new(0.1, 0.0, 0.0)],
        0.0,
        &Motion::default(),
    );
    for i in 0..50 {
        assert!(short.at(i as f32 * 0.05).gone <= 0.1 + 1e-5);
    }
}

#[test]
fn a_turn_takes_the_short_way_round() {
    assert!((turn(3.0, -3.0, 0.5).abs() - TAU / 2.0).abs() < 0.2);
    assert!((turn(0.2, 0.6, 0.5) - 0.4).abs() < 1e-5);
}

fn app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_message::<RequestRedraw>()
        .init_resource::<crate::Clock>()
        .init_resource::<crate::Stage>()
        .init_resource::<crate::Sky>()
        .init_resource::<DetailSettings>()
        .init_resource::<Motion>()
        .add_systems(Update, animate);
    let world = app.world_mut();
    crate::stock(world);
    let id =
        world
            .run_system_once(
                |mut commands: Commands,
                 kit: Res<Kit>,
                 mut materials: ResMut<
                    Assets<StandardMaterial>,
                >| {
                    spawn_agent(
                        &mut commands,
                        &kit,
                        &mut materials,
                        Who::leader(),
                        Transform::default(),
                        None,
                    )
                },
            )
            .unwrap();
    (app, id)
}

/// Runs `secs` of frames and returns the furthest the robot
/// moved in one of them.
fn run(app: &mut App, id: Entity, secs: f32) -> f32 {
    let mut furthest = 0.0f32;
    let at = |app: &App| {
        app.world().get::<Transform>(id).unwrap().translation
    };
    for _ in 0..(secs * 60.0) as usize {
        let before = at(app);
        app.world_mut().resource_mut::<crate::Clock>().t +=
            1.0 / 60.0;
        app.update();
        furthest = furthest.max(before.distance(at(app)));
    }
    furthest
}

fn set(app: &mut App, id: Entity, state: KState) {
    app.world_mut().get_mut::<Keeper>(id).unwrap().state = state;
}

#[test]
fn a_frozen_clock_shows_the_settled_state() {
    let (mut app, id) = app();
    app.world_mut().resource_mut::<crate::Clock>().frozen = true;
    app.update();
    set(&mut app, id, KState::Failed);
    app.update();
    let keeper = app.world().get::<Keeper>(id).unwrap();
    assert!(!keeper.busy(0.0));
    assert_eq!(keeper.eval(0.0).glow, Vec3::ZERO);
}

/// Prints what a robot costs. Run with
/// `cargo test --release keeper::tests::cost -- --ignored
/// --nocapture`.
#[test]
#[ignore]
fn cost() {
    const N: usize = 2_000_000;
    let started = Instant::now();
    let mut sum = 0.0;
    for i in 0..N {
        let act = Act::ALL[i % Act::ALL.len()];
        let p = pose(act, &ctx(i as f32 * 0.0007, i as u32 % 64));
        let placed = p.place(1.0);
        sum += placed.pole.translation.x + p.glow.x;
    }
    let each = started.elapsed().as_nanos() as f64 / N as f64;
    println!("pose and placement: {each:.0} ns per robot ({sum})");
    println!(
        "Keeper {} bytes, Pose {} bytes",
        size_of::<Keeper>(),
        size_of::<Pose>()
    );

    // The whole system over a town, every robot in motion.
    const TOWN: usize = 192;
    let (mut app, id) = app();
    app.world_mut()
        .run_system_once(
            |mut commands: Commands,
             kit: Res<Kit>,
             mut materials: ResMut<Assets<StandardMaterial>>| {
                for i in 0..TOWN {
                    spawn_agent(
                        &mut commands,
                        &kit,
                        &mut materials,
                        Who::citizen(i as u32 + 1),
                        Transform::from_xyz(i as f32, 0.0, 0.0),
                        None,
                    );
                }
            },
        )
        .unwrap();
    let acts = [Act::Walk, Act::Talk, Act::Hammer, Act::Wait];
    let mut keepers = app.world_mut().query::<&mut Keeper>();
    for (i, mut k) in keepers.iter_mut(app.world_mut()).enumerate() {
        k.play(acts[i % acts.len()], 0.0);
    }
    run(&mut app, id, 1.0);
    const FRAMES: usize = 2000;
    let started = Instant::now();
    run(&mut app, id, FRAMES as f32 / 60.0);
    let each = started.elapsed().as_nanos() as f64
        / (FRAMES * (TOWN + 1)) as f64;
    println!("animate, with its writes: {each:.0} ns per robot");
}
