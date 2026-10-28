//! The camera's view, and the detail that leaves each town.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::prelude::*;
use bevy::window::RequestRedraw;

use super::ground::{DetailSettings, Sight};
use super::land::Land;
use super::loading::Loading;
use super::shift::Folded;
use super::society::Society;
use super::sync::Drawn;
use super::work::Robot;
use crate::sky::{CameraSettings, Sun};
use crate::{Clock, Stage, WorldCamera};

/// The camera's view.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct GeneratorView {
    pub focus: Vec2,
    pub dist: f32,
    pub yaw: f32,
    /// Degrees below the horizon.
    pub pitch: f32,
}

impl Default for GeneratorView {
    fn default() -> Self {
        Self {
            focus: Vec2::ZERO,
            dist: 70.0,
            yaw: 0.0,
            pitch: 42.0,
        }
    }
}

impl GeneratorView {
    /// The height the camera looks at: the lie of the land about
    /// its focus, taken wide so that the camera crosses a mountain
    /// without following every rock of it.
    fn level(&self, land: &Land) -> f32 {
        const SPOKES: usize = 8;
        let reach = self.dist * 0.4;
        let mut sum = land.height(self.focus);
        for ring in [0.5, 1.0] {
            for spoke in 0..SPOKES {
                let turn = (spoke as f32 + ring) / SPOKES as f32
                    * std::f32::consts::TAU;
                sum += land.height(
                    self.focus
                        + Vec2::from_angle(turn) * reach * ring,
                );
            }
        }
        sum / (1 + 2 * SPOKES) as f32
    }

    /// The camera's place for this view, over the point it looks at.
    fn eye(&self, land: &Land) -> (Vec3, Vec3) {
        let focus = self.focus.extend(self.level(land) + 0.5).xzy();
        let pitch = self.pitch.to_radians();
        let back = Vec3::new(
            self.yaw.sin() * pitch.cos(),
            pitch.sin(),
            self.yaw.cos() * pitch.cos(),
        );
        (focus + self.dist * back, focus)
    }

    /// The sight of the ground a camera aimed this way has.
    pub(super) fn sight(
        &self,
        land: &Land,
        settings: &CameraSettings,
        detail: &DetailSettings,
    ) -> Sight {
        let (eye, _) = self.eye(land);
        let whole =
            self.dist * settings.fog_end.0 + settings.fog_end.1;
        Sight {
            eye,
            centre: self.focus,
            reach: whole.min(settings.far) * detail.ground_reach,
        }
    }
}

/// The place the camera is being taken to, while that is not yet the
/// place it looks at. The ground is drawn there ahead of it.
#[derive(Resource, Clone, Copy, PartialEq, Debug, Default)]
pub struct Ahead(pub Option<GeneratorView>);

/// Swaps each town between its houses and its plain far mesh by the
/// camera's distance. A town keeps its far mesh until every house
/// it has is built.
pub(super) fn town_detail(
    land: Res<Land>,
    detail: Res<DetailSettings>,
    drawn: Res<Drawn>,
    loading: Res<Loading>,
    society: Res<Society>,
    camera: Single<&Transform, With<WorldCamera>>,
    mut visibility: Query<&mut Visibility>,
) {
    let mut show = |entity: Entity, on: bool| {
        let want = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Ok(mut v) = visibility.get_mut(entity)
            && *v != want
        {
            *v = want;
        }
    };
    let builds = &loading.builds;
    for (id, town) in &drawn.towns {
        let c = land.sites[town.site].at;
        let d = camera.translation.distance(Vec3::new(
            c.x,
            land.set.land.site_y,
            c.y,
        ));
        let built = builds.is_empty()
            || !(builds.is_common_bare(town.common)
                || society.agents.iter().any(|agent| {
                    agent.town == *id
                        && builds.is_house_bare(agent.id)
                }));
        let near = d < detail.town_far && built;
        show(town.near, near);
        show(town.far, !near);
    }
}

/// Hides each robot too far from the camera to see, wherever its
/// house is, and each that has yet to unfold.
pub(super) fn robot_detail(
    detail: Res<DetailSettings>,
    camera: Single<&Transform, With<WorldCamera>>,
    mut robots: Query<
        (&Transform, &mut Visibility, Has<Folded>),
        (With<Robot>, Without<WorldCamera>),
    >,
) {
    for (at, mut seen, folded) in &mut robots {
        let near = camera.translation.distance(at.translation)
            < detail.robots_far;
        seen.set_if_neq(if near && !folded {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// The distances from the camera the sun's shadows start at, split at
/// and end at, for a camera `dist` from what it looks at, `rise`
/// above it and looking down by the sine `down`. They start at the
/// camera, for the volumetric fog takes no light where they do not
/// reach, and stretch to cover the ground in view: to the top of the
/// view, or to the fog's end where the top is over the horizon. The
/// split follows the zoom, so shadows stay sharp up close.
fn shadowed(
    settings: &CameraSettings,
    dist: f32,
    rise: f32,
    down: f32,
    fog_end: f32,
) -> (f32, f32, f32) {
    let (split, end) = settings.shadow_range;
    let pitch = down.clamp(-1.0, 1.0).asin();
    let half = settings.fov.to_radians() * 0.5;
    let farthest = if pitch - half > 0.01 {
        (rise.max(settings.near) / (pitch - half).sin()).min(fog_end)
    } else {
        fog_end
    };
    let least = settings.near;
    let most = (dist * end).max(farthest).max(least * 2.0);
    let split = (dist * split).clamp(least * 1.5, most * 0.75);
    (least, split, most)
}

/// Points the camera where [`GeneratorView`] looks, with fog, shadows
/// and blur fitted to its distance.
pub(super) fn aim(
    mut commands: Commands,
    stage: Res<Stage>,
    settings: Res<CameraSettings>,
    view: Res<GeneratorView>,
    land: Res<Land>,
    time: Res<Time>,
    clock: Res<Clock>,
    mut level: Local<Option<f32>>,
    mut redraw: MessageWriter<RequestRedraw>,
    camera: Single<
        (Entity, &mut Transform, &mut DistanceFog),
        With<WorldCamera>,
    >,
    sun: Single<Entity, With<Sun>>,
) {
    let dist = view.dist;
    let (eye, focus) = view.eye(&land);
    // The height it looks at is eased to, so that it rides over
    // rough ground. A still, a first look and a leap go there at
    // once.
    let from = level.filter(|was| {
        !clock.frozen && (was - focus.y).abs() < dist * 0.5
    });
    let eased = match from {
        Some(was) => {
            let k = 1.0 - (-settings.level * time.delta_secs()).exp();
            was + (focus.y - was) * k
        }
        None => focus.y,
    };
    let settled = (eased - focus.y).abs() < 0.002;
    let eased = if settled { focus.y } else { eased };
    let moved = *level != Some(eased);
    *level = Some(eased);
    if !stage.is_changed() && !view.is_changed() && !moved {
        return;
    }
    if !settled {
        redraw.write(RequestRedraw);
    }
    let (entity, mut at, mut fog) = camera.into_inner();
    let lift = Vec3::Y * (eased - focus.y);
    let (eye, focus) = (eye + lift, focus + lift);
    *at = Transform::from_translation(eye).looking_at(focus, Vec3::Y);

    let fog_end = dist * settings.fog_end.0 + settings.fog_end.1;
    fog.falloff = FogFalloff::Linear {
        start: dist * settings.fog_start,
        end: fog_end,
    };
    let (least, split, most) = shadowed(
        &settings,
        dist,
        eye.y - focus.y,
        -at.forward().y,
        fog_end,
    );
    commands.entity(*sun).try_insert(
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            minimum_distance: least,
            first_cascade_far_bound: split,
            maximum_distance: most,
            ..default()
        }
        .build(),
    );
    if stage.dof && dist < settings.blur_within {
        commands.entity(entity).try_insert(DepthOfField {
            mode: DepthOfFieldMode::Gaussian,
            focal_distance: dist,
            aperture_f_stops: (settings.blur_stop / dist)
                .max(settings.blur_stop_min),
            max_circle_of_confusion_diameter: settings.blur_circle,
            ..default()
        });
    } else {
        commands.entity(entity).try_remove::<DepthOfField>();
    }
}
