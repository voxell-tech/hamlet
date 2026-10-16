//! The Agent Bazaar in the world: striped tents and stalls on ground
//! of their own beside the first town, in no hamlet. It stands for
//! the models the home may buy and run.

mod build;
mod merchant;
mod shoppers;

use std::f32::consts::TAU;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_vfx::flag::{
    FlagMaterial, FlagSettings, Waving, cloth, flag,
};

use self::build::FAIRY;
use self::merchant::{CHEERS, STANDS, turban};
pub use self::merchant::{Merchant, Traded};
pub use self::shoppers::Shopper;
use crate::chief::mascot::{self, Rig};
use crate::chief::{Chief, Look, Settled, place};
use crate::generator::land::Land;
use crate::generator::society::{Claim, Place as Ground, Society};
use crate::lantern::NightLight;
use crate::palette::Palette;
use crate::pick::{self, Footprint, Named, Thing, hit_box};
use crate::{Clock, Sky, Stock};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Pitched>()
        .add_observer(merchant::sold)
        .add_systems(
            Update,
            (
                (repitch, pitch).chain().in_set(Pitching),
                (merchant::tend, shoppers::stroll)
                    .before(mascot::stand),
                (shine, twinkle),
            ),
        );
}

/// The systems that stand the bazaar on its ground. They run once the
/// library has its own and before any town is founded.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Pitching;

/// The radius of the ground the bazaar keeps level and clear.
pub(crate) const GROUNDS: f32 = 9.5;

/// Reach of the bazaar from its middle and its height, for the box
/// it is picked by, the ring about it and the name over it.
const SIZE: (f32, f32) = (7.2, 7.7);

/// The spot a chief has set the bazaar down on. Without one it
/// stands where the land puts it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Pitched(pub Option<Vec2>);

impl Pitched {
    /// The file a home keeps the bazaar's place in.
    pub fn file(home: &std::path::Path) -> std::path::PathBuf {
        home.join("bazaar.place")
    }
}

/// The bazaar, and the middle of the ground it holds.
#[derive(Component, Clone, Copy, Debug)]
pub struct Bazaar {
    pub held: Vec2,
}

/// Takes the bazaar down when a chief has set it down elsewhere, so
/// that it is pitched anew where it now stands.
fn repitch(
    mut commands: Commands,
    pitched: Res<Pitched>,
    bazaars: Query<(Entity, &Bazaar)>,
    mut society: ResMut<Society>,
) {
    let Some(to) = pitched.0.filter(|_| pitched.is_changed()) else {
        return;
    };
    for (entity, bazaar) in &bazaars {
        if bazaar.held != to {
            society
                .claims
                .retain(|claim| claim.place != Ground::Bazaar);
            commands.entity(entity).try_despawn();
        }
    }
}

/// The lit glass of the bazaar's lanterns, and its light at rest.
#[derive(Component, Clone, Debug)]
struct Lanterns {
    glass: Handle<StandardMaterial>,
    glow: LinearRgba,
}

/// The bulbs of the fairy lights, a material to each colour, and
/// each colour at full glow.
#[derive(Component, Clone, Debug)]
struct Fairies {
    bulbs: [Handle<StandardMaterial>; FAIRY],
    colours: [LinearRgba; FAIRY],
}

/// A light over the avenue that takes on the fairy lights' colours
/// in turn after dark; its number sets where in the turn it is.
#[derive(Component, Clone, Copy, Debug)]
struct Glow(usize);

/// The meshes and materials a figure is made of.
type Making<'a> = (
    &'a mut Assets<Mesh>,
    &'a mut Assets<StandardMaterial>,
    &'a Palette,
    &'a Stock,
);

/// A figure on the rig of a chief's mascot, dressed in `look`,
/// standing under `parent` at `at`. Returns its root and its rig.
fn figure(
    commands: &mut Commands,
    (meshes, materials, pal, stock): Making,
    parent: Entity,
    look: Look,
    at: Vec3,
) -> (Entity, Rig) {
    let light = pal.wicks[look.light as usize % pal.wicks.len()];
    let flame = materials.add(StandardMaterial {
        base_color: Color::linear_rgb(light.x, light.y, light.z),
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let root = commands
        .spawn((
            Transform::from_translation(at),
            Visibility::Inherited,
            ChildOf(parent),
        ))
        .id();
    let wick = meshes.add(mascot::wick().build());
    let rig = Rig::spawn(commands, root, &stock.matte, (wick, flame));
    let dressed = mascot::parts(pal, look)
        .each()
        .map(|mb| meshes.add(mb.build()));
    for (entity, which) in rig.dressed() {
        commands
            .entity(entity)
            .insert(Mesh3d(dressed[which].clone()));
    }
    (root, rig)
}

/// Stands the bazaar in the world once every chief there is has its
/// own place: beside the first town, on ground nothing else holds.
#[allow(clippy::too_many_arguments)]
fn pitch(
    mut commands: Commands,
    land: Res<Land>,
    pal: Res<Palette>,
    stock: Res<Stock>,
    mut society: ResMut<Society>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    pitched: Res<Pitched>,
    bazaars: Query<(), With<Bazaar>>,
    waiting: Query<(), (With<Chief>, Without<Settled>)>,
    mut flags: Option<(
        ResMut<Waving>,
        ResMut<Assets<FlagMaterial>>,
        Res<FlagSettings>,
    )>,
    clock: Res<Clock>,
) {
    if !bazaars.is_empty() || !waiting.is_empty() {
        return;
    }
    let taken = society
        .towns
        .iter()
        .map(|town| land.sites[town.site].at)
        .chain(society.claims.iter().map(|claim| claim.at))
        .collect::<Vec<_>>();
    let Some(at) = pitched.0.or_else(|| place::stand(&land, &taken))
    else {
        return;
    };
    society.claims.push(Claim {
        place: Ground::Bazaar,
        at,
        radius: land.set.town.keep_apart * 0.5,
    });
    // The gate looks toward the first town.
    let home = land
        .sites
        .iter()
        .find(|site| site.buildable)
        .map_or(Vec2::ZERO, |site| site.at);
    let toward = (home - at).normalize_or(Vec2::Y);
    let built = build::build(&pal);
    let glow = pal.glass_lit.to_linear();
    let lit = materials.add(StandardMaterial {
        base_color: pal.glass_lit,
        emissive: glow * 2.5,
        ..default()
    });
    let colours = pal.fairy.map(|c| LinearRgba::rgb(c.x, c.y, c.z));
    let bulbs = colours.map(|colour| {
        materials.add(StandardMaterial {
            base_color: colour.into(),
            emissive: colour * BULB.0,
            ..default()
        })
    });
    let bazaar = commands
        .spawn((
            Bazaar { held: at },
            Lanterns {
                glass: lit.clone(),
                glow,
            },
            Fairies {
                bulbs: bulbs.clone(),
                colours,
            },
            Mesh3d(meshes.add(built.walls)),
            MeshMaterial3d(stock.matte.clone()),
            Transform::from_translation(land.on_ground(at))
                .with_rotation(Quat::from_rotation_y(
                    toward.x.atan2(toward.y),
                )),
            Visibility::Inherited,
            children![(
                Mesh3d(meshes.add(built.glass)),
                MeshMaterial3d(lit),
                NotShadowCaster,
            )],
        ))
        .id();
    for (mesh, bulb) in built.bulbs.into_iter().zip(bulbs) {
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(bulb),
            NotShadowCaster,
            ChildOf(bazaar),
        ));
    }
    for (k, x) in [-1.2f32, 1.2].into_iter().enumerate() {
        commands.spawn((
            Glow(k * FAIRY / 2),
            PointLight {
                intensity: 0.0,
                range: GLOW_REACH,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(x, 2.3, 2.6),
            ChildOf(bazaar),
        ));
    }
    if let Some((waving, cloths, set)) = flags.as_mut() {
        for pennon in &built.flags {
            let mesh = meshes.add(cloth(
                set,
                pennon.long,
                pennon.tall,
                &pennon.colours,
            ));
            commands.spawn((
                flag(waving, cloths, set, clock.t, mesh, pennon.long),
                Transform::from_translation(pennon.at),
                ChildOf(bazaar),
            ));
        }
    }
    // A light under the pavilion's front that flares after a sale,
    // by day or by night.
    commands.spawn((
        Flare,
        PointLight {
            color: Color::linear_rgb(
                pal.ember.x,
                pal.ember.y,
                pal.ember.z,
            ),
            intensity: 0.0,
            range: FLARE_REACH,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 3.0, 1.6),
        ChildOf(bazaar),
    ));
    // Each lantern lights what is about it after dark.
    let warm = pal.glass_lit;
    for lamp in built.lamps {
        commands.spawn((
            NightLight(LAMP),
            PointLight {
                color: warm,
                intensity: 0.0,
                range: LAMP_REACH,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(lamp),
            ChildOf(bazaar),
        ));
    }
    let (wide, tall) = SIZE;
    let boxed = commands
        .spawn((
            hit_box(
                Thing::Bazaar,
                Vec3::new(wide * 2.0, tall, wide * 2.0),
                Vec3::new(0.0, tall * 0.5, 0.0),
                &mut meshes,
            ),
            ChildOf(bazaar),
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

    // The merchant before the counter, and the shoppers going round
    // the stalls.
    let (keeper, rig) = figure(
        &mut commands,
        (&mut meshes, &mut materials, &pal, &stock),
        bazaar,
        merchant::LOOK,
        STANDS,
    );
    for (round, look) in shoppers::LOOKS.into_iter().enumerate() {
        let (shopper, rig) = figure(
            &mut commands,
            (&mut meshes, &mut materials, &pal, &stock),
            bazaar,
            look,
            Vec3::ZERO,
        );
        commands.entity(shopper).insert((Shopper { round }, rig));
    }
    commands.spawn((
        Mesh3d(meshes.add(turban(&pal).build())),
        MeshMaterial3d(stock.matte.clone()),
        ChildOf(rig.head),
    ));
    commands
        .entity(keeper)
        .insert((Merchant { sold: None }, rig));
}

/// The light of one of its lanterns after dark, and how far it
/// reaches.
const LAMP: f32 = 80_000.0;
const LAMP_REACH: f32 = 8.0;

/// A bulb's glow by day, its gain after dark and how many times
/// brighter it burns after a sale.
const BULB: (f32, f32, f32) = (2.0, 14.0, 10.0);

/// The rounds a second the lights' chase runs along a string, and
/// how many times faster it runs after a sale.
const CHASE: (f32, f32) = (0.8, 3.0);

/// The light of a glow over the avenue after dark, how far it
/// reaches and the seconds it keeps each colour.
const GLOW: f32 = 120_000.0;
const GLOW_REACH: f32 = 9.0;
const GLOW_HOLDS: f32 = 1.6;

/// The light under the pavilion that flares after a sale.
#[derive(Component)]
struct Flare;

/// The flare's light at its brightest, and how far it reaches.
const FLARE: f32 = 700_000.0;
const FLARE_REACH: f32 = 18.0;

/// Brightness of the lanterns' glass by day and the gain after dark,
/// and how many times brighter it burns after a sale.
const GLASS: (f32, f32) = (1.2, 16.0);
const FLASH: f32 = 9.0;

/// Brightness of the bazaar `since` seconds after a sale: up at once,
/// flickering while the merchant cheers, and down again.
fn flare(since: f32) -> f32 {
    if !(0.0..CHEERS).contains(&since) {
        return 0.0;
    }
    let up = (since / 0.2).min(1.0);
    let down = ((CHEERS - since) / 1.5).min(1.0);
    up * down * (0.75 + 0.25 * (since * 12.0).sin())
}

/// Burns the lanterns, glowing after dark and brighter still after
/// a sale, and the flare under the pavilion.
fn shine(
    clock: Res<Clock>,
    sky: Res<Sky>,
    merchants: Query<&Merchant>,
    lanterns: Query<&Lanterns>,
    mut flares: Query<&mut PointLight, With<Flare>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let level = merchants
        .iter()
        .filter_map(|merchant| merchant.sold)
        .map(|at| flare(clock.t - at))
        .fold(0.0, f32::max);
    for mut flare in &mut flares {
        let lumens = FLARE * level;
        if flare.intensity != lumens {
            flare.intensity = lumens;
        }
    }
    for lantern in &lanterns {
        let (day, night) = GLASS;
        let to = lantern.glow
            * (day + night * sky.look.candle + FLASH * level);
        // Read first, so that the glass is not marked changed every
        // frame it is at rest.
        let differs = materials
            .get(&lantern.glass)
            .is_some_and(|glass| glass.emissive != to);
        if differs
            && let Some(mut glass) = materials.get_mut(&lantern.glass)
        {
            glass.emissive = to;
        }
    }
    if level > 0.0 {
        redraw.write(RequestRedraw);
    }
}

/// The share of its glow the bulb of colour `k` of `FAIRY` has at
/// `t`, of a chase that runs along every string `rounds` times a
/// second: dim between the light passing and bright as it passes.
fn chased(k: usize, t: f32, rounds: f32) -> f32 {
    let wave = 0.5
        + 0.5 * (TAU * (t * rounds - k as f32 / FAIRY as f32)).sin();
    0.3 + 0.7 * wave * wave
}

/// Runs the fairy lights: steady by day, chasing along their strings
/// after dark and faster after a sale, with the glows over the avenue
/// taking on their colours in turn.
fn twinkle(
    clock: Res<Clock>,
    sky: Res<Sky>,
    merchants: Query<&Merchant>,
    fairies: Query<(&Fairies, &ViewVisibility)>,
    mut glows: Query<(&Glow, &mut PointLight)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let level = merchants
        .iter()
        .filter_map(|merchant| merchant.sold)
        .map(|at| flare(clock.t - at))
        .fold(0.0, f32::max);
    let night = sky.look.candle;
    // By day and with no sale the lights stand still, and ask for no
    // frames.
    let still = night < 0.05 && level == 0.0;
    let (day, dark, sale) = BULB;
    let (rounds, faster) = CHASE;
    let rounds = rounds * (1.0 + faster * level);
    let mut seen = false;
    for (fairies, view) in &fairies {
        seen |= view.get();
        for (k, bulb) in fairies.bulbs.iter().enumerate() {
            let chase = if still {
                1.0
            } else {
                chased(k, clock.t, rounds)
            };
            let to = fairies.colours[k]
                * (day + (dark * night + sale * level) * chase);
            let differs = materials
                .get(bulb)
                .is_some_and(|glass| glass.emissive != to);
            if differs
                && let Some(mut glass) = materials.get_mut(bulb)
            {
                glass.emissive = to;
            }
        }
        for (glow, mut light) in &mut glows {
            let turn = clock.t / GLOW_HOLDS + glow.0 as f32;
            let (from, share) = (turn.floor() as usize, turn.fract());
            let (a, b) = (
                fairies.colours[from % FAIRY],
                fairies.colours[(from + 1) % FAIRY],
            );
            let colour = Color::from(a * (1.0 - share) + b * share);
            let lumens = GLOW * (night + level);
            if light.intensity != lumens {
                light.intensity = lumens;
            }
            if light.color != colour {
                light.color = colour;
            }
        }
    }
    if !still && seen {
        redraw.write(RequestRedraw);
    }
}

/// The spot of one who comes to the bazaar, from its middle: in the
/// avenue, a little before the merchant and to its side.
pub(crate) const VISITOR: Vec3 = Vec3::new(-1.3, 0.0, 3.7);
