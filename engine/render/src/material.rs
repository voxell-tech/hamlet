//! A material drawn by a plugin's checked WGSL: Bevy's material
//! pipeline with the plugin's shader in place of its own, and the
//! engine's bindings filled from the plugin's params, texture and
//! buffer.

use bevy::app::App;
use bevy::asset::{Asset, Assets, Handle};
use bevy::image::Image;
use bevy::material::AlphaMode;
use bevy::math::Vec4;
use bevy::mesh::{Mesh, MeshVertexBufferLayoutRef};
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
use bevy::shader::{Shader, ShaderDefVal, ShaderRef};

use crate::shader::{LIT, MASK, PARAMS};

/// The shader defs of Bevy's prepass, and of the normals it is
/// given.
const PREPASS: &str = "PREPASS_PIPELINE";
const NORMALS: &str = "VERTEX_NORMALS";
const NORMALS_OUT: &str = "NORMAL_PREPASS_OR_DEFERRED_PREPASS";

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
    mask: bool,
}

impl From<&ShadedMaterial> for ShadedKey {
    fn from(material: &ShadedMaterial) -> Self {
        Self {
            shader: material.shader.clone(),
            double_sided: material.double_sided,
            lit: material.lit,
            mask: matches!(material.alpha, AlphaMode::Mask(_)),
        }
    }
}

impl Material for ShadedMaterial {
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }

    // Stand-ins, which `specialize` replaces with the plugin's
    // shader. Given, they have Bevy bind the material in each
    // prepass and shadow, as `displace` may read it, and run the
    // fragment shader of one that may cut a masked or blended
    // material.
    fn prepass_vertex_shader() -> ShaderRef {
        ShaderRef::Handle(Handle::default())
    }

    fn prepass_fragment_shader() -> ShaderRef {
        ShaderRef::Handle(Handle::default())
    }

    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let key = key.bind_group_data;
        let mut defs = Vec::<ShaderDefVal>::new();
        if key.mask {
            defs.push(MASK.into());
        }
        let vertex = &mut descriptor.vertex;
        let prepass = vertex.shader_defs.contains(&PREPASS.into());
        // A prepass has normals only for a normal prepass, and
        // `displace` is given them in each pass so the shadow moves
        // with what casts it.
        if prepass
            && layout.0.contains(Mesh::ATTRIBUTE_NORMAL)
            && !vertex.shader_defs.contains(&NORMALS.into())
        {
            let normal = layout.0.get_layout(&[
                Mesh::ATTRIBUTE_NORMAL.at_shader_location(3),
            ])?;
            if let Some(buffer) = vertex.buffers.first_mut() {
                buffer.attributes.extend(normal.attributes);
            }
            defs.extend([NORMALS_OUT.into(), NORMALS.into()]);
        }
        vertex.shader = key.shader.clone();
        vertex.shader_defs.extend(defs.iter().cloned());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = key.shader;
            fragment.shader_defs.extend(defs);
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
