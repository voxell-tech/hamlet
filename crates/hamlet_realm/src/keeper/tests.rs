use std::f32::consts::TAU;
use std::time::Instant;

use bevy::ecs::system::RunSystemOnce;
use bevy::window::RequestRedraw;

use super::acts::{Context, Quirk, pose};
use super::pose::{Pose, ramp};
use super::trip::turn;
use super::*;
use crate::generator::DetailSettings;

fn context(t: f32, seed: u32) -> Context {
    Context {
        t,
        tau: t,
        gait: t * 8.0,
        pace: 1.0,
        // Stepping and rolling by turns.
        roll: ramp(0.3, 0.7, (t * 0.9).sin()),
        q: Quirk::of(seed),
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
    crate::stock::stock(world);
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
                        Who::mayor(),
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

/// Prints what a robot costs. Run with
/// `cargo test --release keeper::tests::cost -- --ignored
/// --nocapture`.
#[test]
#[ignore]
#[expect(
    clippy::print_stdout,
    reason = "it prints its measurements, read with --nocapture"
)]
fn cost() {
    const N: usize = 2_000_000;
    let started = Instant::now();
    let mut sum = 0.0;
    for i in 0..N {
        let act = Act::ALL[i % Act::ALL.len()];
        let p = pose(act, &context(i as f32 * 0.0007, i as u32 % 64));
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
                        Who::agent(i as u32 + 1),
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
