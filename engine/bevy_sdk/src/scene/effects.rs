//! Bevy's camera effects, volumetric lights and fog volumes as
//! components of the plugin's entities, and the size of the shadow
//! maps as a resource, each sent as it changes.
//!
//! ```ignore
//! commands.spawn((
//!     SceneCamera::viewport(CameraSettings::default()),
//!     DepthOfField { focal_distance: 40.0, ..default() },
//!     ColorGrading { global: ColorGradingGlobal { post_saturation: 1.1, ..default() } },
//!     ScreenSpaceAmbientOcclusion::default(),
//!     Smaa::default(),
//! ));
//! commands.spawn((SceneLight(Light::Directional(sun)), VolumetricLight));
//! ```

use bevy_app::{App, PostUpdate};
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryItem;

use crate::bindings::hamlet::engine::scene::{
    self as wit, CameraEffects, Rgba,
};
pub use crate::bindings::hamlet::engine::scene::{
    DepthOfFieldMode, ScreenSpaceAmbientOcclusionQualityLevel,
    SmaaPreset,
};
use crate::scene::{HostCamera, HostEntity, SceneSync, make_cameras};

const WHITE: Rgba = (1.0, 1.0, 1.0, 1.0);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<DirectionalLightShadowMap>()
        .add_systems(
            PostUpdate,
            (
                put_effects,
                put_volumetric,
                put_fog_volumes,
                put_shadow_map,
            )
                .in_set(SceneSync)
                .after(make_cameras),
        );
}

/// Bevy's depth of field, on a
/// [`SceneCamera`](crate::scene::SceneCamera)'s entity.
#[derive(Component, Clone, Copy, Debug)]
pub struct DepthOfField {
    pub mode: DepthOfFieldMode,
    pub focal_distance: f32,
    /// In metres.
    pub sensor_height: f32,
    pub aperture_f_stops: f32,
    /// In pixels.
    pub max_circle_of_confusion_diameter: f32,
    pub max_depth: f32,
}

impl Default for DepthOfField {
    fn default() -> Self {
        Self {
            mode: DepthOfFieldMode::Gaussian,
            focal_distance: 10.0,
            sensor_height: 0.01866,
            aperture_f_stops: 1.0,
            max_circle_of_confusion_diameter: 64.0,
            max_depth: f32::INFINITY,
        }
    }
}

/// Bevy's colour grading, on a camera's entity.
#[derive(Component, Clone, Copy, Default, Debug)]
pub struct ColorGrading {
    pub global: ColorGradingGlobal,
}

/// The grading of the whole image, as Bevy's.
#[derive(Clone, Copy, Debug)]
pub struct ColorGradingGlobal {
    /// In EV, on top of the camera's exposure.
    pub exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    /// In radians.
    pub hue: f32,
    pub post_saturation: f32,
}

impl Default for ColorGradingGlobal {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            hue: 0.0,
            post_saturation: 1.0,
        }
    }
}

/// Bevy's screen-space ambient occlusion, on a camera's entity. The
/// camera draws without MSAA.
#[derive(Component, Clone, Copy, Debug)]
pub struct ScreenSpaceAmbientOcclusion {
    pub quality_level: ScreenSpaceAmbientOcclusionQualityLevel,
    pub constant_object_thickness: f32,
}

impl Default for ScreenSpaceAmbientOcclusion {
    fn default() -> Self {
        Self {
            quality_level:
                ScreenSpaceAmbientOcclusionQualityLevel::High,
            constant_object_thickness: 0.25,
        }
    }
}

/// Bevy's SMAA, on a camera's entity.
#[derive(Component, Clone, Copy, Debug)]
pub struct Smaa {
    pub preset: SmaaPreset,
}

impl Default for Smaa {
    fn default() -> Self {
        Self {
            preset: SmaaPreset::High,
        }
    }
}

/// The engine's scattering sky of an earth-like world, on a camera's
/// entity, drawn in high range.
#[derive(Component, Clone, Copy, Default, Debug)]
pub struct AtmosphereSettings;

/// Bevy's volumetric fog, on a camera's entity: the scene's
/// [`FogVolume`]s lit by its [`VolumetricLight`]s. One plugin's is
/// drawn at a time.
#[derive(Component, Clone, Copy, Debug)]
pub struct VolumetricFog {
    pub ambient_color: Rgba,
    pub ambient_intensity: f32,
    pub jitter: f32,
    pub step_count: u32,
}

impl Default for VolumetricFog {
    fn default() -> Self {
        Self {
            ambient_color: WHITE,
            ambient_intensity: 0.1,
            jitter: 0.0,
            step_count: 64,
        }
    }
}

/// On a directional light with shadows: it lights fog volumes.
#[derive(Component, Clone, Copy, Default, Debug)]
pub struct VolumetricLight;

/// A box of haze one unit each way about the entity, as Bevy's.
#[derive(Component, Clone, Copy, Debug)]
pub struct FogVolume {
    pub fog_color: Rgba,
    pub density_factor: f32,
    pub absorption: f32,
    pub scattering: f32,
    pub scattering_asymmetry: f32,
    pub light_tint: Rgba,
    pub light_intensity: f32,
}

impl Default for FogVolume {
    fn default() -> Self {
        Self {
            fog_color: WHITE,
            density_factor: 0.1,
            absorption: 0.3,
            scattering: 0.3,
            scattering_asymmetry: 0.5,
            light_tint: WHITE,
            light_intensity: 1.0,
        }
    }
}

/// The side of each cascade of a directional light's shadow map, in
/// texels: a power of two. The engine draws the largest any scene
/// asks for.
#[derive(Resource, Clone, Copy, Debug)]
pub struct DirectionalLightShadowMap {
    pub size: usize,
}

impl Default for DirectionalLightShadowMap {
    fn default() -> Self {
        Self { size: 2048 }
    }
}

type Effects<'a> = (
    &'a HostCamera,
    Option<&'a DepthOfField>,
    Option<&'a ColorGrading>,
    Option<&'a ScreenSpaceAmbientOcclusion>,
    Option<&'a Smaa>,
    Has<AtmosphereSettings>,
    Option<&'a VolumetricFog>,
);

fn effects(
    (_, dof, grading, occlusion, smaa, atmosphere, fog): QueryItem<
        Effects,
    >,
) -> CameraEffects {
    let global = grading.copied().unwrap_or_default().global;
    CameraEffects {
        depth_of_field: dof.map(|dof| wit::DepthOfField {
            mode: dof.mode,
            focal_distance: dof.focal_distance,
            sensor_height: dof.sensor_height,
            aperture_f_stops: dof.aperture_f_stops,
            max_circle_of_confusion_diameter: dof
                .max_circle_of_confusion_diameter,
            max_depth: dof.max_depth,
        }),
        color_grading: wit::ColorGrading {
            global: wit::ColorGradingGlobal {
                exposure: global.exposure,
                temperature: global.temperature,
                tint: global.tint,
                hue: global.hue,
                post_saturation: global.post_saturation,
            },
        },
        screen_space_ambient_occlusion: occlusion.map(|occlusion| {
            wit::ScreenSpaceAmbientOcclusion {
                quality_level: occlusion.quality_level,
                constant_object_thickness: occlusion
                    .constant_object_thickness,
            }
        }),
        smaa: smaa.map(|smaa| wit::Smaa {
            preset: smaa.preset,
        }),
        atmosphere,
        volumetric_fog: fog.map(|fog| wit::VolumetricFog {
            ambient_color: fog.ambient_color,
            ambient_intensity: fog.ambient_intensity,
            step_count: fog.step_count,
            jitter: fog.jitter,
        }),
    }
}

fn put_effects(
    changed: Query<
        Entity,
        (
            With<HostCamera>,
            Or<(
                Changed<HostCamera>,
                Changed<DepthOfField>,
                Changed<ColorGrading>,
                Changed<ScreenSpaceAmbientOcclusion>,
                Changed<Smaa>,
                Changed<AtmosphereSettings>,
                Changed<VolumetricFog>,
            )>,
        ),
    >,
    cameras: Query<Effects>,
    mut dof: RemovedComponents<DepthOfField>,
    mut grading: RemovedComponents<ColorGrading>,
    mut occlusion: RemovedComponents<ScreenSpaceAmbientOcclusion>,
    mut smaa: RemovedComponents<Smaa>,
    mut atmosphere: RemovedComponents<AtmosphereSettings>,
    mut fog: RemovedComponents<VolumetricFog>,
) {
    let mut put = changed.iter().collect::<Vec<_>>();
    put.extend(dof.read());
    put.extend(grading.read());
    put.extend(occlusion.read());
    put.extend(smaa.read());
    put.extend(atmosphere.read());
    put.extend(fog.read());
    put.sort_unstable();
    put.dedup();
    for entity in put {
        let Ok(camera) = cameras.get(entity) else {
            continue;
        };
        let host = camera.0;
        if let Err(why) = host.camera.set_effects(effects(camera)) {
            crate::error(&format!("effects are not set: {why}"));
        }
    }
}

fn put_volumetric(
    lit: Query<
        &HostEntity,
        (
            With<VolumetricLight>,
            Or<(Added<VolumetricLight>, Added<HostEntity>)>,
        ),
    >,
    hosts: Query<&HostEntity>,
    mut unlit: RemovedComponents<VolumetricLight>,
) {
    for host in &lit {
        host.entity.set_volumetric(true);
    }
    for entity in unlit.read() {
        if let Ok(host) = hosts.get(entity) {
            host.entity.set_volumetric(false);
        }
    }
}

fn put_fog_volumes(
    volumes: Query<
        (&HostEntity, &FogVolume),
        Or<(Changed<FogVolume>, Added<HostEntity>)>,
    >,
    hosts: Query<&HostEntity>,
    mut cleared: RemovedComponents<FogVolume>,
) {
    for (host, volume) in &volumes {
        let volume = wit::FogVolume {
            fog_color: volume.fog_color,
            density_factor: volume.density_factor,
            absorption: volume.absorption,
            scattering: volume.scattering,
            scattering_asymmetry: volume.scattering_asymmetry,
            light_tint: volume.light_tint,
            light_intensity: volume.light_intensity,
        };
        if let Err(why) = host.entity.set_fog_volume(Some(volume)) {
            crate::error(&format!("a fog volume is not made: {why}"));
        }
    }
    for entity in cleared.read() {
        if let Ok(host) = hosts.get(entity) {
            let _ = host.entity.set_fog_volume(None);
        }
    }
}

fn put_shadow_map(shadow_map: Res<DirectionalLightShadowMap>) {
    if !shadow_map.is_changed() {
        return;
    }
    let size = u32::try_from(shadow_map.size).unwrap_or(u32::MAX);
    if let Err(why) = wit::shadow_map(size) {
        crate::error(&format!("the shadow map is not sized: {why}"));
    }
}
