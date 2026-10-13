//! A hamlet's work where it can be seen: on each common a lever for
//! its mode and an arch for its folder.

use std::f32::consts::PI;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::Face;
use bevy::window::RequestRedraw;
use hamlet_core::agents::Works;
use hamlet_rules::work::Mode;
use hamlet_vfx::inlay;

use crate::generator::Common;
use crate::generator::land::Land;
use crate::hamlet::Sited;
use crate::lantern::{lantern_frame, lantern_glass};
use crate::meshes::{Mb, Rng};
use crate::palette::Palette;
use crate::pick::{self, Named, Reads, Thing, hit_box};
use crate::play::{Play, Played};
use crate::{Sky, Stock, WorldSystems, adopt};

/// The lever and the arch of a common.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorksSettings {
    /// The places of the lever and of the arch on the common, in the
    /// town's frame. Each faces the middle.
    pub lever_at: Vec2,
    pub arch_at: Vec2,
    pub lever_size: f32,
    /// Radians the lever leans from upright in either mode, and the
    /// rate it is thrown at.
    pub throw: f32,
    pub throw_rate: f32,
    /// Brightness of the lamp and of the veil.
    pub lamp_glow: f32,
    pub veil_glow: f32,
    /// The share of what stands behind the veil that it hides.
    pub veil_cover: f32,
    /// The light the lamp throws after dark, and how far.
    pub lumens: f32,
    pub reach: f32,
}

/// The chief pressed the lever of `hamlet`.
#[derive(Event, Clone, Copy, Debug)]
pub struct Pulled {
    pub hamlet: Entity,
}

/// On a common that has its lever and its arch.
#[derive(Component)]
struct Fitted;

/// The arm of a hamlet's lever.
#[derive(Component)]
struct Lever {
    hamlet: Entity,
    /// Radians it leans from upright now.
    lean: f32,
}

/// The lamp over a hamlet's lever.
#[derive(Component)]
struct Beacon {
    hamlet: Entity,
}

/// The light in a hamlet's arch.
#[derive(Component)]
struct Veil {
    hamlet: Entity,
}

/// The part of a hamlet's arch that answers the pointer and is named
/// for the folder.
#[derive(Component)]
struct Doorway {
    hamlet: Entity,
}

/// The meshes and lights of the levers and the arches.
#[derive(Resource)]
struct Fittings {
    stand: Handle<Mesh>,
    arm: Handle<Mesh>,
    glass: Handle<Mesh>,
    arch: Handle<Mesh>,
    veil: Handle<Mesh>,
    /// The lamp's glass with no folder.
    dark: Handle<StandardMaterial>,
    /// The lamp's glass and the veil in each mode, manual first.
    lamps: [Handle<StandardMaterial>; 2],
    veils: [Handle<StandardMaterial>; 2],
    matte: Handle<StandardMaterial>,
    brass: Handle<StandardMaterial>,
}

/// Height of the arm's pivot above the paving, and the length of the
/// arm.
const PIVOT: f32 = 0.62;
const ARM: f32 = 0.82;

/// The middle of the lamp's glass, in the stand's frame.
const LAMP: Vec3 = Vec3::new(0.0, 1.78, -0.3);

/// Height at which the arch springs from its piers, the radius of
/// its ring of stones, and half the width of its opening.
const SPRING: f32 = 1.4;
const RING: f32 = 0.8;
const OPENING: f32 = 0.6;

const MODES: [Mode; 2] = [Mode::Manual, Mode::Auto];

/// The colour a mode is shown in: the orange of what waits for the
/// chief, and the blue of what runs.
fn hue(mode: Mode) -> Vec3 {
    match mode {
        Mode::Manual => inlay::signal(),
        Mode::Auto => inlay::current(),
    }
}

fn index(mode: Mode) -> usize {
    MODES.iter().position(|&m| m == mode).unwrap_or(0)
}

/// The mode a hamlet works in. None while it works on no folder.
fn mode(works: Option<&Works>) -> Option<Mode> {
    works.filter(|w| w.0.folder.is_some()).map(|w| w.0.mode)
}

/// Radians the lever leans for `mode`: toward its orange plate for
/// manual, toward its blue one for auto, upright with no folder.
fn lean(mode: Option<Mode>, throw: f32) -> f32 {
    match mode {
        None => 0.0,
        Some(Mode::Manual) => throw,
        Some(Mode::Auto) => -throw,
    }
}

/// The lever's stand: a stone foot, a box for the pivot with a plate
/// of each mode's colour at the end the arm leans to, and a lantern
/// on a post behind.
fn stand(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    mb.lathe(
        Vec3::ZERO,
        &[
            (0.0, 0.0),
            (0.5, 0.0),
            (0.5, 0.12),
            (0.4, 0.16),
            (0.4, 0.3),
            (0.0, 0.3),
        ],
        12,
        |r, _| {
            if r < 2 {
                pal.limestone_shade
            } else {
                pal.limestone
            }
        },
    );
    mb.rbox(
        Vec3::Y * 0.5,
        Vec3::new(0.3, 0.2, 0.11),
        0.03,
        Quat::IDENTITY,
        pal.timber_dark,
        0.2,
    );
    for side in [-1.0f32, 1.0] {
        mb.rbox(
            Vec3::new(0.0, 0.56, side * 0.12),
            Vec3::new(0.33, 0.1, 0.015),
            0.008,
            Quat::IDENTITY,
            pal.brass,
            0.1,
        );
    }
    mb.tube(
        Vec3::new(0.0, PIVOT, -0.17),
        Vec3::new(0.0, PIVOT, 0.17),
        0.055,
        0.055,
        8,
        pal.brass * 1.2,
        true,
    );
    // Dulled, so that only the lamp is lit.
    for (side, mode) in [(-1.0f32, Mode::Manual), (1.0, Mode::Auto)] {
        mb.rbox(
            Vec3::new(side * 0.37, 0.72, 0.0),
            Vec3::new(0.035, 0.1, 0.1),
            0.015,
            Quat::from_rotation_z(-side * 0.5),
            hue(mode) * 0.45,
            0.0,
        );
    }
    let foot = Vec3::new(LAMP.x, 0.3, LAMP.z);
    mb.tube(
        foot,
        LAMP - Vec3::Y * 0.16,
        0.03,
        0.022,
        6,
        pal.bed,
        false,
    );
    lantern_frame(&mut mb, LAMP, 0.11, 0.26, pal.brass);
    mb.build()
}

/// The lever's arm with its knob, from the pivot up.
fn arm(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    mb.tube(
        Vec3::ZERO,
        Vec3::Y * ARM,
        0.04,
        0.03,
        8,
        pal.brass,
        false,
    );
    mb.blob(
        Vec3::Y * (ARM + 0.06),
        Vec3::splat(0.11),
        6,
        0.0,
        0.0,
        |_| pal.brass * 1.3,
    );
    mb.build()
}

/// The arch: two piers of three blocks, a ring of stones over them
/// with a brass roundel on the keystone, and a threshold.
fn arch(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    let mut rng = Rng::seeded("arch");
    let stone = |rng: &mut Rng| {
        pal.limestone.lerp(pal.limestone_shade, rng.f() * 0.5)
            * rng.value(0.05)
    };
    mb.rbox(
        Vec3::Y * 0.03,
        Vec3::new(1.15, 0.04, 0.38),
        0.02,
        Quat::IDENTITY,
        pal.limestone_shade,
        0.3,
    );
    let block = SPRING / 3.0;
    for side in [-1.0f32, 1.0] {
        for row in 0..3 {
            let col = stone(&mut rng);
            mb.rbox(
                Vec3::new(
                    side * RING + rng.sym(0.015),
                    block * (row as f32 + 0.5),
                    0.0,
                ),
                Vec3::new(0.21, block * 0.5 - 0.008, 0.26),
                0.03,
                rng.wobble(1.0),
                col,
                if row == 0 { 0.3 } else { 0.05 },
            );
        }
    }
    let stones = 7;
    for k in 0..stones {
        let around = PI * (k as f32 + 0.5) / stones as f32;
        let key = k == stones / 2;
        let (out, deep) = if key { (0.26, 0.3) } else { (0.2, 0.27) };
        let col = if key { pal.limestone } else { stone(&mut rng) };
        mb.rbox(
            Vec3::new(
                around.cos() * (RING + out - 0.2),
                SPRING + around.sin() * (RING + out - 0.2),
                0.0,
            ),
            Vec3::new(out, RING * PI / stones as f32 * 0.5, deep),
            0.03,
            Quat::from_rotation_z(around),
            col,
            0.05,
        );
    }
    for side in [-1.0f32, 1.0] {
        mb.rbox(
            Vec3::new(0.0, SPRING + RING + 0.06, side * 0.3),
            Vec3::new(0.09, 0.09, 0.015),
            0.008,
            Quat::from_rotation_z(PI * 0.25),
            pal.brass * 1.2,
            0.0,
        );
    }
    mb.build()
}

/// The light that fills the arch's opening, seen from both sides.
fn veil() -> Mesh {
    let mut mb = Mb::new();
    let at = |x: f32, y: f32| Vec3::new(x, y, 0.0);
    let turns = 12;
    let mut rim = vec![at(-OPENING, 0.07), at(OPENING, 0.07)];
    rim.extend((0..=turns).map(|k| {
        let around = PI * k as f32 / turns as f32;
        at(around.cos() * OPENING, SPRING + around.sin() * OPENING)
    }));
    let rim = rim
        .into_iter()
        .map(|p| mb.v(p, Vec3::Z, Vec3::ONE))
        .collect::<Vec<_>>();
    let middle = mb.v(at(0.0, SPRING * 0.6), Vec3::Z, Vec3::ONE);
    for k in 0..rim.len() {
        mb.tri(middle, rim[k], rim[(k + 1) % rim.len()]);
    }
    mb.build()
}

fn prepare(
    mut commands: Commands,
    pal: Res<Palette>,
    stock: Res<Stock>,
    set: Res<WorksSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let lamps = MODES.map(|mode| {
        let glow = hue(mode);
        materials.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::from_vec3(
                glow,
            )),
            emissive: LinearRgba::from_vec3(glow * set.lamp_glow),
            perceptual_roughness: 0.4,
            ..default()
        })
    });
    let veils = MODES.map(|mode| {
        let glow = hue(mode);
        materials.add(StandardMaterial {
            base_color: Color::LinearRgba(
                LinearRgba::from_vec3(glow)
                    .with_alpha(set.veil_cover),
            ),
            emissive: LinearRgba::from_vec3(glow * set.veil_glow),
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None::<Face>,
            ..default()
        })
    });
    commands.insert_resource(Fittings {
        stand: meshes.add(stand(&pal)),
        arm: meshes.add(arm(&pal)),
        glass: meshes.add(lantern_glass(0.1, 0.24)),
        arch: meshes.add(arch(&pal)),
        veil: meshes.add(veil()),
        dark: materials.add(StandardMaterial {
            base_color: pal.glass_dark,
            perceptual_roughness: 0.4,
            ..default()
        }),
        lamps,
        veils,
        matte: stock.matte.clone(),
        brass: stock.brass.clone(),
    });
}

/// Stands a lever and an arch on every new common.
fn fit(
    mut commands: Commands,
    fittings: Option<Res<Fittings>>,
    set: Res<WorksSettings>,
    land: Res<Land>,
    mut meshes: ResMut<Assets<Mesh>>,
    commons: Query<(Entity, &Common), Without<Fitted>>,
    hamlets: Query<(Entity, &Sited, Option<&Works>)>,
) {
    let Some(fittings) = fittings else {
        return;
    };
    let lift = land.set.town.plaza_lift;
    // Turned so that its front looks at the middle of the common.
    let place = |at: Vec2| {
        Transform::from_xyz(at.x, lift, at.y).with_rotation(
            Quat::from_rotation_y((-at.x).atan2(-at.y)),
        )
    };
    for (common, of) in &commons {
        let Some((hamlet, _, works)) =
            hamlets.iter().find(|(_, sited, _)| sited.0 == of.town)
        else {
            continue;
        };
        let now = mode(works);
        // As it stands already, so that only a change is seen thrown.
        let leans = lean(now, set.throw);
        let stand = commands
            .spawn((
                Mesh3d(fittings.stand.clone()),
                MeshMaterial3d(fittings.matte.clone()),
                place(set.lever_at)
                    .with_scale(Vec3::splat(set.lever_size)),
                ChildOf(common),
                children![
                    (
                        Lever {
                            hamlet,
                            lean: leans,
                        },
                        Mesh3d(fittings.arm.clone()),
                        MeshMaterial3d(fittings.brass.clone()),
                        Transform::from_xyz(0.0, PIVOT, 0.0)
                            .with_rotation(Quat::from_rotation_z(
                                leans
                            )),
                    ),
                    (
                        Beacon { hamlet },
                        Mesh3d(fittings.glass.clone()),
                        MeshMaterial3d(fittings.dark.clone()),
                        Transform::from_translation(LAMP),
                        NotShadowCaster,
                        PointLight {
                            intensity: 0.0,
                            range: set.reach,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                    ),
                ],
            ))
            .id();
        let arch = commands
            .spawn((
                Mesh3d(fittings.arch.clone()),
                MeshMaterial3d(fittings.matte.clone()),
                place(set.arch_at),
                ChildOf(common),
                children![(
                    Veil { hamlet },
                    Mesh3d(fittings.veil.clone()),
                    MeshMaterial3d(fittings.veils[0].clone()),
                    Visibility::Hidden,
                    NotShadowCaster,
                )],
            ))
            .id();
        let handle = commands
            .spawn(hit_box(
                Thing::Lever(hamlet),
                Vec3::new(1.1, 2.0, 0.9),
                Vec3::new(0.0, 1.0, -0.1),
                &mut meshes,
            ))
            .id();
        let top = SPRING + RING + 0.3;
        let doorway = commands
            .spawn((
                Doorway { hamlet },
                hit_box(
                    Thing::Arch(hamlet),
                    Vec3::new(2.3, top, 0.8),
                    Vec3::Y * top * 0.5,
                    &mut meshes,
                ),
            ))
            .id();
        for (parent, boxed) in [(stand, handle), (arch, doorway)] {
            adopt(&mut commands, parent, boxed);
            pick::watch(&mut commands, boxed);
        }
        commands.entity(common).try_insert(Fitted);
    }
}

/// Keeps each lever, its lamp and the veil of its arch as its hamlet
/// works now.
fn show(
    time: Res<Time>,
    set: Res<WorksSettings>,
    sky: Res<Sky>,
    fittings: Option<Res<Fittings>>,
    hamlets: Query<&Works>,
    mut levers: Query<(&mut Lever, &mut Transform)>,
    mut beacons: Query<(
        &Beacon,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut PointLight,
    )>,
    mut veils: Query<
        (
            &Veil,
            &mut MeshMaterial3d<StandardMaterial>,
            &mut Visibility,
        ),
        Without<Beacon>,
    >,
    mut redraw: Option<ResMut<Messages<RequestRedraw>>>,
) {
    let Some(fittings) = fittings else {
        return;
    };
    let now = |hamlet: Entity| mode(hamlets.get(hamlet).ok());
    let near = 1.0 - (-set.throw_rate * time.delta_secs()).exp();
    for (mut lever, mut at) in &mut levers {
        let want = lean(now(lever.hamlet), set.throw);
        if lever.lean == want {
            continue;
        }
        let next = lever.lean + (want - lever.lean) * near;
        lever.lean = if (want - next).abs() < 0.004 {
            want
        } else {
            next
        };
        at.rotation = Quat::from_rotation_z(lever.lean);
        if let Some(redraw) = &mut redraw {
            redraw.write(RequestRedraw);
        }
    }
    for (beacon, mut glass, mut thrown) in &mut beacons {
        let mode = now(beacon.hamlet);
        let want = match mode {
            Some(mode) => &fittings.lamps[index(mode)],
            None => &fittings.dark,
        };
        if glass.0 != *want {
            glass.0 = want.clone();
        }
        let lumens =
            mode.map_or(0.0, |_| set.lumens * sky.look.candle);
        if thrown.intensity != lumens {
            thrown.intensity = lumens;
        }
        if let Some(mode) = mode {
            let glow = hue(mode);
            let colour = Color::linear_rgb(glow.x, glow.y, glow.z);
            if thrown.color != colour {
                thrown.color = colour;
            }
        }
    }
    for (veil, mut light, mut seen) in &mut veils {
        let Some(mode) = now(veil.hamlet) else {
            seen.set_if_neq(Visibility::Hidden);
            continue;
        };
        seen.set_if_neq(Visibility::Inherited);
        let want = &fittings.veils[index(mode)];
        if light.0 != *want {
            light.0 = want.clone();
        }
    }
}

/// Names each arch for the folder its hamlet works on, and takes the
/// name off one whose hamlet works on none.
fn title(
    mut commands: Commands,
    hamlets: Query<&Works>,
    doorways: Query<(Entity, &Doorway, Option<&Reads>, Has<Named>)>,
) {
    for (entity, doorway, reads, named) in &doorways {
        let folder = hamlets
            .get(doorway.hamlet)
            .ok()
            .and_then(|works| works.0.folder.as_deref())
            .map(|folder| {
                folder.file_name().map_or_else(
                    || folder.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
            });
        match folder {
            Some(folder)
                if !named || reads.is_none_or(|r| r.0 != folder) =>
            {
                commands
                    .entity(entity)
                    .try_insert((Named, Reads(folder)));
            }
            None if named => {
                commands.entity(entity).try_remove::<Named>();
            }
            _ => {}
        }
    }
}

/// A lever that has a mode to change is heard thrown.
fn clunk(
    pulled: On<Pulled>,
    hamlets: Query<&Works>,
    levers: Query<(&Lever, &GlobalTransform)>,
    mut played: MessageWriter<Played>,
) {
    if mode(hamlets.get(pulled.hamlet).ok()).is_none() {
        return;
    }
    for (lever, at) in &levers {
        if lever.hamlet == pulled.hamlet {
            played.write(Played {
                what: Play::Lever,
                at: at.translation(),
            });
        }
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<WorksSettings>()
        .add_observer(clunk)
        .add_systems(Startup, prepare)
        .add_systems(
            Update,
            (
                fit,
                show,
                title.run_if(
                    any_match_filter::<
                        Or<(Changed<Works>, Added<Doorway>)>,
                    >,
                ),
            )
                .chain()
                .before(WorldSystems),
        );
}
