//! Pitching the bazaar: its meshes, lights, flags and people stood on
//! its ground.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hamlet_vfx::flag::{
    FlagMaterial, FlagSettings, Waving, cloth, flag,
};

use super::build::{self, FAIRY, PAVILION, POLE, RIM, placing};
use super::lights::{Fairies, Flare, Glow, Lanterns};
use super::merchant::{self, Merchant};
use super::shoppers::{self, Shopper};
use super::{
    Bazaar, BazaarSettings, Booth, CUSTOMER, Detail, GROUNDS, Lod,
    Pitched, SIZE, STALL, Stall, VISITOR, bustle, keepers,
};
use crate::chief::mascot::{self, Rig};
use crate::chief::{Chief, Grounds, Look, Settled, Visitor, place};
use crate::generator::land::Land;
use crate::generator::society::{Claim, Ground, Society};
use crate::lantern::NightLight;
use crate::palette::Palette;
use crate::pick::{self, Footprint, Named, Thing, hit_box};
use crate::{Clock, Stock};

/// The glows over the plaza and the promenade, from the bazaar's
/// middle.
const GLOWS: [Vec3; 3] = [
    Vec3::new(-2.6, 3.0, 6.6),
    Vec3::new(2.6, 3.0, 6.6),
    Vec3::new(0.0, 3.0, -9.0),
];

/// What a figure or a booth is made of.
pub(super) struct Making<'a> {
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub pal: &'a Palette,
    pub stock: &'a Stock,
    /// The material of what glows among the wares.
    pub gleam: Handle<StandardMaterial>,
}

impl Making<'_> {
    /// A figure on the rig of a chief's mascot, dressed in `look`,
    /// standing under `parent` at `at`. Returns its root and its
    /// rig.
    pub fn figure(
        &mut self,
        commands: &mut Commands,
        parent: Entity,
        look: Look,
        at: Vec3,
    ) -> (Entity, Rig) {
        let light = self.pal.wicks
            [look.light as usize % self.pal.wicks.len()];
        let flame = self.materials.add(StandardMaterial {
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
        let wick = self.meshes.add(mascot::wick().build());
        let rig = Rig::spawn(
            commands,
            root,
            &self.stock.matte,
            (wick, flame),
        );
        let dressed = mascot::parts(self.pal, look)
            .each()
            .map(|mb| self.meshes.add(mb.build()));
        for (entity, which) in rig.dressed() {
            commands
                .entity(entity)
                .insert(Mesh3d(dressed[which].clone()));
        }
        (root, rig)
    }
}

/// A holder of small things under `parent`, hidden with the rest
/// when the camera is not close.
fn holder(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Detail,
            Transform::IDENTITY,
            Visibility::Inherited,
            ChildOf(parent),
        ))
        .id()
}

/// Stands the bazaar in the world once every chief there is has its
/// own place: beside the first town, on ground nothing else holds.
pub(super) fn pitch(
    mut commands: Commands,
    (land, pal, stock, set, clock, pitched): (
        Res<Land>,
        Res<Palette>,
        Res<Stock>,
        Res<BazaarSettings>,
        Res<Clock>,
        Res<Pitched>,
    ),
    mut society: ResMut<Society>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    waiting: Query<(), (With<Chief>, Without<Settled>)>,
    mut flags: Option<(
        ResMut<Waving>,
        ResMut<Assets<FlagMaterial>>,
        Res<FlagSettings>,
    )>,
) {
    if !waiting.is_empty() {
        return;
    }
    let Some(at) = pitched.0.or_else(|| site(&land, &society)) else {
        return;
    };
    society.claims.push(Claim {
        place: Ground::Bazaar,
        at,
        radius: (land.set.town.keep_apart * 0.5).max(GROUNDS),
    });
    // The gate looks toward the first town.
    let home = land
        .sites
        .iter()
        .find(|site| site.buildable)
        .map_or(Vec2::ZERO, |site| site.at);
    let toward = (home - at).normalize_or(Vec2::Y);
    let built = build::build(&pal);
    let near = meshes.add(built.walls);
    let far = meshes.add(build::far(&pal));
    let glow = pal.glass_lit.to_linear();
    let lit = materials.add(StandardMaterial {
        base_color: pal.glass_lit,
        emissive: glow * 2.5,
        ..default()
    });
    let gleam = materials.add(StandardMaterial {
        unlit: true,
        ..default()
    });
    let colours = pal.fairy.map(|c| LinearRgba::rgb(c.x, c.y, c.z));
    let bulbs = colours.map(|colour| {
        materials.add(StandardMaterial {
            base_color: colour.into(),
            emissive: colour * set.bulb.0,
            ..default()
        })
    });
    let bazaar = commands
        .spawn((
            (
                Bazaar { held: at },
                Grounds(at, GROUNDS),
                Visitor(VISITOR),
            ),
            Lanterns {
                glass: lit.clone(),
                glow,
                gleam: gleam.clone(),
            },
            Fairies {
                bulbs: bulbs.clone(),
                colours,
            },
            Mesh3d(near.clone()),
            Lod {
                near,
                far,
                close: true,
            },
            MeshMaterial3d(stock.matte.clone()),
            Transform::from_translation(land.on_ground(at))
                .with_rotation(Quat::from_rotation_y(
                    toward.x.atan2(toward.y),
                )),
            Visibility::Inherited,
        ))
        .id();
    let small = holder(&mut commands, bazaar);
    commands.spawn((
        Mesh3d(meshes.add(built.glass)),
        MeshMaterial3d(lit),
        NotShadowCaster,
        ChildOf(small),
    ));
    commands.spawn((
        Mesh3d(meshes.add(built.gleam)),
        MeshMaterial3d(gleam.clone()),
        NotShadowCaster,
        ChildOf(small),
    ));
    for (mesh, bulb) in built.bulbs.into_iter().zip(bulbs) {
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(bulb),
            NotShadowCaster,
            ChildOf(small),
        ));
    }
    // Glows over the plaza before the pavilion, and over the
    // promenade behind it.
    for (k, at) in GLOWS.into_iter().enumerate() {
        commands.spawn((
            Glow(k * FAIRY / GLOWS.len()),
            light(0.0, set.glow_reach, Color::WHITE),
            Transform::from_translation(at),
            ChildOf(small),
        ));
    }
    if let Some((waving, cloths, flag_set)) = flags.as_mut() {
        for pennon in &built.flags {
            let mesh = meshes.add(cloth(
                flag_set,
                pennon.long,
                pennon.tall,
                &pennon.colours,
            ));
            commands.spawn((
                flag(
                    waving,
                    cloths,
                    flag_set,
                    clock.t,
                    mesh,
                    pennon.long,
                ),
                Transform::from_translation(pennon.at),
                ChildOf(small),
            ));
        }
    }
    for cog in built.cogs {
        commands.spawn((
            bustle::Turning {
                lies: cog.lies,
                rate: cog.rate,
            },
            Mesh3d(meshes.add(cog.mesh)),
            MeshMaterial3d(stock.matte.clone()),
            Transform::from_translation(cog.at)
                .with_rotation(cog.lies),
            ChildOf(small),
        ));
    }
    // A light under the pavilion's front that flares after a sale,
    // by day or by night.
    let ember =
        Color::linear_rgb(pal.ember.x, pal.ember.y, pal.ember.z);
    commands.spawn((
        Flare,
        light(0.0, set.flare_reach, ember),
        Transform::from_xyz(0.0, 3.0, 2.8),
        ChildOf(small),
    ));
    // Each lantern lights what is about it after dark.
    for lamp in built.lamps {
        commands.spawn((
            NightLight(set.lamp),
            light(0.0, set.lamp_reach, pal.glass_lit),
            Transform::from_translation(lamp),
            ChildOf(small),
        ));
    }
    // The bazaar itself is picked by its pavilion, and the stalls
    // about it by their own boxes. Its name is shown over the
    // pavilion, and its ring is as wide as all of it.
    let (wide, tall) = (RIM.0, POLE);
    picked(
        &mut commands,
        hit_box(
            Thing::Bazaar,
            Vec3::new(wide * 2.0, tall, wide * 2.0),
            PAVILION + Vec3::Y * tall * 0.5,
            &mut meshes,
        ),
        (bazaar, SIZE.0, tall),
    );
    let mut making = Making {
        meshes: &mut meshes,
        materials: &mut materials,
        pal: &pal,
        stock: &stock,
        gleam,
    };
    for stall in Stall::ALL {
        booth(&mut commands, &mut making, bazaar, stall);
    }
    bustle::scatter(&mut commands, &mut making, &set, small);
    // The merchant before the counter, and the shoppers going round
    // the stalls.
    let (keeper, rig) = making.figure(
        &mut commands,
        small,
        merchant::LOOK,
        merchant::STANDS,
    );
    commands.spawn((
        Mesh3d(making.meshes.add(merchant::turban(&pal).build())),
        MeshMaterial3d(stock.matte.clone()),
        ChildOf(rig.head),
    ));
    commands
        .entity(keeper)
        .insert((Merchant { sold: None }, rig));
    for (round, look) in shoppers::LOOKS.into_iter().enumerate() {
        let (shopper, rig) =
            making.figure(&mut commands, small, look, Vec3::ZERO);
        commands.entity(shopper).insert((Shopper { round }, rig));
    }
}

/// Where the land puts the bazaar: beside the first town, clear of
/// every town and every other claim.
pub(super) fn site(land: &Land, society: &Society) -> Option<Vec2> {
    let taken = society
        .towns
        .iter()
        .map(|town| land.sites[town.site].at)
        .chain(society.claims.iter().map(|claim| claim.at))
        .collect::<Vec<_>>();
    place::stand(land, &taken)
}

/// A point light that throws no shadow.
fn light(intensity: f32, range: f32, color: Color) -> PointLight {
    PointLight {
        color,
        intensity,
        range,
        shadow_maps_enabled: false,
        ..default()
    }
}

/// Spawns `boxed` under `parent`, named over it and ringed `radius`
/// wide and `tall` high, and has the pointer watch it.
fn picked(
    commands: &mut Commands,
    boxed: impl Bundle,
    (parent, radius, tall): (Entity, f32, f32),
) {
    let boxed = commands.spawn((boxed, ChildOf(parent))).id();
    // Inserted after, to take the place of the box's own footprint.
    commands.entity(boxed).insert((
        Named,
        Footprint {
            radius,
            height: tall,
            lift: tall * 0.5,
        },
    ));
    pick::watch(commands, boxed);
}

/// Stands `stall` in `bazaar`: its box to pick it by, with its name
/// over it and its ring about it, and its keeper.
fn booth(
    commands: &mut Commands,
    making: &mut Making,
    bazaar: Entity,
    stall: Stall,
) {
    let (at, turn) = placing(stall);
    let booth = commands
        .spawn((
            Booth { stall },
            Visitor(CUSTOMER),
            Transform::from_translation(at).with_rotation(turn),
            Visibility::Inherited,
            ChildOf(bazaar),
        ))
        .id();
    let (reach, tall, out) = STALL;
    picked(
        commands,
        hit_box(
            Thing::Stall(stall),
            Vec3::new(reach * 2.0, tall, 2.6),
            Vec3::new(0.0, tall * 0.5, out),
            making.meshes,
        ),
        (booth, reach, tall),
    );
    let small = holder(commands, booth);
    keepers::stand(commands, making, (booth, small), stall);
}
