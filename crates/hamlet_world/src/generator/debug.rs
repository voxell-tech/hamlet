//! The generated world's part of the debug overlay: road routes,
//! plots, and each robot's stations and the way it is walking.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use super::DetailSettings;
use super::land::Land;
use super::layout::plot;
use super::society::{RoadLevel, Society};
use super::sync::Drawn;
use super::work::Robot;
use crate::WorldCamera;
use crate::debug::{DebugSettings, WorldGizmos};

/// A ring lying on the ground.
fn ring(
    gizmos: &mut Gizmos<WorldGizmos>,
    at: Vec3,
    radius: f32,
    colour: Color,
) {
    gizmos.circle(
        Isometry3d::new(at, Quat::from_rotation_x(-FRAC_PI_2)),
        radius,
        colour,
    );
}

pub(super) fn draw(
    set: Res<DebugSettings>,
    detail: Res<DetailSettings>,
    land: Res<Land>,
    society: Res<Society>,
    drawn: Res<Drawn>,
    camera: Single<&Transform, With<WorldCamera>>,
    robots: Query<(&Robot, &GlobalTransform, &InheritedVisibility)>,
    mut gizmos: Gizmos<WorldGizmos>,
) {
    let eye = camera.translation;
    let lift = Vec3::Y * set.lift;
    if set.routes {
        for road in drawn.roads.values() {
            let line = &road.route.line.pts;
            let near = line
                .first()
                .is_some_and(|p| p.distance(eye) < detail.town_far);
            if !near {
                continue;
            }
            let colour = match road.level {
                RoadLevel::Overgrown => set.plot,
                _ => set.route,
            };
            gizmos.linestrip(
                line.iter()
                    .zip(&road.route.y)
                    .map(|(p, y)| p.with_y(*y) + lift),
                colour,
            );
        }
    }
    if set.plots {
        for town in &society.towns {
            let centre = land.on_ground(land.sites[town.site].at);
            if centre.distance(eye) > detail.town_far {
                continue;
            }
            for k in 0..land.set.town.plots {
                let spot = plot(&land, town.site, k);
                if !spot.usable {
                    continue;
                }
                let taken = society
                    .citizens(town.id)
                    .any(|agent| agent.plot == k);
                let colour =
                    if taken { set.plot } else { set.unused };
                let at = land.on_ground(spot.at) + lift;
                ring(
                    &mut gizmos,
                    at,
                    land.set.town.house_radius,
                    colour,
                );
                gizmos.line(
                    at,
                    land.on_ground(spot.door()) + lift,
                    colour,
                );
            }
        }
    }
    if set.stations {
        for (robot, place, seen) in &robots {
            let at = place.translation();
            if !seen.get() || at.distance(eye) > detail.robots_far {
                continue;
            }
            for (stand, yaw) in robot.stations(&land) {
                let stand = stand + lift;
                ring(&mut gizmos, stand, set.mark, set.station);
                gizmos.arrow(
                    stand,
                    stand
                        + Quat::from_rotation_y(yaw)
                            * Vec3::Z
                            * set.mark
                            * 3.0,
                    set.station,
                );
            }
            if let Some((way, to)) = robot.walk() {
                gizmos.linestrip(
                    way.iter().map(|p| *p + lift),
                    set.path,
                );
                gizmos.line(at + lift, to + lift, set.path);
            }
        }
    }
}
