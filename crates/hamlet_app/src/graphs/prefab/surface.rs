//! One material type that draws any shader graph: each material
//! carries the shader compiled from its graph, and specializing its
//! pipeline puts that shader in place of the stand-in the type names,
//! so a prefab's parts can each use another graph at once. Each graph
//! is compiled to a shader once, however many parts use it.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::marker::PhantomData;

use anyhow::Context;
use bevy::app::{App, Startup};
use bevy::asset::uuid::Uuid;
use bevy::asset::{Asset, AssetId, Assets, Handle, uuid_handle};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey,
    MaterialExtensionPipeline, MaterialPlugin, StandardMaterial,
};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};
use hamlet_graph::Graph;
use hamlet_graph::prefab::Finish;
use hamlet_graph_editor::library::{Kind, Library, lone};
use hamlet_shader::{Compiled, compile};

use crate::graphs::shader::stages;
use crate::graphs::shader::swatch::Slots;

/// The shader the type names, in place of which each material's own
/// is put: the surface of a graph with no nodes.
const STAND_IN: Handle<Shader> =
    uuid_handle!("5c2e8b14-7a3d-4f69-b0e1-9d4c6a2f8e37");

/// The prepass vertex shader the type names, in place of which each
/// material's own is put.
const PREPASS_STAND_IN: Handle<Shader> =
    uuid_handle!("5c2e8b14-7a3d-4f69-b0e1-9d4c6a2f8e38");

/// The first half of the id of each shader compiled for a part.
const PREFIX: u64 = 0x5c2e_8b14_7a3d_4f6a;

/// The first half of the id of each prepass vertex shader compiled
/// for a part; the second is its main shader's.
const PREPASS_PREFIX: u64 = 0x5c2e_8b14_7a3d_4f6b;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<GraphMaterial>::default())
        .init_resource::<Shaders>()
        .add_systems(Startup, stand_in);
}

/// A material whose surface a shader graph works out.
pub(super) type GraphMaterial =
    ExtendedMaterial<StandardMaterial, GraphExt>;

/// The graph's slots, and the shader compiled from it.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(GraphKey)]
pub(super) struct GraphExt {
    #[uniform(100)]
    pub slots: Slots,
    /// The id of the shader compiled from the graph, made by
    /// [`Shaders::of`].
    pub shader: Uuid,
}

/// What a material's pipeline is specialized by: its shader.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GraphKey {
    shader: Uuid,
}

impl From<&GraphExt> for GraphKey {
    fn from(ext: &GraphExt) -> Self {
        Self { shader: ext.shader }
    }
}

impl MaterialExtension for GraphExt {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(STAND_IN)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(STAND_IN)
    }

    fn prepass_vertex_shader() -> ShaderRef {
        ShaderRef::Handle(PREPASS_STAND_IN)
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let main = key.bind_group_data.shader;
        let prepass = prepass_of(main);
        if descriptor.vertex.shader == STAND_IN {
            descriptor.vertex.shader =
                Handle::Uuid(main, PhantomData);
        } else if descriptor.vertex.shader == PREPASS_STAND_IN {
            descriptor.vertex.shader =
                Handle::Uuid(prepass, PhantomData);
        }
        // The prepass and shadow passes run the standard material's
        // fragment shaders; only the main pass's is the stand-in.
        if let Some(fragment) = descriptor.fragment.as_mut()
            && fragment.shader == STAND_IN
        {
            fragment.shader = Handle::Uuid(main, PhantomData);
        }
        stages::prepass_normals(descriptor, layout)
    }
}

/// The id of the prepass vertex shader that goes with the shader
/// `main`.
fn prepass_of(main: Uuid) -> Uuid {
    Uuid::from_u64_pair(PREPASS_PREFIX, main.as_u64_pair().1)
}

fn stand_in(mut shaders: ResMut<Assets<Shader>>) {
    if let Ok(compiled) = compile(&Graph::default()) {
        stages::insert(
            &mut shaders,
            STAND_IN.id(),
            PREPASS_STAND_IN.id(),
            &compiled,
            "surface_stand_in",
        );
    }
}

/// The shader of each graph's code compiled so far.
#[derive(Resource, Default)]
pub(super) struct Shaders(HashMap<String, Uuid>);

impl Shaders {
    /// The shader of `compiled`, made the first time its code is
    /// asked for.
    pub fn of(
        &mut self,
        compiled: &Compiled,
        shaders: &mut Assets<Shader>,
    ) -> Uuid {
        let made = self.0.len() as u64;
        *self.0.entry(compiled.code.clone()).or_insert_with(|| {
            // Kept by id rather than by a strong handle, so that a
            // pipeline key can name it; it lives as long as the app.
            let id = Uuid::from_u64_pair(PREFIX, made);
            let mut hasher = DefaultHasher::new();
            compiled.code.hash(&mut hasher);
            stages::insert(
                shaders,
                AssetId::Uuid { uuid: id },
                AssetId::Uuid {
                    uuid: prepass_of(id),
                },
                compiled,
                &format!("surface_{:016x}", hasher.finish()),
            );
            id
        })
    }
}

/// A graph as a part draws it: compiled, and with the values its
/// slots are filled from.
pub(super) struct Shaded {
    pub compiled: Compiled,
    /// The graph its slots read, its inputs' defaults what a
    /// material makes them.
    pub graph: Graph,
}

/// The shader graph open in the shader editor as it is now, saved or
/// not: what the parts that use it draw with, so its changes show on
/// them as they are made.
#[derive(Clone, Copy)]
pub(super) struct Live<'a> {
    /// Its name in the library.
    pub name: &'a str,
    pub graph: &'a Graph,
}

/// The graph `finish` draws with, compiled, or why it cannot be; none
/// for the file's own material.
pub(super) fn shaded(
    library: &Library,
    finish: &Finish,
    live: Option<Live>,
) -> Option<anyhow::Result<Shaded>> {
    Some(graph_of(library, finish, live)?.and_then(|graph| {
        let compiled = compile(&graph)?;
        Ok(Shaded { compiled, graph })
    }))
}

/// The graph `finish` draws with, a material's values set on its
/// shader's, or why there is none; none for the file's own material.
pub(super) fn graph_of(
    library: &Library,
    finish: &Finish,
    live: Option<Live>,
) -> Option<anyhow::Result<Graph>> {
    Some(match finish {
        Finish::Own => return None,
        Finish::Shader(name) => shader_graph(library, name, live),
        Finish::Material(name) => library
            .find(Kind::Material, name)
            .with_context(|| {
                format!("there is no material named {name}")
            })
            .and_then(|item| item.material())
            .and_then(|material| {
                let graph =
                    shader_graph(library, &material.shader, live)?;
                Ok(material.apply(&graph))
            }),
    })
}

/// Whether `finish` draws with the shader graph `name`, itself or as
/// a material's shader.
pub(super) fn uses(
    library: &Library,
    finish: &Finish,
    name: &str,
) -> bool {
    match finish {
        Finish::Own => false,
        Finish::Shader(shader) => shader == name,
        Finish::Material(material) => library
            .find(Kind::Material, material)
            .and_then(|item| item.material().ok())
            .is_some_and(|material| material.shader == name),
    }
}

/// The shader graph `name`: `live`'s when it is that one, else the
/// library's as saved.
fn shader_graph(
    library: &Library,
    name: &str,
    live: Option<Live>,
) -> anyhow::Result<Graph> {
    if let Some(live) = live
        && live.name == name
    {
        return Ok(live.graph.clone());
    }
    let item =
        library.find(Kind::Shader, name).with_context(|| {
            format!("there is no shader named {name}")
        })?;
    Ok(lone(item.read()?).graph)
}
