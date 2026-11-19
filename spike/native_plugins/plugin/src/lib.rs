//! A plugin that spins a cube drawn with its own material and shader,
//! and shows a label in Hamlet's theme.

use std::sync::atomic::Ordering;

use hamlet_sdk::bevy::asset::embedded_asset;
use hamlet_sdk::bevy::prelude::*;
use hamlet_sdk::bevy::render::render_resource::AsBindGroup;
use hamlet_sdk::bevy::shader::ShaderRef;
use hamlet_sdk::bevy::tasks::ComputeTaskPool;
use hamlet_sdk::bevy_fynix::mount;
use hamlet_sdk::bevy_fynix::views::{column, label};
use hamlet_sdk::hamlet_ui::theme::HamletTheme;
use hamlet_sdk::{LOADS, Probe, Probes};

/// The plugin's entry, which the window calls once at start.
#[unsafe(no_mangle)]
pub fn hamlet_plugin(app: &mut App) {
    app.add_plugins(PulsePlugin);
}

struct PulsePlugin;

impl Plugin for PulsePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "pulse.wesl");
        app.add_plugins(MaterialPlugin::<PulseMaterial>::default())
            .register_type::<Spin>()
            .add_systems(Startup, (spawn, probe, panel))
            .add_systems(Update, spin);
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
struct PulseMaterial {
    #[uniform(0)]
    low: LinearRgba,
    #[uniform(0)]
    high: LinearRgba,
}

impl Material for PulseMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://pulse/pulse.wesl".into()
    }
}

#[derive(Component, Reflect)]
struct Spin(f32);

fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PulseMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.5, 1.5, 1.5))),
        MeshMaterial3d(materials.add(PulseMaterial {
            low: LinearRgba::rgb(0.1, 0.0, 0.4),
            high: LinearRgba::rgb(1.0, 0.5, 0.1),
        })),
        Transform::from_xyz(0.0, 1.0, 0.0),
        Spin(1.2),
    ));
}

fn spin(
    time: Res<Time>,
    mut spinning: Query<(&Spin, &mut Transform)>,
) {
    for (spin, mut transform) in &mut spinning {
        transform.rotate_y(spin.0 * time.delta_secs());
    }
}

/// A label in Hamlet's theme, built by fynix against the theme the
/// window's `hamlet_ui` set up.
fn panel(world: &mut World) {
    mount::<HamletTheme>(
        world,
        column((label(
            "pulse: a label from the plugin, in hamlet_ui",
        ),)),
    );
}

fn probe(mut probes: ResMut<Probes>) {
    LOADS.fetch_add(1, Ordering::SeqCst);
    info!("pulse: hello through the app's logger");
    probes.0.push(Probe {
        plugin: String::from("pulse"),
        loads: LOADS.load(Ordering::SeqCst),
        loads_address: std::ptr::from_ref(&LOADS) as usize,
        transform_type: std::any::TypeId::of::<Transform>(),
        compute_pool_address: std::ptr::from_ref(
            ComputeTaskPool::get(),
        ) as usize,
    });
}
