//! A plugin's camera effects and fog volumes, as the WIT has them,
//! made into Bevy's. A number that is not finite is none; one Bevy
//! would fail on is refused, saying why.

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::color::{Color, LinearRgba};
use bevy::light::{FogVolume, VolumetricFog};
use bevy::pbr::{
    ScreenSpaceAmbientOcclusion,
    ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::render::view::{ColorGrading, ColorGradingGlobal};
use hamlet_engine_render::CameraEffects;

use crate::scene::scene_bindings::hamlet::engine::scene::{
    CameraEffects as Effects, DepthOfFieldMode as Mode,
    FogVolume as Volume, Quality, Rgba,
};

/// What is made, or why it is refused.
pub(crate) type Made<T> = Result<T, String>;

fn finite(values: &[f32]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn color((r, g, b, a): Rgba) -> Option<Color> {
    finite(&[r, g, b, a])
        .then(|| Color::LinearRgba(LinearRgba::new(r, g, b, a)))
}

pub(crate) fn fog_volume(volume: Volume) -> Option<FogVolume> {
    let values = [
        volume.density,
        volume.absorption,
        volume.scattering,
        volume.scattering_asymmetry,
        volume.light_intensity,
    ];
    if !finite(&values) {
        return None;
    }
    Some(FogVolume {
        fog_color: color(volume.color)?,
        density_factor: volume.density.max(0.0),
        absorption: volume.absorption.max(0.0),
        scattering: volume.scattering.max(0.0),
        scattering_asymmetry: volume
            .scattering_asymmetry
            .clamp(-1.0, 1.0),
        light_tint: color(volume.light_tint)?,
        light_intensity: volume.light_intensity.max(0.0),
        ..FogVolume::default()
    })
}

fn preset(quality: Quality) -> SmaaPreset {
    match quality {
        Quality::Low => SmaaPreset::Low,
        Quality::Medium => SmaaPreset::Medium,
        Quality::High => SmaaPreset::High,
        Quality::Ultra => SmaaPreset::Ultra,
    }
}

fn occlusion_level(
    quality: Quality,
) -> ScreenSpaceAmbientOcclusionQualityLevel {
    match quality {
        Quality::Low => ScreenSpaceAmbientOcclusionQualityLevel::Low,
        Quality::Medium => {
            ScreenSpaceAmbientOcclusionQualityLevel::Medium
        }
        Quality::High => {
            ScreenSpaceAmbientOcclusionQualityLevel::High
        }
        Quality::Ultra => {
            ScreenSpaceAmbientOcclusionQualityLevel::Ultra
        }
    }
}

pub(crate) fn effects(
    effects: Effects,
) -> Option<Made<CameraEffects>> {
    let grading = effects.color_grading;
    if !finite(&[
        grading.exposure,
        grading.temperature,
        grading.tint,
        grading.hue,
        grading.post_saturation,
    ]) {
        return None;
    }
    let depth_of_field = match effects.depth_of_field {
        Some(dof) => {
            let values = [
                dof.focal_distance,
                dof.aperture_f_stops,
                dof.max_circle_of_confusion,
            ];
            if !finite(&values) {
                return None;
            }
            if values.iter().any(|&value| value <= 0.0) {
                return Some(Err("a depth of field's distance, \
                                 f-stops and circle are positive"
                    .into()));
            }
            Some(DepthOfField {
                mode: match dof.mode {
                    Mode::Gaussian => DepthOfFieldMode::Gaussian,
                    Mode::Bokeh => DepthOfFieldMode::Bokeh,
                },
                focal_distance: dof.focal_distance,
                aperture_f_stops: dof.aperture_f_stops,
                max_circle_of_confusion_diameter: dof
                    .max_circle_of_confusion,
                ..DepthOfField::default()
            })
        }
        None => None,
    };
    let occlusion = match effects.ambient_occlusion {
        Some(occlusion) => {
            if !finite(&[occlusion.thickness]) {
                return None;
            }
            Some(ScreenSpaceAmbientOcclusion {
                quality_level: occlusion_level(occlusion.quality),
                constant_object_thickness: occlusion
                    .thickness
                    .max(0.0),
            })
        }
        None => None,
    };
    let volumetric_fog = match effects.volumetric_fog {
        Some(fog) => {
            if !finite(&[fog.ambient_intensity, fog.jitter]) {
                return None;
            }
            if fog.step_count == 0 {
                return Some(Err("volumetric fog takes a step or \
                                 more"
                    .into()));
            }
            Some(VolumetricFog {
                ambient_color: color(fog.ambient_color)?,
                ambient_intensity: fog.ambient_intensity.max(0.0),
                jitter: fog.jitter.max(0.0),
                step_count: fog.step_count,
            })
        }
        None => None,
    };
    Some(Ok(CameraEffects {
        depth_of_field,
        color_grading: ColorGrading {
            global: ColorGradingGlobal {
                exposure: grading.exposure,
                temperature: grading.temperature,
                tint: grading.tint,
                hue: grading.hue,
                post_saturation: grading.post_saturation,
                ..ColorGradingGlobal::default()
            },
            ..ColorGrading::default()
        },
        occlusion,
        smaa: effects.smaa.map(|quality| Smaa {
            preset: preset(quality),
        }),
        atmosphere: effects.atmosphere,
        volumetric_fog,
    }))
}
