//! A material drawn by a plugin's checked WGSL: Bevy's material
//! pipeline with the plugin's shader in place of its own, and the
//! engine's bindings filled from the plugin's params, texture and
//! buffer.

use bevy::app::App;
use bevy::asset::{Asset, Assets, Handle};
use bevy::image::Image;
use bevy::material::AlphaMode;
use bevy::math::Vec4;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin,
};
use bevy::reflect::TypePath;
use bevy::render::RenderApp;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::Shader;

use crate::shader::{LIT, PARAMS};

pub(crate) fn plugin(app: &mut App) {
    if app.get_sub_app(RenderApp).is_some() {
        app.add_plugins(MaterialPlugin::<ShadedMaterial>::default());
    } else {
        app.init_resource::<Assets<ShadedMaterial>>();
    }
}

/// A material of a plugin's shader.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(ShadedKey)]
pub struct ShadedMaterial {
    #[uniform(0)]
    pub params: [Vec4; PARAMS],
    #[texture(1)]
    #[sampler(2)]
    pub picture: Option<Handle<Image>>,
    #[storage(3, read_only)]
    pub data: Handle<ShaderBuffer>,
    pub shader: Handle<Shader>,
    pub alpha: AlphaMode,
    pub double_sided: bool,
    pub lit: bool,
}

/// What a pipeline of a [`ShadedMaterial`] is specialized by.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ShadedKey {
    shader: Handle<Shader>,
    double_sided: bool,
    lit: bool,
}

impl From<&ShadedMaterial> for ShadedKey {
    fn from(material: &ShadedMaterial) -> Self {
        Self {
            shader: material.shader.clone(),
            double_sided: material.double_sided,
            lit: material.lit,
        }
    }
}

impl Material for ShadedMaterial {
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }

    // A plugin's vertices may move, which the engine's prepass and
    // shadows would not see.
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
        descriptor.vertex.shader = key.shader.clone();
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = key.shader;
            if key.lit {
                fragment.shader_defs.push(LIT.into());
            }
        }
        if key.double_sided {
            descriptor.primitive.cull_mode = None;
        }
        Ok(())
    }
}

/// The params of a plugin's shader, as many as `floats` gives, zero
/// past them.
pub(crate) fn params(floats: &[f32]) -> [Vec4; PARAMS] {
    let mut params = [Vec4::ZERO; PARAMS];
    for (param, four) in params.iter_mut().zip(floats.chunks(4)) {
        let mut padded = [0.0; 4];
        padded[..four.len()].copy_from_slice(four);
        *param = Vec4::from_array(padded);
    }
    params
}
