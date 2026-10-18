//! A chief given its ground: the place and the mascot spawned, the
//! ground held against the towns, and both taken away again.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::dress::lit;
use super::mascot::{self, Rig};
use super::{
    Chief, ChiefSettings, Look, Mascot, Place, Settled, Stands, place,
};
use crate::generator::land::Land;
use crate::generator::society::{Claim, Place as Ground, Society};
use crate::generator::{flora, glow};
use crate::lantern::lantern_glass;
use crate::library::{self, Library};
use crate::palette::Palette;
use crate::pick::{self, Footprint, Named, Thing, hit_box};
use crate::{Clock, Stock};

/// Takes the place and the mascot of a chief out of the world, and
/// frees the ground the place held.
fn vacate(
    commands: &mut Commands,
    settled: &Settled,
    places: &Query<&Place>,
    society: &mut Society,
) {
    if let Ok(place) = places.get(settled.place) {
        let held = place.held;
        society.claims.retain(|claim| {
            claim.place != Ground::Chief || claim.at != held
        });
    }
    for entity in [settled.place, settled.mascot] {
        commands.entity(entity).try_despawn();
    }
}

/// Takes up the place of a chief that has set it down elsewhere,
/// so that it is settled anew where it now stands.
pub(super) fn resettle(
    mut commands: Commands,
    moved: Query<(Entity, &Settled), Changed<Stands>>,
    places: Query<&Place>,
    mut society: ResMut<Society>,
) {
    for (chief, settled) in &moved {
        vacate(&mut commands, settled, &places, &mut society);
        commands.entity(chief).remove::<Settled>();
    }
}

/// Keeps the ground of every chief's place level and clear of
/// trees, as a town's site is.
pub(super) fn hold_ground(
    set: Res<ChiefSettings>,
    places: Query<&Place>,
    libraries: Query<&Library>,
    mut land: ResMut<Land>,
) {
    let mut rounds = places
        .iter()
        .map(|place| (place.held, set.grounds))
        .chain(
            libraries
                .iter()
                .map(|library| (library.held, library::GROUNDS)),
        )
        .collect::<Vec<_>>();
    rounds.sort_by(|a, b| {
        a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y))
    });
    if land.held() != rounds {
        land.hold(&rounds);
    }
}

/// Gives every chief the world has not met a place and a mascot.
#[allow(clippy::too_many_arguments)]
pub(super) fn settle(
    mut commands: Commands,
    clock: Res<Clock>,
    land: Res<Land>,
    set: Res<ChiefSettings>,
    pal: Res<Palette>,
    stock: Res<Stock>,
    mut society: ResMut<Society>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new: Query<
        (Entity, Option<&Stands>),
        (With<Chief>, Without<Settled>),
    >,
    places: Query<&Transform, With<Place>>,
) {
    if new.is_empty() {
        return;
    }
    let mut taken = society
        .towns
        .iter()
        .map(|town| land.sites[town.site].at)
        .chain(places.iter().map(|at| at.translation.xz()))
        .chain(society.claims.iter().map(|claim| claim.at))
        .collect::<Vec<_>>();
    for (chief, stands) in &new {
        let chosen = stands.map(|stands| stands.0);
        let Some(at) = chosen.or_else(|| place::stand(&land, &taken))
        else {
            continue;
        };
        taken.push(at);
        // The door looks toward the first town.
        let home = land
            .sites
            .iter()
            .find(|site| site.buildable)
            .map_or(Vec2::ZERO, |site| site.at);
        let toward = (home - at).normalize_or(Vec2::Y);
        let yaw = toward.x.atan2(toward.y);
        let frame = Transform::from_translation(land.on_ground(at))
            .with_rotation(Quat::from_rotation_y(yaw));
        // No town is founded or set down on a chief's ground.
        society.claims.push(Claim {
            place: Ground::Chief,
            at,
            radius: land.set.town.keep_apart * 0.5,
        });

        let built =
            place::build(&pal, &set, Look::default(), |_| 0.0);
        let [orb, ring, glass] = [
            lit(Vec3::ONE, 1.0),
            glow(Vec3::ONE, 1.0),
            StandardMaterial {
                base_color: pal.glass_lit,
                perceptual_roughness: 0.4,
                ..default()
            },
        ]
        .map(|material| materials.add(material));
        let panes = commands
            .spawn((
                Mesh3d::default(),
                MeshMaterial3d(stock.windows.clone()),
                NotShadowCaster,
            ))
            .id();
        let (orb_at, orb_size) = built.lights.orb;
        let (ring_at, ring_size) = built.lights.ring;
        let place = commands
            .spawn((
                Place {
                    chief,
                    panes,
                    held: at,
                    orb: orb.clone(),
                    ring: ring.clone(),
                    glass: glass.clone(),
                },
                Mesh3d::default(),
                MeshMaterial3d(stock.matte.clone()),
                frame,
                Visibility::Inherited,
                children![
                    (
                        Mesh3d(meshes.add(
                            Sphere::new(1.0).mesh().ico(2).unwrap()
                        )),
                        MeshMaterial3d(orb),
                        Transform::from_translation(orb_at)
                            .with_scale(Vec3::splat(orb_size)),
                        NotShadowCaster,
                    ),
                    (
                        Mesh3d(
                            meshes.add(
                                Annulus::new(0.9, 1.0)
                                    .mesh()
                                    .resolution(32)
                            )
                        ),
                        MeshMaterial3d(ring),
                        Transform::from_translation(ring_at)
                            .with_rotation(Quat::from_rotation_x(
                                -std::f32::consts::FRAC_PI_2,
                            ))
                            .with_scale(Vec3::splat(ring_size)),
                        NotShadowCaster,
                    ),
                    (
                        Mesh3d(meshes.add(lantern_glass(0.1, 0.24))),
                        MeshMaterial3d(glass),
                        Transform::from_translation(
                            built.lights.lantern
                        ),
                        NotShadowCaster,
                    ),
                ],
            ))
            .add_child(panes)
            .id();
        for tree in &built.trees {
            commands.spawn((
                Mesh3d(meshes.add(flora::mesh(&pal, tree.kind))),
                MeshMaterial3d(stock.matte.clone()),
                Transform::from_translation(tree.at)
                    .with_scale(Vec3::splat(tree.scale)),
                ChildOf(place),
            ));
        }
        let (radius, height) = set.house;
        let house = place::house_at(&set);
        let boxed = commands
            .spawn((
                hit_box(
                    Thing::ChiefPlace(chief),
                    Vec3::new(radius * 1.3, height, radius * 1.3),
                    Vec3::new(house.x, height * 0.5, house.y),
                    &mut meshes,
                ),
                ChildOf(place),
            ))
            .id();
        commands.entity(boxed).insert((
            Named,
            Footprint {
                radius,
                height,
                lift: height * 0.5,
            },
        ));
        pick::watch(&mut commands, boxed);

        // The mascot, on the waystone and looking out of the gate.
        let stone = place::spots(&set)[0].at;
        let stone = frame
            .transform_point(Vec3::new(stone.x, 0.0, stone.y))
            .xz();
        let flame = materials.add(lit(Vec3::ONE, 1.0));
        let mascot = commands
            .spawn((
                Mascot::new(
                    chief,
                    flame.clone(),
                    yaw,
                    clock.t,
                    clock.t + set.linger.0,
                ),
                Transform::from_translation(land.on_ground(stone))
                    .with_rotation(Quat::from_rotation_y(yaw)),
                Visibility::Inherited,
            ))
            .id();
        let wick = meshes.add(mascot::wick().build());
        let rig = Rig::spawn(
            &mut commands,
            mascot,
            &stock.matte,
            (wick, flame),
        );
        let boxed = commands
            .spawn((
                hit_box(
                    Thing::Chief(chief),
                    set.mascot_box,
                    Vec3::Y * set.mascot_box.y * 0.5,
                    &mut meshes,
                ),
                ChildOf(mascot),
            ))
            .id();
        pick::watch(&mut commands, boxed);
        commands.entity(mascot).insert(rig);
        commands.entity(chief).insert(Settled { place, mascot });
    }
}

/// A chief is gone from the world: its place and its mascot go, and
/// its site is free for a town again.
pub(super) fn leave(
    gone: On<Remove, Chief>,
    mut commands: Commands,
    chiefs: Query<&Settled>,
    places: Query<&Place>,
    mut society: ResMut<Society>,
) {
    if let Ok(settled) = chiefs.get(gone.entity) {
        vacate(&mut commands, settled, &places, &mut society);
    }
}
