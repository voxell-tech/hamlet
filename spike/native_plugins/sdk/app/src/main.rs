//! A window that loads every plugin library in `HAMLET_SPIKE_PLUGINS`
//! and draws a scene for them.
//!
//! `HAMLET_SPIKE_FRAMES` closes it after that many frames, and
//! `HAMLET_SPIKE_SHOT` saves a screenshot to that path first.

#![expect(unsafe_code, reason = "a plugin is a native library")]
#![expect(
    clippy::print_stdout,
    reason = "the spike reports on stdout"
)]

use std::any::TypeId;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::tasks::ComputeTaskPool;
use hamlet_sdk::{ENTRY, Entry, LOADS, Probes};

#[derive(Resource)]
struct Started(Instant);

fn main() {
    let started = Instant::now();
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
        .init_resource::<Probes>()
        .insert_resource(Started(started))
        .add_systems(Startup, scene)
        .add_systems(Update, (first_frame, report, finish));
    let built = started.elapsed();
    if let Some(folder) = std::env::var_os("HAMLET_SPIKE_PLUGINS") {
        for path in libraries(Path::new(&folder)) {
            let loading = Instant::now();
            match load(&mut app, &path) {
                Ok(()) => println!(
                    "loaded {} in {:?}",
                    path.display(),
                    loading.elapsed()
                ),
                Err(error) => {
                    println!("failed {}: {error}", path.display())
                }
            }
        }
    }
    println!(
        "app built in {built:?}, plugins loaded at {:?}",
        started.elapsed()
    );
    app.run();
}

fn libraries(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension().is_some_and(|extension| {
                extension == std::env::consts::DLL_EXTENSION
            })
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn load(app: &mut App, path: &Path) -> Result<(), libloading::Error> {
    // SAFETY: a plugin is built by the same compiler against the same
    // `hamlet_sdk`, so `Entry` means the same on both sides.
    let library = unsafe { libloading::Library::new(path)? };
    // SAFETY: as above.
    let entry = unsafe { library.get::<Entry>(ENTRY)? };
    entry(app);
    // A Rust library cannot be closed once it has registered systems,
    // types and statics, so it stays loaded until the process ends.
    std::mem::forget(library);
    Ok(())
}

fn scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    LOADS.fetch_add(1, Ordering::SeqCst);
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

fn report(
    probes: Res<Probes>,
    registry: Res<AppTypeRegistry>,
    mut done: Local<bool>,
) {
    if *done || probes.0.is_empty() {
        return;
    }
    *done = true;
    println!(
        "app: loads {} at {:p}, Transform {:?}, compute pool at {:p}",
        LOADS.load(Ordering::SeqCst),
        &LOADS,
        TypeId::of::<Transform>(),
        ComputeTaskPool::get(),
    );
    for probe in &probes.0 {
        println!(
            "{}: loads {} at {:#x}, same Transform type: {}, same \
             compute pool: {}",
            probe.plugin,
            probe.loads,
            probe.loads_address,
            probe.transform_type == TypeId::of::<Transform>(),
            probe.compute_pool_address
                == std::ptr::from_ref(ComputeTaskPool::get())
                    as usize,
        );
    }
    let registry = registry.read();
    let spin = registry.get_with_short_type_path("Spin").is_some();
    println!(
        "app sees the plugin's Spin in its type registry: {spin}"
    );
}

fn finish(
    mut commands: Commands,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(limit) = std::env::var("HAMLET_SPIKE_FRAMES")
        .ok()
        .and_then(|frames| frames.parse::<u32>().ok())
    else {
        return;
    };
    *frames += 1;
    if *frames == limit.saturating_sub(10)
        && let Some(path) = std::env::var_os("HAMLET_SPIKE_SHOT")
    {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(PathBuf::from(path)));
    }
    if *frames >= limit {
        exit.write(AppExit::Success);
    }
}
