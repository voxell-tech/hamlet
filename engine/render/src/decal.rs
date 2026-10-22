//! Decals of scenes: Bevy's forward decals, a standard material laid
//! on what lies under and over a square, and decals of a plugin's
//! shader, drawn through a box over the place. A camera that sees
//! either has a depth prepass.

use bevy::app::App;
use bevy::asset::{Asset, Assets, Handle};
use bevy::ecs::resource::Resource;
use bevy::image::Image;
use bevy::material::AlphaMode;
use bevy::math::Vec4;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::decal::{ForwardDecalMaterial, ForwardDecalPlugin};
use bevy::pbr::{
    Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin,
    StandardMaterial,
};
use bevy::reflect::TypePath;
use bevy::render::RenderApp;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, Face, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::Shader;

use crate::shader::{ADD, PARAMS};

/// The material of a decal.
pub(crate) type DecalMaterial =
    ForwardDecalMaterial<StandardMaterial>;

pub(crate) fn plugin(app: &mut App) {
    // Without Bevy's decals, as in the harness, a decal is kept all
    // the same and drawn nowhere.
    if app.is_plugin_added::<ForwardDecalPlugin>() {
        app.init_resource::<DrawsDecals>();
    } else {
        app.init_resource::<Assets<DecalMaterial>>();
    }
    if app.get_sub_app(RenderApp).is_some() {
        app.add_plugins(MaterialPlugin::<ShadedDecal>::default());
    } else {
        app.init_resource::<Assets<ShadedDecal>>();
    }
}

/// Present when Bevy draws decals.
#[derive(Resource, Default, Debug)]
pub(crate) struct DrawsDecals;

/// A decal of a plugin's shader, drawn on a box one unit each way.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(ShadedDecalKey)]
pub(crate) struct ShadedDecal {
    #[uniform(0)]
    pub params: [Vec4; PARAMS],
    #[texture(1)]
    #[sampler(2)]
    pub picture: Option<Handle<Image>>,
    #[storage(3, read_only)]
    pub data: Handle<ShaderBuffer>,
    pub shader: Handle<Shader>,
    /// Always a blended one.
    pub alpha: AlphaMode,
}

/// What a pipeline of a [`ShadedDecal`] is specialized by.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct ShadedDecalKey {
    shader: Handle<Shader>,
    add: bool,
}

impl From<&ShadedDecal> for ShadedDecalKey {
    fn from(decal: &ShadedDecal) -> Self {
        Self {
            shader: decal.shader.clone(),
            add: decal.alpha == AlphaMode::Add,
        }
    }
}

impl Material for ShadedDecal {
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let key = key.bind_group_data;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = key.shader;
            if key.add {
                fragment.shader_defs.push(ADD.into());
            }
        }
        // The box is drawn wherever it stands, through what lies over
        // it too: the shader finds what each pixel falls on. Only its
        // far sides are drawn, which cover it on the screen once,
        // from outside it and from within.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Always);
        }
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}
