//! The app's scene with Bevy linked in and no plugins, for
//! comparison.
//!
//! `HAMLET_SPIKE_FRAMES` closes it after that many frames.

#![expect(
    clippy::print_stdout,
    reason = "the spike reports on stdout"
)]

use std::time::Instant;

use bevy::prelude::*;

#[derive(Resource)]
struct Started(Instant);

fn main() {
    let started = Instant::now();
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(Started(started))
        .add_systems(Startup, scene)
        .add_systems(Update, (first_frame, finish))
        .run();
}

fn scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 3.0, 6.0)
            .looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(3.0, 6.0, 2.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(8.0, 8.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
    ));
}

fn first_frame(started: Res<Started>, mut done: Local<bool>) {
    if !*done {
        *done = true;
        println!("first update at {:?}", started.0.elapsed());
    }
}

fn finish(mut frames: Local<u32>, mut exit: MessageWriter<AppExit>) {
    let Some(limit) = std::env::var("HAMLET_SPIKE_FRAMES")
        .ok()
        .and_then(|frames| frames.parse::<u32>().ok())
    else {
        return;
    };
    *frames += 1;
    if *frames >= limit {
        exit.write(AppExit::Success);
    }
}
