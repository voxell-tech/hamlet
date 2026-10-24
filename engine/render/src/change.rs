//! A change to a scene, as its owner made it, by the number the
//! owner's host gave each thing in it.

use bevy::anti_alias::smaa::Smaa;
use bevy::camera::{ClearColorConfig, Exposure, Projection};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{
    AmbientLight, CascadeShadowConfig, DirectionalLight, FogVolume,
    PointLight, ShadowFilteringMethod, SpotLight, VolumetricFog,
};
use bevy::material::AlphaMode;
use bevy::mesh::Mesh;
use bevy::pbr::{
    DistanceFog, ScreenSpaceAmbientOcclusion, StandardMaterial,
};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::dof::DepthOfField;
use bevy::render::view::{ColorGrading, Msaa};
use bevy::transform::components::Transform;

use crate::balloon::Balloon;
use crate::label::{PointerLabel, WorldLabel};
use crate::shader::CheckedShader;

#[derive(Clone)]
pub enum SceneChange {
    /// An entity, under `parent` or at the scene's root.
    Entity {
        id: u32,
        key: u32,
        parent: Option<u32>,
    },
    Transform {
        id: u32,
        transform: Transform,
    },
    Visible {
        id: u32,
        visible: bool,
    },
    Parent {
        id: u32,
        parent: Option<u32>,
    },
    Draw {
        id: u32,
        mesh: u32,
        material: u32,
    },
    /// A decal of the material `material` laid at the entity `id`.
    DrawDecal {
        id: u32,
        material: u32,
    },
    Undraw(u32),
    Light {
        id: u32,
        light: Option<SceneLight>,
    },
    Shadows {
        id: u32,
        cast: bool,
        receive: bool,
    },
    /// Whether the directional light of `id` lights fog volumes.
    Volumetric {
        id: u32,
        on: bool,
    },
    FogVolume {
        id: u32,
        volume: Option<FogVolume>,
    },
    /// The side of a directional light's shadow map the owner asks
    /// for.
    ShadowMap(usize),
    Label {
        id: u32,
        label: Option<WorldLabel>,
    },
    Balloon {
        id: u32,
        balloon: Option<Balloon>,
    },
    /// The words beside the pointer over the owner's views.
    PointerLabel(Option<PointerLabel>),
    Watch {
        id: u32,
        hover: bool,
        click: bool,
    },
    /// A mesh made, or given in place of what it was.
    Mesh {
        id: u32,
        mesh: Mesh,
    },
    /// A material made, or set.
    Material {
        id: u32,
        material: StandardMaterial,
    },
    /// A decal's material made with `texture`, or set keeping the
    /// texture it was made with.
    Decal {
        id: u32,
        material: StandardMaterial,
        depth_fade: f32,
        texture: Option<u32>,
    },
    Instances {
        id: u32,
        key: u32,
        parent: Option<u32>,
        mesh: u32,
        material: u32,
    },
    /// The copies of instances, one for each transform.
    Copies {
        id: u32,
        transforms: Vec<Transform>,
    },
    /// A camera at the entity `at`, drawing behind the dock when
    /// `background`, else into an image a viewport shows.
    Camera {
        id: u32,
        at: u32,
        setup: CameraSetup,
        background: bool,
    },
    Setup {
        id: u32,
        setup: CameraSetup,
    },
    Effects {
        id: u32,
        effects: CameraEffects,
    },
    /// A shader that passed its check.
    Shader {
        id: u32,
        shader: CheckedShader,
    },
    /// A storage buffer of `size` bytes, zeroed.
    Buffer {
        id: u32,
        size: u32,
    },
    Write {
        id: u32,
        offset: u32,
        bytes: Vec<u8>,
    },
    /// An image of sRGB bytes, made or given in place of what it was.
    Texture {
        id: u32,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    /// A material of the shader `shader`.
    Shaded {
        id: u32,
        shader: u32,
        alpha: AlphaMode,
        double_sided: bool,
        lit: bool,
        params: Vec<f32>,
        texture: Option<u32>,
        data: Option<u32>,
    },
    /// A decal's material of the shader `shader`.
    ShadedDecal {
        id: u32,
        shader: u32,
        alpha: AlphaMode,
        params: Vec<f32>,
        texture: Option<u32>,
        data: Option<u32>,
    },
    /// The params of a shaded material or decal, or of compute.
    Params {
        id: u32,
        params: Vec<f32>,
    },
    Compute {
        id: u32,
        shader: u32,
        data: u32,
        extra: Option<u32>,
    },
    Dispatch {
        id: u32,
        groups: [u32; 3],
        every_frame: bool,
    },
    /// The pass of the camera `camera`, or none.
    Pass {
        camera: u32,
        shader: Option<u32>,
        params: Vec<f32>,
    },
    /// Takes away whatever has this number, with what is under it.
    Drop(u32),
}

#[derive(Clone, Debug)]
pub enum SceneLight {
    /// With how its shadows are split by distance, none leaving the
    /// split it has.
    Directional(DirectionalLight, Option<CascadeShadowConfig>),
    Point(PointLight),
    Spot(SpotLight),
}

/// How a camera draws.
#[derive(Clone)]
pub struct CameraSetup {
    pub projection: Projection,
    pub clear: ClearColorConfig,
    pub hdr: bool,
    pub tonemapping: Tonemapping,
    pub exposure: Exposure,
    pub fog: Option<DistanceFog>,
    pub bloom: Option<Bloom>,
    pub ambient: AmbientLight,
    pub msaa: Msaa,
    pub shadow_filtering: ShadowFilteringMethod,
}

/// What a camera draws over its image, each off when none.
#[derive(Clone, Default)]
pub struct CameraEffects {
    pub depth_of_field: Option<DepthOfField>,
    pub color_grading: ColorGrading,
    pub occlusion: Option<ScreenSpaceAmbientOcclusion>,
    pub smaa: Option<Smaa>,
    pub atmosphere: bool,
    pub volumetric_fog: Option<VolumetricFog>,
}
