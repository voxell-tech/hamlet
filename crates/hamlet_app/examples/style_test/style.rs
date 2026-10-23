//! The style test's scene (the first look test): one stall, a path
//! with the lit groove over a bridge, a cottage, a signal post and
//! the Keeper. Only `examples/style_test` shows it.

mod cottage;
pub mod demo;
mod groove;
mod planting;
pub mod scene;
mod stall;
mod stations;
mod tag;

use bevy::light::{
    CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver,
};
use bevy::pbr::ExtendedMaterial;
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::prelude::*;
use hamlet_realm::keeper::{self, KState, Kit, Who};
use hamlet_realm::lantern::lantern_glass;
use hamlet_realm::needs::TagSettings;
use hamlet_realm::palette::Palette;
use hamlet_realm::pick::{
    self, Glide, PickSettings, Thing, hit_box, watch,
};
use hamlet_realm::sky::{self, CameraSettings, Looks, Sun};
use hamlet_realm::{Clock, Sky, Step, Stock, WorldCamera};
use hamlet_vfx::inlay::{self, InlayExt, InlayMaterial, InlayState};

use self::demo::{Demo, View};
use self::scene::{Places, Terrain};
use self::stations::Stationed;

/// Builds the scene and keeps it in step with [`Demo`]. Add it after
/// Bevy's default plugins. It spawns a `WorldCamera` that draws to
/// the window until the app gives it another target.
pub struct StylePlugin;

impl Plugin for StylePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CameraSettings {
            near: 1.0,
            far: 400.0,
            distance: 28.0,
            pan: 26.0,
            zoom: (0.3, 2.2),
            ..default()
        });
        // The scene of the realm, without its hamlets.
        app.add_plugins(hamlet_realm::scene::plugin);
        app.init_resource::<TagSettings>()
            .init_resource::<Demo>()
            .insert_resource(tag::Anchor(Places::default().knot()))
            .insert_resource(Terrain::new())
            .add_systems(
                Startup,
                (setup.after(sky::spawn), tag::spawn),
            )
            .add_systems(
                Update,
                (
                    (
                        frame_view,
                        respawn_keepers,
                        update_inlay,
                        fixtures,
                    ),
                    stations::walk,
                )
                    .chain()
                    .in_set(Step::Scene),
            )
            .add_systems(
                PostUpdate,
                tag::place.before(bevy::ui::UiSystems::Layout),
            );
        app.add_plugins(pick::plugin)
            .add_systems(Startup, box_cottage)
            .add_systems(Update, box_keeper);
    }
}

#[derive(Component)]
struct SignalArm;

#[derive(Component)]
struct MainKeeper;

#[derive(Component)]
struct LineupKeeper;

/// Handles the systems need after start-up.
#[derive(Resource)]
struct Rig {
    groove: Handle<InlayMaterial>,
    wash: Handle<InlayMaterial>,
    stall_lamp: Handle<StandardMaterial>,
    stall_light: Entity,
    signal_lamp: Handle<StandardMaterial>,
    signal_light: Entity,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut inlays: ResMut<Assets<InlayMaterial>>,
    terrain: Res<Terrain>,
    pal: Res<Palette>,
    looks: Res<Looks>,
    stock: Res<Stock>,
    sun: Single<Entity, With<Sun>>,
) {
    commands.entity(*sun).try_insert(
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            minimum_distance: 4.0,
            first_cascade_far_bound: 34.0,
            maximum_distance: 90.0,
            ..default()
        }
        .build(),
    );

    let (matte, brass) = (stock.matte.clone(), stock.brass.clone());
    let cloth = materials.add(StandardMaterial {
        perceptual_roughness: 1.0,
        reflectance: 0.1,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    // Unculled but not double-sided: both faces take the light as the
    // front does.
    let leaf = materials.add(StandardMaterial {
        perceptual_roughness: 1.0,
        reflectance: 0.1,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    let water = materials.add(StandardMaterial {
        perceptual_roughness: 0.28,
        reflectance: 0.35,
        ..default()
    });
    let iron = materials.add(StandardMaterial {
        perceptual_roughness: 0.7,
        metallic: 0.3,
        ..default()
    });
    let built = scene::build(&terrain, &pal);
    info!(
        "triangles: ground {}, solid {}, cloth {}, leaf {}, tuft \
         {}, water {}",
        built.ground.tris(),
        built.solid.tris(),
        built.cloth.tris(),
        built.leaf.tris(),
        built.tuft.tris(),
        built.water.tris()
    );
    commands.spawn((
        Mesh3d(meshes.add(built.ground.build())),
        MeshMaterial3d(matte.clone()),
        NotShadowCaster,
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.solid.build())),
        MeshMaterial3d(matte.clone()),
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.cloth.build())),
        MeshMaterial3d(cloth.clone()),
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.leaf.build())),
        MeshMaterial3d(leaf.clone()),
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.tuft.build())),
        MeshMaterial3d(leaf),
        NotShadowCaster,
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.water.build())),
        MeshMaterial3d(water),
        NotShadowCaster,
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.brass.build())),
        MeshMaterial3d(brass),
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.iron.build())),
        MeshMaterial3d(iron),
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.window.build())),
        MeshMaterial3d(stock.windows.clone()),
        NotShadowCaster,
    ));

    // The inlay: the groove itself, and the wider ribbon for its
    // night wash.
    let groove = inlays.add(ExtendedMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.85,
            ..default()
        },
        extension: InlayExt::new(
            groove::uniform(
                InlayState::Unlit,
                0.0,
                &terrain,
                0.0,
                1.0,
                &looks,
            ),
            pal.bed,
            pal.pewter,
        ),
    });
    let wash = inlays.add(ExtendedMaterial {
        base: StandardMaterial {
            unlit: true,
            alpha_mode: AlphaMode::Add,
            fog_enabled: false,
            ..default()
        },
        extension: InlayExt::new(
            groove::uniform(
                InlayState::Unlit,
                0.0,
                &terrain,
                0.3,
                1.0,
                &looks,
            ),
            pal.bed,
            pal.pewter,
        ),
    });
    commands.spawn((
        Mesh3d(meshes.add(groove::groove_mesh(&terrain))),
        MeshMaterial3d(groove.clone()),
        NotShadowCaster,
    ));
    commands.spawn((
        Mesh3d(meshes.add(groove::wash_mesh(&terrain))),
        MeshMaterial3d(wash.clone()),
        NotShadowCaster,
        NotShadowReceiver,
    ));

    // The stall's lamp on its hook.
    let stall_lamp = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.46, 0.38),
        perceptual_roughness: 0.4,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(lantern_glass(0.085, 0.2))),
        MeshMaterial3d(stall_lamp.clone()),
        Transform::from_translation(terrain.at.stall_lamp()),
        NotShadowCaster,
    ));
    let stall_light = commands
        .spawn((
            PointLight {
                intensity: 0.0,
                range: 7.0,
                radius: 0.05,
                shadow_maps_enabled: false,
                color: Color::srgb(1.0, 0.82, 0.54),
                ..default()
            },
            Transform::from_translation(
                terrain.at.stall_lamp() + Vec3::new(0.0, -0.05, 0.25),
            ),
        ))
        .id();

    // The signal post's arm and lamp.
    let post = terrain.at.signal_post;
    commands.spawn((
        SignalArm,
        Mesh3d(meshes.add(stall::signal_arm(&pal))),
        MeshMaterial3d(matte.clone()),
        Transform::from_translation(
            post + Vec3::new(0.0, terrain.at.signal_arm_y, 0.09),
        ),
    ));
    let signal_lamp = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.2, 0.12),
        perceptual_roughness: 0.4,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(lantern_glass(0.075, 0.15))),
        MeshMaterial3d(signal_lamp.clone()),
        Transform::from_translation(
            post + Vec3::Y * terrain.at.signal_lamp_y,
        ),
        NotShadowCaster,
    ));
    let signal_light = commands
        .spawn((
            PointLight {
                intensity: 0.0,
                range: 6.0,
                radius: 0.05,
                shadow_maps_enabled: false,
                color: Color::srgb(1.0, 0.54, 0.12),
                ..default()
            },
            Transform::from_translation(
                post + Vec3::new(0.0, terrain.at.signal_lamp_y, 0.2),
            ),
        ))
        .id();

    commands.insert_resource(Rig {
        groove,
        wash,
        stall_lamp,
        stall_light,
        signal_lamp,
        signal_light,
    });
}

/// Focus point, distance and pitch for a view.
fn framing(
    view: View,
    keeper: Vec3,
    at: &Places,
) -> (Vec3, f32, f32) {
    match view {
        View::Default => (Vec3::new(-1.0, 0.5, 0.0), 28.0, 42.0),
        View::Wide => (Vec3::new(-1.0, 0.5, 0.0), 43.0, 42.0),
        View::Keeper => (keeper + Vec3::Y * 0.7, 7.2, 30.0),
        View::Stall => {
            (at.stall + Vec3::new(0.25, 1.1, 0.9), 12.5, 33.0)
        }
        View::Lineup => (at.lineup + Vec3::Y * 0.65, 14.0, 30.0),
    }
}

/// Points the camera at the demo's view, moved by hand through
/// `Orbit`.
fn frame_view(
    mut commands: Commands,
    demo: Res<Demo>,
    orbit: Res<Glide>,
    terrain: Res<Terrain>,
    camera: Single<
        (Entity, &mut Transform, &mut DistanceFog),
        With<WorldCamera>,
    >,
    keeper: Query<
        &Transform,
        (With<MainKeeper>, Without<WorldCamera>),
    >,
) {
    let (entity, mut at, mut fog) = camera.into_inner();

    let keeper_at = keeper
        .iter()
        .next()
        .map(|t| t.translation)
        .unwrap_or(Vec3::ZERO);
    let (focus, dist, pitch) =
        framing(demo.view, keeper_at, &terrain.at);
    let orbit = orbit.0;
    let focus = focus + Vec3::new(orbit.pan.x, 0.0, orbit.pan.y);
    let dist = dist * orbit.zoom;
    let pitch = (pitch.to_radians() + orbit.pitch).clamp(0.25, 1.4);
    let arm = Quat::from_rotation_y(orbit.yaw)
        * Vec3::new(0.0, pitch.sin(), pitch.cos());
    *at = Transform::from_translation(focus + dist * arm)
        .looking_at(focus, Vec3::Y);
    fog.falloff = FogFalloff::Linear {
        start: dist * 1.15,
        end: dist * 1.15 + 62.0,
    };

    // Mild tilt-shift: the pitched camera puts the near and far
    // ground out of focus by itself. Off in the close views,
    // where everything is the subject.
    if demo.dof && matches!(demo.view, View::Default | View::Wide) {
        commands.entity(entity).try_insert(DepthOfField {
            mode: DepthOfFieldMode::Gaussian,
            focal_distance: dist,
            aperture_f_stops: terrain.at.blur_stop,
            max_circle_of_confusion_diameter: 6.0,
            ..default()
        });
    } else {
        commands.entity(entity).try_remove::<DepthOfField>();
    }
}

fn respawn_keepers(
    mut commands: Commands,
    demo: Res<Demo>,
    kit: Res<Kit>,
    terrain: Res<Terrain>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    main: Query<(), With<MainKeeper>>,
    lineup: Query<Entity, With<LineupKeeper>>,
) {
    if main.is_empty() {
        let e = keeper::spawn_as(
            &mut commands,
            &kit,
            &mut materials,
            Who::leader(),
            Transform::default(),
            demo.kstate,
        );
        commands
            .entity(e)
            .try_insert((MainKeeper, Stationed::default()));
    }

    let wanted = if demo.lineup { KState::ALL.len() } else { 0 };
    let right = lineup.iter().count() == wanted;
    if !right {
        for entity in &lineup {
            commands.entity(entity).try_despawn();
        }
        for (i, state) in
            KState::ALL.into_iter().enumerate().take(wanted)
        {
            let at =
                terrain.at.lineup + Vec3::X * (i as f32 - 2.0) * 1.25;
            let at = at.with_y(terrain.ground_y(at.x, at.z));
            let turn =
                Quat::from_rotation_y(if state == KState::Working {
                    0.9
                } else {
                    0.25
                });
            let e = keeper::spawn_as(
                &mut commands,
                &kit,
                &mut materials,
                Who::leader(),
                Transform::from_translation(at).with_rotation(turn),
                state,
            );
            commands.entity(e).try_insert(LineupKeeper);
        }
    }
}

fn update_inlay(
    clock: Res<Clock>,
    demo: Res<Demo>,
    sky: Res<Sky>,
    looks: Res<Looks>,
    rig: Res<Rig>,
    terrain: Res<Terrain>,
    mut inlays: ResMut<Assets<InlayMaterial>>,
) {
    let look = &sky.look;
    if let Some(mut m) = inlays.get_mut(&rig.groove) {
        m.extension.u = groove::uniform(
            demo.inlay,
            clock.t,
            &terrain,
            0.0,
            look.inlay_gain,
            &looks,
        );
    }
    if let Some(mut m) = inlays.get_mut(&rig.wash) {
        // The wash is nothing by day: a zero strength would read as
        // "groove", so keep it tiny.
        m.extension.u = groove::uniform(
            demo.inlay,
            clock.t,
            &terrain,
            look.wash.max(1e-4),
            1.0,
            &looks,
        );
    }
}

/// The signal post and the stall's lamp.
fn fixtures(
    clock: Res<Clock>,
    demo: Res<Demo>,
    sky: Res<Sky>,
    rig: Res<Rig>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut arm: Single<&mut Transform, With<SignalArm>>,
    mut lights: Query<&mut PointLight>,
) {
    let look = &sky.look;
    let waiting = demo.inlay == InlayState::Signal;
    // Horizontal and pointing at the stall while it waits, hanging
    // otherwise.
    arm.rotation = Quat::from_rotation_y(std::f32::consts::PI)
        * Quat::from_rotation_z(if waiting { 0.0 } else { 1.25 });

    let breath = 0.8 + 0.2 * (clock.t * std::f32::consts::PI).sin();
    let signal = if waiting {
        inlay::signal() * inlay::LIT * breath
    } else {
        Vec3::ZERO
    };
    if let Some(mut m) = materials.get_mut(&rig.signal_lamp) {
        m.emissive = LinearRgba::rgb(signal.x, signal.y, signal.z);
    }
    if let Ok(mut l) = lights.get_mut(rig.signal_light) {
        l.intensity = if waiting {
            look.lantern_lumens * breath
        } else {
            0.0
        };
    }

    let candle = inlay::candle() * inlay::EMBER * 1.6 * look.candle;
    if let Some(mut m) = materials.get_mut(&rig.stall_lamp) {
        m.emissive = LinearRgba::rgb(candle.x, candle.y, candle.z);
    }
    if let Ok(mut l) = lights.get_mut(rig.stall_light) {
        l.intensity = look.lantern_lumens * 1.4 * look.candle;
    }
}

/// The room the cottage takes for the pointer.
fn box_cottage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    terrain: Res<Terrain>,
) {
    let size = Vec3::new(4.2, 4.4, 3.8);
    let middle = terrain.at.cottage + Vec3::Y * size.y * 0.5;
    let boxed = commands
        .spawn(hit_box(Thing::Cottage, size, middle, &mut meshes))
        .id();
    watch(&mut commands, boxed);
}

#[derive(Component)]
struct Boxed;

/// A robot's hit box rides on it.
fn box_keeper(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    pick: Res<PickSettings>,
    mayors: Query<Entity, (With<MainKeeper>, Without<Boxed>)>,
) {
    for robot in &mayors {
        let boxed = commands
            .spawn((
                hit_box(
                    Thing::Keeper,
                    pick.robot_box,
                    Vec3::Y * pick.robot_box.y * 0.5,
                    &mut meshes,
                ),
                ChildOf(robot),
            ))
            .id();
        watch(&mut commands, boxed);
        commands.entity(robot).try_insert(Boxed);
    }
}
