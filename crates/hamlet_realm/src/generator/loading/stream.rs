//! The ground and the trees, drawn about where the camera looks.

use std::collections::HashSet;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use bevy::window::RequestRedraw;

use super::lanes::Lanes;
use super::{Bed, Ground, Loading, Sea, Standing, Wood};
use crate::WorldCamera;
use crate::generator::building::mesh_bytes;
use crate::generator::ground::{self, Recipe, Sight, Stand};
use crate::generator::land::{Land, LandData};
use crate::generator::view::Ahead;
use crate::generator::{DetailSettings, Kit, Tally};
use crate::sky::{CameraSettings, DistanceFogSettings};

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
    // The point of the ground it looks at. A camera that looks at
    // none counts as looking straight down.
    let level = land.set.land.site_y;
    let fall = (eye.y - level).max(1.0);
    let run = if ahead.y < -0.05 {
        (fall / -ahead.y).min(far)
    } else {
        0.0
    };
    let centre = (eye + ahead * run).xz();
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

/// Keeps the ground and the trees drawn about where the camera
/// looks: asks for the tiles the view calls for, puts those that are
/// built into the world, and takes out those the view has left
/// behind.
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
        (With<WorldCamera>, Without<Ground>),
    >,
    mut bed: Query<
        (&mut Transform, Has<Sea>),
        (
            Or<(With<Bed>, With<Sea>)>,
            Without<WorldCamera>,
            Without<Ground>,
        ),
    >,
    mut places: Query<&mut Transform, With<Ground>>,
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
    for (mut bed, is_sea) in &mut bed {
        let below = if is_sea { 0.0 } else { detail.bed };
        let next = Transform::from_xyz(
            sight.centre.x,
            land.set.land.water_y - below,
            sight.centre.y,
        )
        .with_scale(Vec3::splat(far * 2.0));
        if *bed != next {
            *bed = next;
        }
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
                loading.ground.redo(chunk, *at, hold);
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
        let reach = set.island + set.coast.1 + set.shore;
        for at in shores.iter().chain(&loading.shores) {
            if shores.contains(at) && loading.shores.contains(at) {
                continue;
            }
            loading.ground.redo(chunk, *at, reach);
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
            loading.ground.redo(chunk, round.0, reach);
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
        let top = detail.tile_levels.max(1) - 1;
        // A tile of ground is cut to the tiles drawn beside it.
        loading.ground.choose(
            &land,
            &detail,
            views,
            true,
            |drawn, tile| ground::rim_key(drawn, top, tile),
        );
        loading
            .groves
            .choose(&land, &detail, views, false, |_, _| 0);
        loading.sight = Some((sight, going));
    }

    let mut came =
        loading.ground.land(&mut commands, |commands, tile, sod| {
            let bytes = mesh_bytes(&sod.mesh);
            let entity = commands
                .spawn((
                    Ground(tile),
                    Mesh3d(meshes.add(sod.mesh)),
                    MeshMaterial3d(kit.ground.clone()),
                    NotShadowCaster,
                    Transform::default(),
                    Visibility::Inherited,
                ))
                .id();
            (entity, bytes, true)
        });
    let waiting = &mut loading.waiting;
    came +=
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
    loading.ground.retire(
        &mut commands,
        top,
        detail.stale_sink,
        &mut places,
    );
    loading.groves.retire(&mut commands, top, 0.0, &mut places);

    let pool = AsyncComputeTaskPool::get();
    loading.ground.build(
        chunk,
        going.centre,
        detail.tile_tasks,
        |tile, drawn| {
            let recipe = Recipe::of(&land, &detail, tile);
            let (land, drawn) = (land.clone(), drawn.clone());
            pool.spawn(
                async move { ground::sod(&land, recipe, &drawn) },
            )
        },
    );
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
        tally.tiles = loading.ground.drawn().count();
        tally.trees = standing.iter().len();
        // Keep frames coming while the world fills in.
        redraw.write(RequestRedraw);
    }
}
