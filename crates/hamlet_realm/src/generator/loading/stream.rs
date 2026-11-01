//! The ground, the sea and the trees, drawn about where the camera
//! looks.

use std::collections::HashSet;

use bevy::camera::primitives::Aabb;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use bevy::window::RequestRedraw;
use hamlet_vfx::patch::{self, Patch};
use hamlet_vfx::water::{self, Waves};

use super::lanes::Lanes;
use super::{Bed, Loading, Sea, Standing, Terrain, Wood};
use crate::WorldCamera;
use crate::generator::building::mesh_bytes;
use crate::generator::ground::{self, Sight, Stand};
use crate::generator::land::{Land, LandData};
use crate::generator::view::Ahead;
use crate::generator::{DetailSettings, Kit, Tally};
use crate::sky::{CameraSettings, DistanceFogSettings};

/// Where a patch is drawn: moved to its corner on the plane `y` high,
/// and as large as its side.
fn placed(patch: Patch, y: f32) -> Transform {
    Transform::from_xyz(patch.corner.x, y, patch.corner.y)
        .with_scale(Vec3::new(patch.side, 1.0, patch.side))
}

/// The camera's sight of the ground: as far as the fog lets it.
fn sight(
    land: &Land,
    detail: &DetailSettings,
    camera: &Transform,
    fog: Option<&DistanceFog>,
    far: f32,
) -> Sight {
    let eye = camera.translation;
    let ahead = camera.forward().as_vec3();
    // The point of the ground it looks at, found where its line
    // meets the plains, then where it meets the ground or the sea
    // about there. A camera that looks at none counts as looking
    // straight down.
    let meets = |level: f32| {
        let fall = (eye.y - level).max(1.0);
        let run = if ahead.y < -0.05 {
            (fall / -ahead.y).min(far)
        } else {
            0.0
        };
        (eye + ahead * run).xz()
    };
    let plains = meets(land.set.land.site_y);
    let centre =
        meets(land.height(plains).max(land.set.land.water_y));
    let whole = match fog.map(|fog| &fog.falloff) {
        Some(FogFalloff::Linear { end, .. }) => *end,
        _ => far,
    };
    Sight {
        eye,
        centre,
        reach: whole.min(far) * detail.ground_reach,
    }
}

/// Keeps the ground, the sea and the trees drawn about where the
/// camera looks: lays the patches of the ground and the sea, asks for
/// the tiles of trees the view calls for, puts those that are built
/// into the world, and takes out those the view has left behind.
pub(in crate::generator) fn stream(
    mut commands: Commands,
    mut loading: ResMut<Loading>,
    land: Res<Land>,
    detail: Res<DetailSettings>,
    kit: Res<Kit>,
    lanes: Res<Lanes>,
    ahead: Option<Res<Ahead>>,
    (lens, fog_set): (
        Option<Res<CameraSettings>>,
        Option<Res<DistanceFogSettings>>,
    ),
    camera: Query<
        (&Transform, &Projection, Option<&DistanceFog>),
        (With<WorldCamera>, Without<Wood>),
    >,
    mut laid: Query<
        (Entity, &mut Transform, Has<Sea>, Has<Terrain>),
        (
            Or<(With<Bed>, With<Sea>, With<Terrain>)>,
            Without<WorldCamera>,
            Without<Wood>,
        ),
    >,
    mut places: Query<&mut Transform, With<Wood>>,
    standing: Query<(), With<Standing>>,
    mut tally: ResMut<Tally>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let loading = &mut *loading;
    let chunk = land.set.land.chunk;
    let Some((at, projection, fog)) = camera.iter().next() else {
        // Nothing looks at the world, so no ground is needed.
        loading.landed = true;
        return;
    };
    let far = match projection {
        Projection::Perspective(lens) => lens.far,
        _ => 4000.0,
    };
    let sight = sight(&land, &detail, at, fog, far);
    // The sea and the ground in patches about the eye, each about as
    // large to it: one entity for each patch, kept from frame to
    // frame, more spawned or the spare ones let go as the patches
    // change. The ground's are chosen on the plane its plains lie on
    // and its shader raises them to the land; the sea's lie flat on
    // the water.
    let water_y = land.set.land.water_y;
    let mut sea =
        patch::choose(at.translation, water_y, water::FINEST, far)
            .into_iter()
            .map(|patch| placed(patch, water_y));
    let finest = land.set.land.cell * patch::QUADS as f32;
    let terrain = patch::choose(
        at.translation,
        land.set.land.plain_y,
        finest,
        sight.reach,
    );
    let ground_patches = terrain.len();
    let mut terrain =
        terrain.into_iter().map(|patch| placed(patch, 0.0));
    for (entity, mut place, is_sea, is_terrain) in &mut laid {
        let next = if is_sea || is_terrain {
            let next =
                if is_sea { sea.next() } else { terrain.next() };
            let Some(next) = next else {
                commands.entity(entity).despawn();
                continue;
            };
            next
        } else {
            Transform::from_xyz(
                sight.centre.x,
                water_y - detail.bed,
                sight.centre.y,
            )
            .with_scale(Vec3::splat(far * 2.0))
        };
        if *place != next {
            *place = next;
        }
    }
    for at in sea {
        commands.spawn((
            Sea,
            Waves,
            Mesh3d(kit.patch.clone()),
            MeshMaterial3d(kit.water.clone()),
            at,
            NotShadowCaster,
            // Its swells rise and lean beyond the flat square its
            // bounds would be taken from.
            Aabb::from_min_max(
                Vec3::new(-0.1, -3.0, -0.1),
                Vec3::new(1.1, 3.0, 1.1),
            ),
        ));
    }
    for at in terrain {
        commands.spawn((
            Terrain,
            Mesh3d(kit.patch.clone()),
            MeshMaterial3d(kit.ground.clone()),
            at,
            NotShadowCaster,
            // Its shader raises it from the plane to the land, from
            // the sea floor to the tops of the hills.
            Aabb::from_min_max(
                Vec3::new(-0.05, -30.0, -0.05),
                Vec3::new(1.05, 40.0, 1.05),
            ),
        ));
    }
    // The place the camera is being taken to, so that the ground is
    // there before it.
    let going = ahead
        .and_then(|ahead| ahead.0)
        .zip(lens)
        .map(|(view, lens)| {
            view.sight(&land, &lens, fog_set.as_deref(), &detail)
        })
        .unwrap_or(sight);

    // Ground a site came to or left is built anew, and so are the
    // trees about it.
    let mut changed = false;
    if !land.sites.iter().map(|s| &s.at).eq(&loading.sites) {
        let sites =
            land.sites.iter().map(|s| s.at).collect::<Vec<_>>();
        let hold = land.set.town.radius + 17.0;
        for i in 0..sites.len().max(loading.sites.len()) {
            let (now, was) = (sites.get(i), loading.sites.get(i));
            if now == was {
                continue;
            }
            for at in [now, was].into_iter().flatten() {
                loading.groves.redo(chunk, *at, hold);
            }
        }
        loading.sites = sites;
        changed = true;
    }
    // So is the coast about a town that came, went or moved.
    if !land.shores().eq(loading.shores.iter().copied()) {
        let shores = land.shores().collect::<Vec<_>>();
        let set = &land.set.land;
        let reach = set.island + set.coast.1 + set.rim.1 + 30.0;
        for at in shores.iter().chain(&loading.shores) {
            if shores.contains(at) && loading.shores.contains(at) {
                continue;
            }
            loading.groves.redo(chunk, *at, reach);
        }
        loading.shores = shores;
        changed = true;
    }
    // So is ground that came to be held or was let go.
    if !land.rounds().eq(loading.holds.iter().copied()) {
        let holds = land.held();
        for round in holds.iter().chain(&loading.holds) {
            if holds.contains(round) && loading.holds.contains(round)
            {
                continue;
            }
            let reach = LandData::hold_reach(round.1) + 17.0;
            loading.groves.redo(chunk, round.0, reach);
        }
        loading.holds = holds;
        changed = true;
    }

    let moved = |was: &Sight, now: &Sight| {
        was.eye.distance(now.eye) > 0.5
            || was.centre.distance(now.centre) > 0.5
            || (was.reach - now.reach).abs() > 1.0
    };
    let looked = loading.sight.is_none_or(|was| {
        moved(&was.0, &sight) || moved(&was.1, &going)
    });
    if looked || changed {
        let views = (&sight, &going);
        loading.groves.choose(&land, &detail, views, |_, _| 0);
        loading.sight = Some((sight, going));
    }

    let waiting = &mut loading.waiting;
    let came =
        loading.groves.land(&mut commands, |commands, _, grove| {
            // A wood with trees to plant is shown when they stand.
            let whole = grove.trees.is_empty();
            let wood = commands
                .spawn((
                    Wood,
                    Transform::default(),
                    if whole {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ))
                .id();
            let bytes = grove.canopy.as_ref().map_or(0, mesh_bytes);
            if let Some(canopy) = grove.canopy {
                commands.entity(wood).try_insert((
                    Mesh3d(meshes.add(canopy)),
                    MeshMaterial3d(kit.matte.clone()),
                ));
            }
            waiting.extend(
                grove.trees.into_iter().map(|tree| (wood, tree)),
            );
            (wood, bytes, whole)
        });

    let top = detail.tile_levels.max(1) - 1;
    loading.groves.retire(&mut commands, top, 0.0, &mut places);

    let pool = AsyncComputeTaskPool::get();
    loading.groves.build(
        chunk,
        going.centre,
        detail.tile_tasks,
        |tile, _| {
            let stand = Stand::of(&detail, tile);
            let land = land.clone();
            pool.spawn(
                async move { ground::grove(&land, tile, stand) },
            )
        },
    );

    let plant = loading.waiting.len().min(detail.trees_per_frame);
    let from = loading.waiting.len() - plant;
    let mut planted = HashSet::new();
    for (wood, tree) in loading.waiting.drain(from..) {
        planted.insert(wood);
        if lanes.is_on_lane(tree.at.xz()) {
            continue;
        }
        let tree = commands
            .spawn((
                Standing,
                Mesh3d(kit.trees[tree.kind].clone()),
                MeshMaterial3d(kit.matte.clone()),
                Transform::from_translation(tree.at)
                    .with_rotation(Quat::from_rotation_y(tree.yaw))
                    .with_scale(Vec3::splat(tree.scale)),
            ))
            .id();
        // Its wood may have been taken out this very frame.
        crate::adopt(&mut commands, wood, tree);
    }
    // A wood whose last tree was planted takes the place of what it
    // replaces.
    for (wood, _) in &loading.waiting {
        planted.remove(wood);
    }
    for wood in planted {
        loading.groves.whole(&mut commands, wood);
    }
    if !loading.landed && loading.is_settled() {
        loading.landed = true;
    }
    if came > 0 || plant > 0 || !loading.is_settled() {
        tally.tiles = ground_patches;
        tally.trees = standing.iter().len();
        // Keep frames coming while the world fills in.
        redraw.write(RequestRedraw);
    }
}
