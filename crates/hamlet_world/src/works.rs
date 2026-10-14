//! A hamlet's work where it can be seen: a workbench in the middle of
//! each common for its folder, and clockwork on the bench for its
//! mode.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_core::agents::Works;
use hamlet_rules::work::Mode;
use hamlet_vfx::inlay;

use crate::generator::Common;
use crate::generator::land::Land;
use crate::hamlet::Sited;
use crate::lantern::{lantern_frame, lantern_glass};
use crate::meshes::Mb;
use crate::palette::Palette;
use crate::pick::{self, Footprint, Named, Reads, Thing, hit_box};
use crate::play::{Play, Played};
use crate::{Sky, Stock, WorldSystems, adopt};

/// Height of the bench's top above the paving.
const TOP: f32 = 0.91;

/// The middle of the lamp's glass, in the bench's frame.
const LAMP: Vec3 = Vec3::new(-0.85, 1.8, 0.36);

/// The wheels of the clockwork, the largest first: the hub of each in
/// the bench's frame, its radius and the number of its teeth.
const WHEELS: [(Vec3, f32, usize); 3] = [
    (Vec3::new(0.36, 1.2, -0.15), 0.19, 16),
    (Vec3::new(0.677, 1.348, -0.15), 0.12, 10),
    (Vec3::new(0.614, 1.022, -0.15), 0.08, 7),
];

const MODES: [Mode; 2] = [Mode::Manual, Mode::Auto];

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<WorksSettings>()
        .add_observer(click)
        .add_systems(Startup, prepare)
        .add_systems(
            Update,
            (
                fit,
                show,
                title.run_if(
                    any_match_filter::<
                        Or<(Changed<Works>, Added<Top>)>,
                    >,
                ),
            )
                .chain()
                .before(WorldSystems),
        );
}

/// The workbench of a common.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorksSettings {
    pub size: f32,
    /// Radians a second the clockwork's largest wheel turns in auto.
    pub spin: f32,
    /// Brightness of the lamp, the light it throws after dark, and
    /// how far.
    pub lamp_glow: f32,
    pub lumens: f32,
    pub reach: f32,
}

/// The chief pressed the clockwork of `hamlet`.
#[derive(Event, Clone, Copy, Debug)]
pub struct Switched {
    pub hamlet: Entity,
}

/// On a common that has its workbench.
#[derive(Component)]
struct Fitted;

/// A wheel of a hamlet's clockwork.
#[derive(Component)]
struct Wheel {
    hamlet: Entity,
    /// Turns it makes for one of the largest wheel. Its sign is the
    /// way it turns.
    ratio: f32,
    /// Radians the largest wheel had turned when this was last
    /// placed.
    turned: f32,
}

/// The lamp of a hamlet's workbench.
#[derive(Component)]
struct Beacon {
    hamlet: Entity,
}

/// The work laid out on a hamlet's workbench.
#[derive(Component)]
struct Laid {
    hamlet: Entity,
}

/// The part of a hamlet's workbench that answers the pointer and is
/// named for the folder.
#[derive(Component)]
struct Top {
    hamlet: Entity,
}

/// The meshes and lights of the workbenches.
#[derive(Resource)]
struct Fittings {
    bench: Handle<Mesh>,
    work: Handle<Mesh>,
    /// The wheels of the clockwork, the largest first.
    wheels: [Handle<Mesh>; 3],
    glass: Handle<Mesh>,
    /// The lamp's glass with no folder.
    dark: Handle<StandardMaterial>,
    /// The lamp's glass in each mode, manual first.
    lamps: [Handle<StandardMaterial>; 2],
    matte: Handle<StandardMaterial>,
    brass: Handle<StandardMaterial>,
}

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

/// The bench: a plank top on four legs with a shelf under it, a
/// lantern on a post at one corner, and the plate the clockwork is
/// mounted on at the other end.
fn bench(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    mb.rbox(
        Vec3::Y * (TOP - 0.045),
        Vec3::new(1.0, 0.045, 0.48),
        0.015,
        Quat::IDENTITY,
        pal.log,
        0.1,
    );
    for (x, z) in
        [(-1.0f32, -1.0f32), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)]
    {
        mb.rbox(
            Vec3::new(x * 0.88, (TOP - 0.09) * 0.5, z * 0.38),
            Vec3::new(0.06, (TOP - 0.09) * 0.5, 0.06),
            0.015,
            Quat::IDENTITY,
            pal.timber_dark,
            0.3,
        );
    }
    mb.rbox(
        Vec3::Y * 0.26,
        Vec3::new(0.86, 0.025, 0.36),
        0.01,
        Quat::IDENTITY,
        pal.timber,
        0.2,
    );
    let foot = Vec3::new(LAMP.x, TOP, LAMP.z);
    mb.tube(
        foot,
        LAMP - Vec3::Y * 0.15,
        0.03,
        0.022,
        6,
        pal.bed,
        false,
    );
    lantern_frame(&mut mb, LAMP, 0.1, 0.24, pal.brass);
    mb.rbox(
        Vec3::new(0.52, TOP + 0.3, -0.22),
        Vec3::new(0.44, 0.3, 0.025),
        0.012,
        Quat::IDENTITY,
        pal.timber_dark,
        0.1,
    );
    for (hub, ..) in WHEELS {
        mb.tube(
            Vec3::new(hub.x, hub.y, -0.2),
            Vec3::new(hub.x, hub.y, hub.z - 0.03),
            0.025,
            0.025,
            6,
            pal.bed,
            false,
        );
    }
    mb.build()
}

/// The work a hamlet with a folder has on its bench: a drawing
/// unrolled, a mallet, an inkpot with its quill, and two books.
fn work(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    let turn = Quat::from_rotation_y(0.12);
    let sheet = Vec3::new(-0.35, TOP + 0.008, 0.08);
    mb.rbox(
        sheet,
        Vec3::new(0.36, 0.006, 0.27),
        0.003,
        turn,
        pal.parchment,
        0.0,
    );
    for line in 0..4 {
        let along = 0.3 - line as f32 * 0.07;
        let down = -0.2 + line as f32 * 0.12;
        mb.rbox(
            sheet + turn * Vec3::new(-0.04, 0.008, down),
            Vec3::new(along, 0.002, 0.008),
            0.001,
            turn,
            pal.bed,
            0.0,
        );
    }
    mb.tube(
        sheet + turn * Vec3::new(-0.38, 0.03, -0.28),
        sheet + turn * Vec3::new(-0.38, 0.03, 0.28),
        0.035,
        0.035,
        8,
        pal.parchment * 0.88,
        true,
    );
    mb.tube(
        Vec3::new(-0.1, TOP + 0.025, 0.4),
        Vec3::new(0.14, TOP + 0.025, 0.33),
        0.02,
        0.02,
        6,
        pal.timber,
        true,
    );
    mb.rbox(
        Vec3::new(0.16, TOP + 0.045, 0.325),
        Vec3::new(0.045, 0.045, 0.085),
        0.012,
        Quat::from_rotation_y(0.28),
        pal.timber_dark,
        0.0,
    );
    let pot = Vec3::new(0.05, TOP, -0.3);
    mb.lathe(
        pot,
        &[
            (0.0, 0.0),
            (0.05, 0.0),
            (0.06, 0.05),
            (0.035, 0.08),
            (0.0, 0.08),
        ],
        8,
        |_, _| pal.bed,
    );
    mb.tube(
        pot + Vec3::Y * 0.06,
        pot + Vec3::new(0.09, 0.3, -0.05),
        0.006,
        0.018,
        5,
        pal.whitewash,
        true,
    );
    for (lift, half, colour, yaw) in [
        (0.03, Vec3::new(0.14, 0.03, 0.1), pal.terracotta, 0.0),
        (0.085, Vec3::new(0.12, 0.025, 0.085), pal.ochre, 0.25),
    ] {
        mb.rbox(
            Vec3::new(-0.45, TOP + lift, -0.3),
            half,
            0.008,
            Quat::from_rotation_y(yaw),
            colour,
            0.0,
        );
    }
    mb.build()
}

/// A wheel of the clockwork about its hub, lying flat: its teeth, and
/// three dark holes in its face so its turning shows. With `crank`
/// it has the handle it is turned by hand with.
fn wheel(
    pal: &Palette,
    radius: f32,
    teeth: usize,
    crank: bool,
) -> Mesh {
    let mut mb = Mb::new();
    let thick = 0.03;
    let disc =
        |mb: &mut Mb, at: Vec3, r: f32, tall: f32, col: Vec3| {
            mb.lathe(
                at,
                &[(0.0, 0.0), (r, 0.0), (r, tall), (0.0, tall)],
                teeth * 2,
                |_, _| col,
            );
        };
    disc(&mut mb, Vec3::Y * -thick, radius, thick * 2.0, pal.brass);
    for tooth in 0..teeth {
        let around = tooth as f32 / teeth as f32 * TAU;
        mb.rbox(
            Vec3::new(around.cos(), 0.0, around.sin())
                * (radius + 0.012),
            Vec3::new(
                0.03,
                thick * 0.9,
                radius * PI / teeth as f32 * 0.5,
            ),
            0.005,
            Quat::from_rotation_y(-around),
            pal.brass * 1.15,
            0.0,
        );
    }
    for hole in 0..3 {
        let around = hole as f32 / 3.0 * TAU + 0.5;
        disc(
            &mut mb,
            Vec3::new(around.cos(), 0.0, around.sin()) * radius * 0.6
                + Vec3::Y * thick,
            radius * 0.18,
            0.003,
            pal.bed,
        );
    }
    disc(
        &mut mb,
        Vec3::Y * thick,
        radius * 0.28,
        0.02,
        pal.brass * 1.3,
    );
    if crank {
        let out = radius * 0.8;
        mb.rbox(
            Vec3::new(out * 0.5, thick + 0.035, 0.0),
            Vec3::new(out * 0.5 + 0.02, 0.012, 0.022),
            0.006,
            Quat::IDENTITY,
            pal.bed,
            0.0,
        );
        mb.tube(
            Vec3::new(out, thick + 0.04, 0.0),
            Vec3::new(out, thick + 0.2, 0.0),
            0.028,
            0.028,
            8,
            pal.timber,
            true,
        );
    }
    mb.build()
}

/// The place of a wheel whose largest fellow has turned `turned`
/// radians. It stands on its edge, with its face to the bench's
/// front.
fn turn(hub: Vec3, ratio: f32, turned: f32) -> Transform {
    Transform::from_translation(hub).with_rotation(
        Quat::from_rotation_x(FRAC_PI_2)
            * Quat::from_rotation_y(turned * ratio),
    )
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
    let mut first = true;
    let wheels = WHEELS.map(|(_, radius, teeth)| {
        let crank = std::mem::take(&mut first);
        meshes.add(wheel(&pal, radius, teeth, crank))
    });
    commands.insert_resource(Fittings {
        bench: meshes.add(bench(&pal)),
        work: meshes.add(work(&pal)),
        wheels,
        glass: meshes.add(lantern_glass(0.09, 0.22)),
        dark: materials.add(StandardMaterial {
            base_color: pal.glass_dark,
            perceptual_roughness: 0.4,
            ..default()
        }),
        lamps,
        matte: stock.matte.clone(),
        brass: stock.brass.clone(),
    });
}

/// Stands a workbench on every new common.
fn fit(
    mut commands: Commands,
    fittings: Option<Res<Fittings>>,
    set: Res<WorksSettings>,
    land: Res<Land>,
    mut meshes: ResMut<Assets<Mesh>>,
    commons: Query<(Entity, &Common), Without<Fitted>>,
    hamlets: Query<(Entity, &Sited)>,
) {
    let Some(fittings) = fittings else {
        return;
    };
    let town = &land.set.town;
    let at = town.bench.0;
    for (common, of) in &commons {
        let Some((hamlet, _)) =
            hamlets.iter().find(|(_, sited)| sited.0 == of.town)
        else {
            continue;
        };
        let bench = commands
            .spawn((
                Mesh3d(fittings.bench.clone()),
                MeshMaterial3d(fittings.matte.clone()),
                Transform::from_xyz(at.x, town.plaza_lift, at.y)
                    .with_scale(Vec3::splat(set.size)),
                ChildOf(common),
                children![
                    (
                        Laid { hamlet },
                        Mesh3d(fittings.work.clone()),
                        MeshMaterial3d(fittings.matte.clone()),
                        Visibility::Hidden,
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
        let largest = WHEELS[0].2 as f32;
        for (k, (hub, _, teeth)) in WHEELS.into_iter().enumerate() {
            // Each is turned by the largest, the other way round.
            let ratio =
                if k == 0 { 1.0 } else { -largest / teeth as f32 };
            commands.spawn((
                Wheel {
                    hamlet,
                    ratio,
                    turned: 0.0,
                },
                Mesh3d(fittings.wheels[k].clone()),
                MeshMaterial3d(fittings.brass.clone()),
                turn(hub, ratio, 0.0),
                ChildOf(bench),
            ));
        }
        let top = commands
            .spawn((
                Top { hamlet },
                hit_box(
                    Thing::Workbench(hamlet),
                    Vec3::new(2.1, TOP + 0.1, 1.1),
                    Vec3::Y * (TOP + 0.1) * 0.5,
                    &mut meshes,
                ),
            ))
            .id();
        // The folder's name stands just over the clockwork, under
        // the hamlet's own.
        commands.entity(top).try_insert(Footprint {
            radius: 1.2 * set.size,
            height: (TOP + 0.7) * set.size,
            lift: (TOP + 0.1) * 0.5 * set.size,
        });
        let train = commands
            .spawn(hit_box(
                Thing::Clockwork(hamlet),
                Vec3::new(0.95, 0.7, 0.45),
                Vec3::new(0.52, TOP + 0.36, -0.1),
                &mut meshes,
            ))
            .id();
        for boxed in [top, train] {
            adopt(&mut commands, bench, boxed);
            pick::watch(&mut commands, boxed);
        }
        commands.entity(common).try_insert(Fitted);
    }
}

/// Keeps each bench as its hamlet works now: its work laid out, its
/// lamp in the mode's colour, and its clockwork turning in auto.
fn show(
    time: Res<Time>,
    set: Res<WorksSettings>,
    sky: Res<Sky>,
    fittings: Option<Res<Fittings>>,
    hamlets: Query<&Works>,
    mut wheels: Query<(&mut Wheel, &mut Transform)>,
    mut beacons: Query<(
        &Beacon,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut PointLight,
    )>,
    mut laid: Query<(&Laid, &mut Visibility)>,
    mut redraw: Option<ResMut<Messages<RequestRedraw>>>,
) {
    let Some(fittings) = fittings else {
        return;
    };
    let now = |hamlet: Entity| mode(hamlets.get(hamlet).ok());
    let step = set.spin * time.delta_secs();
    for (mut wheel, mut at) in &mut wheels {
        if now(wheel.hamlet) != Some(Mode::Auto) || step == 0.0 {
            continue;
        }
        wheel.turned += step;
        at.rotation =
            turn(Vec3::ZERO, wheel.ratio, wheel.turned).rotation;
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
    for (laid, mut seen) in &mut laid {
        let want = match now(laid.hamlet) {
            Some(_) => Visibility::Inherited,
            None => Visibility::Hidden,
        };
        seen.set_if_neq(want);
    }
}

/// Names each bench for the folder its hamlet works on, and takes the
/// name off one whose hamlet works on none.
fn title(
    mut commands: Commands,
    hamlets: Query<&Works>,
    tops: Query<(Entity, &Top, Option<&Reads>, Has<Named>)>,
) {
    for (entity, top, reads, named) in &tops {
        let folder = hamlets
            .get(top.hamlet)
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

/// Clockwork that has a mode to change is heard switched.
fn click(
    switched: On<Switched>,
    hamlets: Query<&Works>,
    beacons: Query<(&Beacon, &GlobalTransform)>,
    mut played: MessageWriter<Played>,
) {
    if mode(hamlets.get(switched.hamlet).ok()).is_none() {
        return;
    }
    for (beacon, at) in &beacons {
        if beacon.hamlet == switched.hamlet {
            played.write(Played {
                what: Play::Clockwork,
                at: at.translation(),
            });
        }
    }
}
