//! What a scene's camera draws over its image, as Bevy's own effects
//! do it, and what of them is shared by every scene: the earth's sky,
//! the fog volumes of one owner at a time and the size of the
//! directional lights' shadow maps.

use std::collections::HashMap;

use bevy::anti_alias::smaa::Smaa;
use bevy::app::{App, PostUpdate};
use bevy::asset::Assets;
use bevy::camera::Hdr;
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ecs::world::EntityWorldMut;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{
    Atmosphere, DirectionalLightShadowMap, FogVolume, VolumetricFog,
};
use bevy::pbr::{AtmosphereSettings, ScreenSpaceAmbientOcclusion};
use bevy::post_process::dof::DepthOfField;
use bevy::render::view::Msaa;

use crate::change::CameraEffects;
use crate::scene::{Scene, SceneCamera, ScenePart};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<FogHolder>()
        .init_resource::<ShadowMaps>()
        .add_systems(
            PostUpdate,
            (raise_sky, release_fog, size_shadow_maps),
        );
}

/// The owner whose fog volumes and volumetric fog are drawn. Bevy
/// shows every fog volume to every camera with volumetric fog, so
/// one owner's would show in another's scene.
#[derive(Resource, Default, Debug)]
pub(crate) struct FogHolder(Option<Entity>);

impl FogHolder {
    /// Whether `owner` holds the fog, taking it if no one does.
    pub(crate) fn take(&mut self, owner: Entity) -> bool {
        let free = self.0.is_none_or(|held| held == owner);
        if free {
            self.0 = Some(owner);
        }
        free
    }
}

/// The side of the shadow map each scene, by its root, asks for.
#[derive(Resource, Default, Debug)]
pub(crate) struct ShadowMaps(pub(crate) HashMap<Entity, usize>);

/// On a scene's camera: what its owner asked of its samples and
/// range, and what its effects need of them.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct CameraLook {
    pub(crate) msaa: Msaa,
    pub(crate) hdr: bool,
    /// Ambient occlusion, which Bevy draws without MSAA.
    pub(crate) single_sample: bool,
    /// The sky, which draws in high range.
    pub(crate) sky: bool,
}

impl Default for CameraLook {
    fn default() -> Self {
        Self {
            msaa: Msaa::Sample4,
            hdr: false,
            single_sample: false,
            sky: false,
        }
    }
}

impl CameraLook {
    /// Gives `camera` its samples and range.
    pub(crate) fn show(self, camera: &mut EntityWorldMut) {
        let msaa = if self.single_sample {
            Msaa::Off
        } else {
            self.msaa
        };
        camera.insert((self, msaa));
        if self.hdr || self.sky {
            camera.insert(Hdr);
        } else {
            camera.remove::<Hdr>();
        }
    }
}

impl CameraEffects {
    /// Puts the effects on `camera` and takes off those that are
    /// off. `decals` keeps the depth prepass a decal reads; `fog` is
    /// whether its owner holds the fog.
    pub(crate) fn put_on(
        self,
        camera: &mut EntityWorldMut,
        decals: bool,
        fog: bool,
    ) {
        let look = CameraLook {
            single_sample: self.occlusion.is_some(),
            sky: self.atmosphere,
            ..camera.get::<CameraLook>().copied().unwrap_or_default()
        };
        look.show(camera);
        camera.insert(self.color_grading);
        match self.depth_of_field {
            Some(dof) => camera.insert(dof),
            None => camera.remove::<DepthOfField>(),
        };
        if let Some(occlusion) = self.occlusion {
            camera.insert(occlusion);
        } else {
            camera.remove::<(ScreenSpaceAmbientOcclusion, NormalPrepass)>();
            if !decals {
                camera.remove::<DepthPrepass>();
            }
        }
        match self.smaa {
            Some(smaa) => camera.insert(smaa),
            None => camera.remove::<Smaa>(),
        };
        if self.atmosphere {
            camera.insert(AtmosphereSettings::default());
        } else {
            camera.remove::<AtmosphereSettings>();
        }
        match self.volumetric_fog.filter(|_| fog) {
            Some(volumetric) => camera.insert(volumetric),
            None => camera.remove::<VolumetricFog>(),
        };
    }
}

/// Makes the earth's sky once a camera draws it. It is never taken
/// away: the renderer fails if its medium goes while a frame still
/// holds it.
fn raise_sky(
    mut commands: Commands,
    cameras: Query<(), (With<AtmosphereSettings>, With<SceneCamera>)>,
    skies: Query<(), With<Atmosphere>>,
    media: Option<ResMut<Assets<ScatteringMedium>>>,
) {
    if cameras.is_empty() || !skies.is_empty() {
        return;
    }
    let Some(mut media) = media else {
        return;
    };
    let medium = media.add(ScatteringMedium::earth(256, 256));
    commands.spawn(Atmosphere::earth(medium));
}

/// Lets go of the fog once its owner has no fog volume and no camera
/// with volumetric fog.
fn release_fog(
    cameras: Query<&SceneCamera, With<VolumetricFog>>,
    volumes: Query<&ScenePart, With<FogVolume>>,
    mut holder: ResMut<FogHolder>,
) {
    let Some(owner) = holder.0 else {
        return;
    };
    let held = cameras.iter().any(|camera| camera.owner == owner)
        || volumes.iter().any(|part| part.owner == owner);
    if !held {
        holder.0 = None;
    }
}

/// Draws the directional lights' shadow maps at the largest side a
/// scene asks for, Bevy's own when none does.
fn size_shadow_maps(
    scenes: Query<(), With<Scene>>,
    mut maps: ResMut<ShadowMaps>,
    shadow_map: Option<ResMut<DirectionalLightShadowMap>>,
) {
    maps.0.retain(|&root, _| scenes.contains(root));
    let Some(mut shadow_map) = shadow_map else {
        return;
    };
    let size = maps
        .0
        .values()
        .copied()
        .max()
        .unwrap_or(DirectionalLightShadowMap::default().size);
    if shadow_map.size != size {
        shadow_map.size = size;
    }
}
