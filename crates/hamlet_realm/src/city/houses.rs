//! The houses of the scholars in the city: one for each scholar that
//! has a spot, with its name over it, a lit window for each of its
//! hires that lives, and its windows shuttered once the scholar has
//! left.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hamlet_client::realm::{Identity, Left, Scholar};

use super::{Glass, Level};
use crate::generator::house::{self, Banner, TownStyle};
use crate::generator::land::{IslandId, Land};
use crate::generator::society::AgentId;
use crate::lantern::NightLight;
use crate::pick::{self, Named, PickSettings, Thing, hit_box};
use crate::stock::Stock;

/// The radius of the ground a house keeps level and clear.
const GROUNDS: f32 = 4.6;

/// The light of the lantern by a house's door after dark, and how
/// far it reaches.
const LANTERN: (f32, f32) = (30_000.0, 6.0);

/// A scholar's house, and what it was built as: where it stands, the
/// way its door faces, how many of its windows are lit and whether
/// it is shuttered.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(super) struct Home {
    scholar: Entity,
    at: Vec2,
    turn: f32,
    lit: usize,
    shut: bool,
}

impl Home {
    /// How many of its windows are lit, and whether they are
    /// shuttered.
    #[cfg(test)]
    pub(super) fn windows(&self) -> (usize, bool) {
        (self.lit, self.shut)
    }
}

/// The windows to light for `hires`, those of them that live.
fn lit(
    hires: &[hamlet_protocol::client::AgentId],
    living: &[u64],
) -> usize {
    hires.iter().filter(|hire| living.contains(&hire.0)).count()
}

/// The house every scholar with a spot has now.
fn wanted(
    land: &Land,
    scholars: &Query<(Entity, &Scholar)>,
    living: &[u64],
) -> Vec<Home> {
    let city = land.island(IslandId::City).centre;
    let mut out = scholars
        .iter()
        .filter_map(|(entity, scholar)| {
            let at = scholar.0.house.as_ref()?;
            let shut = scholar.0.left.is_some();
            Some(Home {
                scholar: entity,
                at: city + Vec2::new(at.x as f32, at.z as f32),
                turn: at.turn as f32,
                lit: if shut {
                    0
                } else {
                    lit(&scholar.0.hires, living)
                },
                shut,
            })
        })
        .collect::<Vec<_>>();
    out.sort_by_key(|home| home.scholar);
    out
}

/// The places of the houses the scholars have, to grow the city
/// about.
pub(super) fn spots(
    land: &Land,
    scholars: &Query<(Entity, &Scholar)>,
) -> Vec<Vec2> {
    wanted(land, scholars, &[])
        .into_iter()
        .map(|home| home.at)
        .collect()
}

/// Raises the house of every scholar that has none, takes down the
/// house of one that has gone, and builds a house again when its
/// windows have changed.
pub(super) fn tend(
    mut commands: Commands,
    land: Res<Land>,
    glass: Res<Glass>,
    stock: Res<Stock>,
    pick: Res<PickSettings>,
    scholars: Query<(Entity, &Scholar)>,
    agents: Query<&Identity, Without<Left>>,
    homes: Query<(Entity, &Home)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let living = agents.iter().map(|id| id.0.0).collect::<Vec<_>>();
    let wanted = wanted(&land, &scholars, &living);
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
        let Ok((_, scholar)) = scholars.get(want.scholar) else {
            continue;
        };
        let number = scholar.0.id.0;
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
        // The first panes are the lit ones, the rest are dark, or
        // shuttered.
        let lit = built.panes.take_quads(want.lit);
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
        if want.lit > 0 {
            commands.spawn((
                Mesh3d(meshes.add(lit.build())),
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
                    Thing::Scholar(want.scholar),
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
