//! The library in the world: a stone house under a dome, on ground of
//! its own beside the first town and in no hamlet. It stands for the
//! skills every agent shares.

mod build;
mod librarian;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;

pub use self::librarian::Librarian;
use self::librarian::{ATTENDS, LOOK, STANDS, sage};
use crate::chief::mascot::{self, Rig};
use crate::chief::{Chief, Settled, place};
use crate::generator::land::Land;
use crate::generator::society::{Claim, Ground, Society};
use crate::lantern::NightLight;
use crate::palette::Palette;
use crate::pick::{self, Footprint, Named, Thing, hit_box};
use crate::{Clock, RealmSystems, Sky, Stock};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Stood>()
        .add_observer(librarian::called)
        .add_systems(
            Update,
            (
                (resettle, raise).chain().in_set(Raising),
                librarian::tend.before(mascot::stand),
                shine,
            )
                .in_set(RealmSystems::Act),
        );
}

/// The systems that stand the library on its ground. They run once
/// every chief has its place and before any town is founded.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Raising;

/// The radius of the ground the library keeps level and clear.
pub(crate) const GROUNDS: f32 = 9.5;

/// Reach of the library from its middle and its height, for the box
/// it is picked by, the ring about it and the name over it.
const SIZE: (f32, f32) = (7.2, 9.2);

/// The spot a chief has set the library down on. Without one it
/// stands where the land puts it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Stood(pub Option<Vec2>);

impl Stood {
    /// The file for the library's place under the window's folder.
    pub fn file(view: &std::path::Path) -> std::path::PathBuf {
        view.join("library.place")
    }
}

/// Takes the library up when a chief has set it down elsewhere, so
/// that it is raised anew where it now stands, as a chief's place
/// is.
fn resettle(
    mut commands: Commands,
    stood: Res<Stood>,
    libraries: Query<(Entity, &Library)>,
    mut society: ResMut<Society>,
) {
    let Some(to) = stood.0.filter(|_| stood.is_changed()) else {
        return;
    };
    for (entity, library) in &libraries {
        if library.held != to {
            society
                .claims
                .retain(|claim| claim.place != Ground::Library);
            commands.entity(entity).try_despawn();
        }
    }
}

/// The library, and the middle of the ground it holds.
#[derive(Component, Clone, Copy, Debug)]
pub struct Library {
    pub held: Vec2,
}

/// Stands the library in the world, once every chief there is has
/// its own place: beside the first town, on ground nothing else
/// holds.
fn raise(
    mut commands: Commands,
    land: Res<Land>,
    pal: Res<Palette>,
    stock: Res<Stock>,
    mut society: ResMut<Society>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    stood: Res<Stood>,
    libraries: Query<(), With<Library>>,
    waiting: Query<(), (With<Chief>, Without<Settled>)>,
) {
    if !libraries.is_empty() || !waiting.is_empty() {
        return;
    }
    let taken = society
        .towns
        .iter()
        .map(|town| land.sites[town.site].at)
        .chain(society.claims.iter().map(|claim| claim.at))
        .collect::<Vec<_>>();
    let Some(at) = stood.0.or_else(|| place::stand(&land, &taken))
    else {
        return;
    };
    society.claims.push(Claim {
        place: Ground::Library,
        at,
        radius: land.set.town.keep_apart * 0.5,
    });
    // The door looks toward the first town.
    let home = land
        .sites
        .iter()
        .find(|site| site.buildable)
        .map_or(Vec2::ZERO, |site| site.at);
    let toward = (home - at).normalize_or(Vec2::Y);
    let (walls, glass) = build::build(&pal);
    let glow = pal.glass_lit.to_linear();
    let lit = materials.add(StandardMaterial {
        base_color: pal.glass_lit,
        emissive: glow * 2.5,
        ..default()
    });
    let rune = Color::linear_rgb(pal.rune.x, pal.rune.y, pal.rune.z);
    let library = commands
        .spawn((
            Library { held: at },
            Windows {
                glass: lit.clone(),
                glow,
            },
            Mesh3d(meshes.add(walls)),
            MeshMaterial3d(stock.matte.clone()),
            Transform::from_translation(land.on_ground(at))
                .with_rotation(Quat::from_rotation_y(
                    toward.x.atan2(toward.y),
                )),
            Visibility::Inherited,
            children![(
                Mesh3d(meshes.add(glass)),
                MeshMaterial3d(lit),
                NotShadowCaster,
            )],
        ))
        .id();
    // A light over the dome that flares when the librarian is
    // called on, by day or by night.
    commands.spawn((
        Beacon,
        PointLight {
            color: rune,
            intensity: 0.0,
            range: BEACON_REACH,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 10.2, 1.0),
        ChildOf(library),
    ));
    // A lamp at each post and one under the porch, lit after dark.
    let warm = pal.glass_lit;
    for (at, lumens) in [
        (Vec3::new(3.5, 2.3, 6.3), LAMP),
        (Vec3::new(-3.5, 2.3, 6.3), LAMP),
        (Vec3::new(0.0, 3.6, 4.4), LAMP * 1.6),
    ] {
        commands.spawn((
            NightLight(lumens),
            PointLight {
                color: warm,
                intensity: 0.0,
                range: LAMP_REACH,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(at),
            ChildOf(library),
        ));
    }
    let (wide, tall) = SIZE;
    let boxed = commands
        .spawn((
            // The porch and the steps reach out before the hall.
            hit_box(
                Thing::Library,
                Vec3::new(11.0, tall, 12.0),
                Vec3::new(0.0, tall * 0.5, 1.2),
                &mut meshes,
            ),
            ChildOf(library),
        ))
        .id();
    // Its name is shown over it, and its ring is as wide as it is.
    commands.entity(boxed).insert((
        Named,
        Footprint {
            radius: wide,
            height: tall,
            lift: tall * 0.5,
        },
    ));
    pick::watch(&mut commands, boxed);

    // The librarian, on the porch before its doors.
    let light = pal.wicks[LOOK.light as usize % pal.wicks.len()];
    let flame = materials.add(StandardMaterial {
        base_color: Color::linear_rgb(light.x, light.y, light.z),
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let keeper = commands
        .spawn((
            Transform::from_translation(STANDS),
            Visibility::Inherited,
            ChildOf(library),
        ))
        .id();
    let wick = meshes.add(mascot::wick().build());
    let rig = Rig::spawn(
        &mut commands,
        keeper,
        &stock.matte,
        (wick, flame),
    );
    let dressed = mascot::parts(&pal, LOOK)
        .each()
        .map(|meshing| meshes.add(meshing.build()));
    for (entity, which) in rig.dressed() {
        commands
            .entity(entity)
            .insert(Mesh3d(dressed[which].clone()));
    }
    let (head, staff, (stone, tip)) = sage(&pal);
    for (mesh, on) in [(head, rig.head), (staff, rig.figure)] {
        commands.spawn((
            Mesh3d(meshes.add(mesh.build())),
            MeshMaterial3d(stock.matte.clone()),
            ChildOf(on),
        ));
    }
    // The stone in the staff glows and lights what is near it.
    let (burn, lumens, reach) = STONE;
    let shine = Color::linear_rgb(light.x, light.y, light.z);
    commands.spawn((
        Mesh3d(meshes.add(stone.build())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: shine,
            emissive: LinearRgba::from_vec3(light * burn),
            ..default()
        })),
        NotShadowCaster,
        ChildOf(rig.figure),
        children![(
            PointLight {
                color: shine,
                intensity: lumens,
                range: reach,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(tip),
        )],
    ));
    commands
        .entity(keeper)
        .insert((Librarian { called: None }, rig));
}

/// The light of one of its lamps after dark, and how far it
/// reaches.
const LAMP: f32 = 90_000.0;
const LAMP_REACH: f32 = 11.0;

/// The glass of the library's windows, and its light at rest.
#[derive(Component, Clone, Debug)]
struct Windows {
    glass: Handle<StandardMaterial>,
    glow: LinearRgba,
}

/// The light over the dome that flares when the librarian is
/// called on.
#[derive(Component)]
struct Beacon;

/// The beacon's light at its brightest and how far it reaches, and
/// how many times brighter the windows burn with it.
const BEACON: f32 = 900_000.0;
const BEACON_REACH: f32 = 26.0;
const FLASH: f32 = 9.0;

/// Brightness of the windows' glass and of the lamps by day, and the
/// gain after dark.
const GLASS: (f32, f32) = (1.2, 16.0);

/// Brightness of the stone in the librarian's staff, the light it
/// throws and its reach.
const STONE: (f32, f32, f32) = (14.0, 30_000.0, 4.5);

/// Brightness of the library `since` seconds after a call: up at
/// once, wavering while the librarian attends, and down again.
fn flare(since: f32) -> f32 {
    if !(0.0..ATTENDS).contains(&since) {
        return 0.0;
    }
    let up = (since / 0.25).min(1.0);
    let down = ((ATTENDS - since) / 1.8).min(1.0);
    up * down * (0.82 + 0.18 * (since * 9.0).sin())
}

/// Burns the beacon and the windows while the librarian is called
/// on.
fn shine(
    clock: Res<Clock>,
    sky: Res<Sky>,
    librarians: Query<&Librarian>,
    windows: Query<&Windows>,
    mut beacons: Query<&mut PointLight, With<Beacon>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let level = librarians
        .iter()
        .filter_map(|librarian| librarian.called)
        .map(|at| flare(clock.t - at))
        .fold(0.0, f32::max);
    for mut beacon in &mut beacons {
        let lumens = BEACON * level;
        if beacon.intensity != lumens {
            beacon.intensity = lumens;
        }
    }
    for window in &windows {
        // Dim by day, glowing after dark, and brighter still
        // while the librarian is called on.
        let (day, night) = GLASS;
        let to = window.glow
            * (day + night * sky.look.candle + FLASH * level);
        // Read first, so that the glass is not marked changed
        // every frame it is at rest.
        let differs = materials
            .get(&window.glass)
            .is_some_and(|glass| glass.emissive != to);
        if differs
            && let Some(mut glass) = materials.get_mut(&window.glass)
        {
            glass.emissive = to;
        }
    }
    if level > 0.0 {
        redraw.write(RequestRedraw);
    }
}

/// The spot of one who comes to the library, from its middle: at the
/// foot of the steps, to the side the librarian is not on.
pub(crate) const VISITOR: Vec3 = Vec3::new(-1.7, 0.0, 7.3);
