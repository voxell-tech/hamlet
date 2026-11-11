//! The shaders a graph's material runs: both stages of its main pass,
//! and the vertex stage of its prepass and shadows, so what the
//! graph's vertex stage moves casts its shadow where it is drawn.

use bevy::asset::{AssetId, Assets};
use bevy::mesh::{
    Mesh, MeshAttributeCompressionFlags, MeshVertexBufferLayoutRef,
};
use bevy::render::render_resource::{
    RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::Shader;
use hamlet_shader::{
    Compiled, PREPASS_NORMALS, PREPASS_NORMALS_COMPRESSED,
    prepass_shader, surface_shader,
};

/// Puts `compiled`'s shader for the main pass at `main` and its
/// prepass vertex shader at `prepass`, named after `name`.
pub(crate) fn insert(
    shaders: &mut Assets<Shader>,
    main: AssetId<Shader>,
    prepass: AssetId<Shader>,
    compiled: &Compiled,
    name: &str,
) {
    let _ = shaders.insert(
        main,
        Shader::from_wesl(
            surface_shader(compiled),
            format!("hamlet_shader/{name}.wesl"),
        ),
    );
    let _ = shaders.insert(
        prepass,
        Shader::from_wesl(
            prepass_shader(compiled),
            format!("hamlet_shader/{name}_prepass.wesl"),
        ),
    );
}

/// Gives a prepass pipeline's vertex the mesh's normal, which the
/// graph's vertex stage reads to move a vertex, as the main pass's
/// does. Bevy's prepass reads it only for a normal or deferred
/// prepass, and never for shadows; there it is added at the location
/// Bevy's would be. Other pipelines are left as they are.
pub(crate) fn prepass_normals(
    descriptor: &mut RenderPipelineDescriptor,
    layout: &MeshVertexBufferLayoutRef,
) -> Result<(), SpecializedMeshPipelineError> {
    let defs = &mut descriptor.vertex.shader_defs;
    if !defs.contains(&"PREPASS_PIPELINE".into())
        || !layout.0.contains(Mesh::ATTRIBUTE_NORMAL)
    {
        return Ok(());
    }
    let carried =
        defs.contains(&"NORMAL_PREPASS_OR_DEFERRED_PREPASS".into());
    let skinned = defs.contains(&"SKINNED".into());
    defs.push(PREPASS_NORMALS.into());
    if layout
        .0
        .get_attribute_compression()
        .contains(MeshAttributeCompressionFlags::COMPRESS_NORMAL)
    {
        defs.push(PREPASS_NORMALS_COMPRESSED.into());
    }
    if carried {
        return Ok(());
    }
    // Bevy's prepass attributes and their locations, with the normal.
    let mut attributes = Vec::new();
    for (attribute, location) in [
        (Mesh::ATTRIBUTE_POSITION, 0),
        (Mesh::ATTRIBUTE_UV_0, 1),
        (Mesh::ATTRIBUTE_UV_1, 2),
        (Mesh::ATTRIBUTE_NORMAL, 3),
        (Mesh::ATTRIBUTE_COLOR, 7),
    ] {
        if layout.0.contains(attribute.id) {
            attributes.push(attribute.at_shader_location(location));
        }
    }
    if skinned {
        attributes
            .push(Mesh::ATTRIBUTE_JOINT_INDEX.at_shader_location(5));
        attributes
            .push(Mesh::ATTRIBUTE_JOINT_WEIGHT.at_shader_location(6));
    }
    descriptor.vertex.buffers =
        vec![layout.0.get_layout(&attributes)?];
    Ok(())
}
