//! The houses of the profiles in the city: one for each profile that
//! has a spot, with its name over it, a glowing window for each of
//! its hires that lives, and its windows shuttered once the profile
//! has left.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hamlet_client::realm::{Identity, Left, Profile};
use hamlet_protocol::client::HouseAt;

use super::{Glass, Level};
use crate::generator::house::{self, Banner, TownStyle};
use crate::generator::land::{IslandId, Land};
use crate::generator::society::{AgentId, Claim, Ground, Society};
use crate::lantern::NightLight;
use crate::pick::{self, Named, PickSettings, Thing, hit_box};
use crate::stock::Stock;

/// The radius of the ground a house keeps level and clear.
const GROUNDS: f32 = 4.6;

/// How far before a house its door's ground lies.
pub(super) const DOOR_OUT: f32 = 4.0;

/// The light of the lantern by a house's door after dark, and how
/// far it reaches.
const LANTERN: (f32, f32) = (30_000.0, 6.0);

/// A profile's house, and what it was built as: where it stands, the
/// way its door faces, how many of its windows glow and whether it
/// is shuttered.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(super) struct Home {
    pub(super) profile: Entity,
    pub(super) at: Vec2,
    pub(super) turn: f32,
    glowing: usize,
    pub(super) shut: bool,
}

impl Home {
    /// How many of its windows glow, and whether they are shuttered.
    #[cfg(test)]
    pub(super) fn windows(&self) -> (usize, bool) {
        (self.glowing, self.shut)
    }
}

/// The windows to glow for `hires`, those of them that live.
fn glowing(
    hires: &[hamlet_protocol::client::AgentId],
    living: &[u64],
) -> usize {
    hires.iter().filter(|hire| living.contains(&hire.0)).count()
}

/// The house every profile with a spot has now.
pub(super) fn wanted(
    land: &Land,
    profiles: &Query<(Entity, &Profile)>,
    living: &[u64],
) -> Vec<Home> {
    let city = land.island(IslandId::City).centre;
    let mut out = profiles
        .iter()
        .filter_map(|(entity, profile)| {
            let at = profile.0.house.as_ref()?;
            let shut = profile.0.left.is_some();
            Some(Home {
                profile: entity,
                at: city + Vec2::new(at.x as f32, at.z as f32),
                turn: at.turn as f32,
                glowing: if shut {
                    0
                } else {
                    glowing(&profile.0.hires, living)
                },
                shut,
            })
        })
        .collect::<Vec<_>>();
    out.sort_by_key(|home| home.profile);
    out
}

/// The ground before the door of a profile's house.
pub(crate) fn door(land: &Land, house: &HouseAt) -> Vec2 {
    let city = land.island(IslandId::City).centre;
    let turn = house.turn as f32;
    city + Vec2::new(house.x as f32, house.z as f32)
        + Vec2::new(turn.sin(), turn.cos()) * DOOR_OUT
}

/// The places of the houses the profiles have, to grow the city
/// about.
pub(super) fn spots(
    land: &Land,
    profiles: &Query<(Entity, &Profile)>,
) -> Vec<Vec2> {
    wanted(land, profiles, &[])
        .into_iter()
        .map(|home| home.at)
        .collect()
}

/// Half the distance the server keeps between two houses.
const ROOM: f32 = (HouseAt::APART * 0.5) as f32;

/// Gives the house of every profile that has not left a claim on
/// its ground, and takes the claims of houses that are gone.
pub(super) fn claim(
    land: Res<Land>,
    profiles: Query<(Entity, &Profile)>,
    mut society: ResMut<Society>,
) {
    let wanted = wanted(&land, &profiles, &[])
        .into_iter()
        .filter(|home| !home.shut)
        .filter_map(|home| {
            let (_, profile) = profiles.get(home.profile).ok()?;
            Some(Claim {
                place: Ground::House(profile.0.id),
                at: home.at,
                radius: ROOM,
            })
        })
        .collect::<Vec<_>>();
    let held =
        |claim: &&Claim| matches!(claim.place, Ground::House(_));
    let have = society.claims.iter().filter(held).count();
    if have == wanted.len()
        && wanted.iter().all(|claim| society.claims.contains(claim))
    {
        return;
    }
    society.claims.retain(|claim| !held(&claim));
    society.claims.extend(wanted);
}

/// Raises the house of every profile that has none, takes down the
/// house of one that has gone, and builds a house again when its
/// windows have changed.
pub(super) fn tend(
    mut commands: Commands,
    land: Res<Land>,
    glass: Res<Glass>,
    stock: Res<Stock>,
    pick: Res<PickSettings>,
    profiles: Query<(Entity, &Profile)>,
    agents: Query<&Identity, Without<Left>>,
    homes: Query<(Entity, &Home)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let living = agents.iter().map(|id| id.0.0).collect::<Vec<_>>();
    let wanted = wanted(&land, &profiles, &living);
    for (entity, home) in &homes {
        if !wanted.contains(home) {
            commands.entity(entity).despawn();
        }
    }
    let (radius, tall) = pick.house;
    for want in wanted {
        if homes.iter().any(|(_, home)| *home == want) {
            continue;
        }
        let Ok((_, profile)) = profiles.get(want.profile) else {
            continue;
        };
        let number = profile.0.id.0;
        let agent = AgentId(number as u32);
        let style = TownStyle::of(&land, number as usize);
        let yard = house::yard_of(&land, agent, false, 0.0, 0);
        let mut built = house::house(
            &land,
            agent,
            &style,
            false,
            Banner::None,
            0,
            &yard,
        );
        // The first panes are the glowing ones, the rest are dark, or
        // shuttered.
        let glow = built.panes.take_quads(want.glowing);
        let yaw = Quat::from_rotation_y(want.turn);
        let lamp = built.lamp;
        let house = commands
            .spawn((
                want,
                Level(want.at, GROUNDS),
                Mesh3d(meshes.add(built.body.build())),
                MeshMaterial3d(stock.matte.clone()),
                Transform::from_translation(land.on_ground(want.at))
                    .with_rotation(yaw),
                Visibility::Inherited,
            ))
            .id();
        let dark = if want.shut {
            glass.shut.clone()
        } else {
            glass.dark.clone()
        };
        commands.spawn((
            Mesh3d(meshes.add(built.panes.build())),
            MeshMaterial3d(dark),
            NotShadowCaster,
            ChildOf(house),
        ));
        if want.glowing > 0 {
            commands.spawn((
                Mesh3d(meshes.add(glow.build())),
                MeshMaterial3d(glass.windows.clone()),
                NotShadowCaster,
                ChildOf(house),
            ));
        }
        if !want.shut {
            let (lumens, reach) = LANTERN;
            commands.spawn((
                NightLight(lumens),
                PointLight {
                    color: land.pal.glass_lit,
                    intensity: 0.0,
                    range: reach,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(lamp),
                ChildOf(house),
            ));
        }
        let boxed = commands
            .spawn((
                hit_box(
                    Thing::Profile(want.profile),
                    Vec3::new(radius * 2.0, tall, radius * 2.0),
                    Vec3::Y * tall * 0.5,
                    &mut meshes,
                ),
                ChildOf(house),
            ))
            .id();
        commands.entity(boxed).insert((
            Named,
            pick::Footprint {
                radius,
                height: tall,
                lift: tall * 0.5,
            },
        ));
        pick::watch(&mut commands, boxed);
    }
}
