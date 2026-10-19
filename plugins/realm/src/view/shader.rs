//! The material of a plugin's shader: the WGSL the host of the
//! plugins checked, wrapped in the fragment shader of a mesh, with
//! the values its node gives it. Each shader is compiled once, as a
//! pipeline of its own.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};
use bevy::shader::ValidateShader;
use hamlet_realm_api::scene::{INTERFACE, PARAMS};

/// What comes before a plugin's source: the mesh's view and the
/// values the material binds.
const HEAD: &str = "
#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> \
                    hamlet_params: HamletParams;
";

/// What comes after it: the entry point, which colours each point
/// with the plugin's `shade`.
const TAIL: &str = "
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.0);
#endif
    let surface = Surface(uv, in.world_position.xyz, \
                    normalize(in.world_normal), globals.time);
    return vec4<f32>(shade(surface).rgb, 1.0);
}
";

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<PluginMaterial>::default());
}

/// A shape painted by a plugin's shader.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
#[bind_group_data(Compiled)]
pub struct PluginMaterial {
    #[uniform(0)]
    params: Params,
    shader: Handle<Shader>,
}

/// Mirrors `HamletParams` of the interface.
#[derive(ShaderType, Clone, Debug)]
struct Params {
    values: [Vec4; PARAMS],
}

/// The shader a material's pipeline is specialised with.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Compiled(Handle<Shader>);

impl From<&PluginMaterial> for Compiled {
    fn from(material: &PluginMaterial) -> Self {
        Self(material.shader.clone())
    }
}

impl PluginMaterial {
    pub fn new(
        shader: Handle<Shader>,
        params: [Vec4; PARAMS],
    ) -> Self {
        Self {
            params: Params { values: params },
            shader,
        }
    }
}

impl Material for PluginMaterial {
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
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = key.bind_group_data.0;
        }
        Ok(())
    }
}

/// The shader of the plugin `plugin` kept as `id`, its `source`
/// wrapped, with the renderer's checks of bounds on.
pub fn compiled(plugin: &str, id: &str, source: &str) -> Shader {
    let wrapped = format!("{HEAD}\n{INTERFACE}\n{source}\n{TAIL}");
    let mut shader = Shader::from_wgsl(
        wrapped,
        format!("plugins/{plugin}/{id}.wgsl"),
    );
    shader.validate_shader = ValidateShader::Enabled;
    shader
}
