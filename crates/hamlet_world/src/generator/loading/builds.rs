//! Finished houses, roads and commons, put into the world.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_vfx::flag::{FlagMaterial, FlagSettings, Waving, flag};

use super::Loading;
use crate::Clock;
use crate::generator::building::Finished;
use crate::generator::lights::{Lamp, Spill};
use crate::generator::shift::Shift;
use crate::generator::society::Society;
use crate::generator::sync::{Drawn, RoadDrawn};
use crate::generator::{
    DetailSettings, Kit, Phase, Road, Tally, house,
};
use crate::keeper::KState;
use crate::sky::SkySettings;

/// Puts whatever the background tasks have finished into the world.
pub(in crate::generator) fn finish_loading(
    mut commands: Commands,
    mut loading: ResMut<Loading>,
    kit: Res<Kit>,
    detail: Res<DetailSettings>,
    sky: Res<SkySettings>,
    society: Res<Society>,
    phase: Res<State<Phase>>,
    mut next: ResMut<NextState<Phase>>,
    mut drawn: ResMut<Drawn>,
    mut tally: ResMut<Tally>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut flags: Option<(
        ResMut<Waving>,
        ResMut<Assets<FlagMaterial>>,
        Res<FlagSettings>,
    )>,
    clock: Res<Clock>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if loading.landed && *phase.get() == Phase::Loading {
        next.set(Phase::Ready);
    }
    if loading.builds.is_empty() {
        return;
    }
    let loading = &mut *loading;
    let finished = loading.builds.finished(detail.builds_per_frame);
    place_builds(
        &mut commands,
        finished,
        &kit,
        &sky,
        &society,
        &mut drawn,
        &mut meshes,
        flags.as_mut().map(|(waving, cloth, set)| {
            (&mut **waving, &mut **cloth, &**set)
        }),
        clock.t,
    );
    tally.count(&society, &drawn);
    // Keep frames coming while the world fills in.
    redraw.write(RequestRedraw);
}

/// Puts finished houses, roads and commons into the world, unless
/// the society has moved on since they were asked for.
fn place_builds(
    commands: &mut Commands,
    finished: Finished,
    kit: &Kit,
    sky: &SkySettings,
    society: &Society,
    drawn: &mut Drawn,
    meshes: &mut Assets<Mesh>,
    // None in a world with nothing to draw on.
    mut flags: Option<(
        &mut Waving,
        &mut Assets<FlagMaterial>,
        &FlagSettings,
    )>,
    now: f32,
) {
    for (entity, mesh) in finished.commons {
        if let Ok(mut common) = commands.get_entity(entity) {
            common.try_insert(Mesh3d(meshes.add(mesh)));
        }
    }
    for (job, built) in finished.houses {
        let Some(house) = drawn
            .houses
            .get_mut(&job.agent)
            .filter(|house| house.0 == job.entity)
        else {
            if let Some(old) = job.old {
                commands.entity(old).try_despawn();
            }
            continue;
        };
        house.2 = built.cost;
        let mut new = commands.entity(job.entity);
        new.try_insert(Mesh3d(meshes.add(built.body)));
        match job.way {
            Some(way) => {
                let (comes, goes) = way.parts();
                let replaces = job.old.is_some();
                new.try_insert(
                    Shift::new(comes, replaces).after(job.wait.1),
                );
                if let Some(old) = job.old {
                    commands.entity(old).try_insert(
                        Shift::new(goes, true).after(job.wait.0),
                    );
                }
            }
            None => {
                if let Some(old) = job.old {
                    commands.entity(old).try_despawn();
                }
            }
        }
        if let Some(((cloth, at), (waving, flags, set))) =
            built.banner.zip(flags.as_mut())
        {
            let banner = commands
                .spawn((
                    flag(
                        waving,
                        flags,
                        set,
                        now,
                        meshes.add(cloth),
                        house::BANNER.x,
                    ),
                    Transform::from_translation(at),
                ))
                .id();
            crate::adopt(commands, job.entity, banner);
        }
        let Some((panes, lamp_at)) = built.fittings else {
            continue;
        };
        // The house may have gone with its town while this was
        // built, so its fittings are put under it only if it stands.
        let panes = commands
            .spawn((
                Mesh3d(meshes.add(panes)),
                MeshMaterial3d(kit.windows.clone()),
                NotShadowCaster,
            ))
            .id();
        crate::adopt(commands, job.entity, panes);
        let state = society
            .agent(job.agent)
            .map_or(KState::Rest, |agent| agent.state);
        let lamp = commands
            .spawn((
                Mesh3d(kit.lamp_glass.clone()),
                Transform::from_translation(lamp_at),
                NotShadowCaster,
                children![Spill::bundle(sky)],
            ))
            // Once its light exists, so the light takes its colour.
            .insert(Lamp(state))
            .id();
        crate::adopt(commands, job.entity, lamp);
        drawn.lamps.insert(job.agent, (lamp, state));
    }
    for (pair, level, course, built) in finished.roads {
        let mut spans = built.span.is_some();
        if let Some(old) = drawn.roads.remove(&pair) {
            commands.entity(old.entity).try_despawn();
            spans |= old.span.is_some();
        }
        // The roads whose bridges meet its own are looked over
        // when the society next changes.
        if spans {
            drawn.shape = None;
        }
        let entity = commands
            .spawn((
                Road { pair, level },
                Mesh3d(meshes.add(built.surface)),
                MeshMaterial3d(kit.matte.clone()),
            ))
            .id();
        // Only a bridge stands high enough to throw a shadow.
        if !built.bridge {
            commands.entity(entity).try_insert(NotShadowCaster);
        }
        let groove = commands
            .spawn((
                Mesh3d(meshes.add(built.groove)),
                MeshMaterial3d(kit.groove.clone()),
                NotShadowCaster,
                ChildOf(entity),
            ))
            .id();
        let ends = course.ends;
        drawn.roads.insert(
            pair,
            RoadDrawn {
                entity,
                groove,
                level,
                course,
                cost: built.cost,
                span: built.span,
                route: built.route,
                first: if ends.0 <= ends.1 { pair.0 } else { pair.1 },
            },
        );
    }
}
