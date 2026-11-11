//! A compiled graph wrapped as whole shaders: the realm's PBR
//! surface, with its vertex stage, and its prepass; or a card's
//! swatch.
//!
//! The vertex shaders are Bevy's own, `bevy_pbr`'s `mesh.wesl` and
//! `prepass.wesl`, with one step added: after the vertex is read from
//! the mesh and morphed, and before it is placed in the world, the
//! graph's `vertex_stage` says where it goes and the way it faces.
//! Everything after it, skinning, the world, the clip space, the
//! tangent and the visibility range, is Bevy's as it is.

use crate::compile::Compiled;

/// The binding the realm's material reads its slots from, after those
/// of the standard material.
pub const SURFACE_BINDING: u32 = 100;

/// The shader def a prepass pipeline sets when its vertex carries the
/// mesh's normal at location 3 for the graph, though Bevy's prepass
/// would not.
pub const PREPASS_NORMALS: &str = "HAMLET_PREPASS_NORMALS";

/// Set beside [`PREPASS_NORMALS`] when that normal is compressed.
pub const PREPASS_NORMALS_COMPRESSED: &str =
    "HAMLET_PREPASS_NORMALS_COMPRESSED";

/// `compiled`'s code with its slots at [`SURFACE_BINDING`] of the
/// material's bind group.
fn material_code(compiled: &Compiled) -> String {
    compiled
        .code
        .replace("GROUP", "constants::MATERIAL_BIND_GROUP")
        .replace("SLOTS_BINDING", &SURFACE_BINDING.to_string())
}

/// What the graph's vertex stage reads at a vertex the mesh has at
/// `position`, facing `normal`, both in its own space, and where it
/// puts it. Shared by the main pass and the prepass.
const READ_VERTEX: &str = "
// Where the graph's vertex stage puts the vertex the mesh has at
// `position` facing `normal`, both in the mesh's own space.
fn hm_place(
    uv: vec2<f32>,
    uv_b: vec2<f32>,
    color: vec4<f32>,
    position: vec3<f32>,
    normal: vec3<f32>,
    world_from_local: mat4x4<f32>,
    local_from_world: mat4x4<f32>,
) -> Placed {
    let world = (world_from_local * vec4<f32>(position, 1.0)).xyz;
    let from_world = mat3x3<f32>(local_from_world[0].xyz, \
                           local_from_world[1].xyz, \
                           local_from_world[2].xyz);
    let camera = view.world_position.xyz;
    let local_camera = (local_from_world * vec4<f32>(camera, \
                           1.0)).xyz;
    let i = SurfaceIn(
        uv,
        world,
        hm_normalize(transpose(from_world) * normal),
        hm_normalize(camera - world),
        color,
        slots.v[0].x,
        position,
        normal,
        hm_normalize(local_camera - position),
        uv_b,
    );
    return vertex_stage(i);
}
";

/// `compiled` as a WESL shader extending the standard material, for
/// both stages of the main pass. The vertex shader is Bevy's with the
/// graph's vertex stage; the fragment shader puts the graph's surface
/// through the realm's PBR lighting, shadows and fog, or writes it
/// unlit. Its slots are a uniform at [`SURFACE_BINDING`].
pub fn surface_shader(compiled: &Compiled) -> String {
    let code = material_code(compiled);
    format!(
        "import bevy_pbr::render::{{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{{apply_pbr_lighting, \
         main_pass_post_lighting_processing}},
    forward_io::{{Vertex, UncompressedVertex, VertexOutput, \
         FragmentOutput, decompress_vertex}},
    mesh_bindings::mesh,
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}};

{code}
{READ_VERTEX}
// Bevy's VertexOutput, and what the graph's surface reads in the
// mesh's own space: where the vertex stage put the point, the way it
// faces, and the camera.
struct GraphVertexOutput {{
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @if(VERTEX_UVS_A)
    @location(2) uv: vec2<f32>,
    @if(VERTEX_UVS_B)
    @location(3) uv_b: vec2<f32>,
    @if(VERTEX_TANGENTS)
    @location(4) world_tangent: vec4<f32>,
    @if(VERTEX_COLORS)
    @location(5) color: vec4<f32>,
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    @location(6) @interpolate(flat) instance_index: u32,
    @if(VISIBILITY_RANGE_DITHER)
    @location(7) @interpolate(flat) visibility_range_dither: i32,
    @location(8) local_position: vec3<f32>,
    @location(9) local_normal: vec3<f32>,
    @location(10) @interpolate(flat) local_camera: vec3<f32>,
}}

@if(MORPH_TARGETS)
fn morph_vertex(vertex_in: UncompressedVertex, instance_index: u32) \
         -> UncompressedVertex {{
    var vertex = vertex_in;
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = vertex.index - first_vertex;
    let morph_descriptor_index = \
         mesh[instance_index].morph_descriptor_index;
    let weight_count = \
         bevy_pbr::render::morph::layer_count(morph_descriptor_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {{
        let weight = bevy_pbr::render::morph::weight_at(i, \
         morph_descriptor_index);
        if weight == 0.0 {{
            continue;
        }}
        vertex.position += weight * \
         bevy_pbr::render::morph::morph_position(vertex_index, i, \
         morph_descriptor_index);
        @if(VERTEX_NORMALS)
        vertex.normal += weight * \
         bevy_pbr::render::morph::morph_normal(vertex_index, i, \
         morph_descriptor_index);
        @if(VERTEX_TANGENTS)
        vertex.tangent += vec4(weight * \
         bevy_pbr::render::morph::morph_tangent(vertex_index, i, \
         morph_descriptor_index), 0.0);
    }}
    return vertex;
}}

@vertex
fn vertex(vertex_no_morph: Vertex) -> GraphVertexOutput {{
    var out: GraphVertexOutput;
    let uncompressed_vertex_no_morph = \
         decompress_vertex(vertex_no_morph, vertex_no_morph.instance_index);
@if(MORPH_TARGETS)
    var vertex = morph_vertex(uncompressed_vertex_no_morph, \
         vertex_no_morph.instance_index);
@else
    var vertex = uncompressed_vertex_no_morph;

    let mesh_world_from_local = \
         mesh_functions::get_world_from_local(vertex_no_morph.instance_index);

@if(SKINNED)
    var world_from_local = bevy_pbr::render::skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex_no_morph.instance_index
    );
@else
    var world_from_local = mesh_world_from_local;

    // The graph's vertex stage, on the vertex as the mesh gives it.
    var uv = vec2<f32>(0.0);
@if(VERTEX_UVS_A)
    uv = vertex.uv;
    var uv_b = vec2<f32>(0.0);
@if(VERTEX_UVS_B)
    uv_b = vertex.uv_b;
    var color = vec4<f32>(1.0);
@if(VERTEX_COLORS)
    color = vertex.color;
    var normal = vec3<f32>(0.0, 0.0, 0.0);
@if(VERTEX_NORMALS)
    normal = vertex.normal;
    let local_from_world = \
         mesh_functions::get_local_from_world(vertex_no_morph.instance_index);
    let placed = hm_place(uv, uv_b, color, vertex.position, normal, \
         world_from_local, local_from_world);
    vertex.position = placed.position;
@if(VERTEX_NORMALS)
    vertex.normal = placed.normal;
    out.local_position = placed.position;
    out.local_normal = placed.normal;
    out.local_camera = (local_from_world * \
         vec4<f32>(view.world_position.xyz, 1.0)).xyz;

@if(VERTEX_NORMALS && SKINNED)
    out.world_normal = \
         bevy_pbr::render::skinning::skin_normals(world_from_local, \
         vertex.normal);
@if(VERTEX_NORMALS && !SKINNED)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    );

@if(VERTEX_POSITIONS)
    out.world_position = \
         mesh_functions::mesh_position_local_to_world(world_from_local, \
         vec4<f32>(vertex.position, 1.0));
@if(VERTEX_POSITIONS)
    out.position = position_world_to_clip(out.world_position.xyz);

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;

@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_no_morph.instance_index
    );

@if(VERTEX_COLORS)
    out.color = vertex.color;

@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_no_morph.instance_index;

@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = \
         mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, mesh_world_from_local[3]);

    return out;
}}

@fragment
fn fragment(graph: GraphVertexOutput, @builtin(front_facing) is_front: \
         bool) -> FragmentOutput {{
    var in: VertexOutput;
    in.position = graph.position;
    in.world_position = graph.world_position;
    in.world_normal = graph.world_normal;
@if(VERTEX_UVS_A)
    in.uv = graph.uv;
@if(VERTEX_UVS_B)
    in.uv_b = graph.uv_b;
@if(VERTEX_TANGENTS)
    in.world_tangent = graph.world_tangent;
@if(VERTEX_COLORS)
    in.color = graph.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    in.instance_index = graph.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    in.visibility_range_dither = graph.visibility_range_dither;
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var uv = vec2<f32>(0.0);
@if(VERTEX_UVS_A)
    uv = in.uv;
    var uv_b = vec2<f32>(0.0);
@if(VERTEX_UVS_B)
    uv_b = in.uv_b;
    let i = SurfaceIn(uv, in.world_position.xyz, pbr_input.N, \
         pbr_input.V, pbr_input.material.base_color, slots.v[0].x, \
         graph.local_position, hm_normalize(graph.local_normal), \
         hm_normalize(graph.local_camera - graph.local_position), uv_b);
    let s = surface(i);
    pbr_input.material.base_color = s.base_color;
    pbr_input.material.perceptual_roughness = s.roughness;
    pbr_input.material.metallic = s.metallic;
    pbr_input.material.emissive = vec4<f32>(s.emission, 1.0);
    pbr_input.N = s.normal;
    var out: FragmentOutput;
    if s.unlit > 0.5 {{
        let shown = vec4<f32>(s.base_color.rgb + s.emission, \
         s.base_color.a);
        out.color = main_pass_post_lighting_processing(pbr_input, \
         shown);
    }} else {{
        out.color = main_pass_post_lighting_processing(pbr_input, \
         apply_pbr_lighting(pbr_input));
    }}
    return out;
}}
"
    )
}

/// `compiled` as a WESL vertex shader for the prepass and the shadow
/// passes of a material extending the standard material: Bevy's
/// prepass vertex shader with the graph's vertex stage, so the depth,
/// the normals, the motion and the shadows follow where it puts each
/// vertex. It gives Bevy's prepass `VertexOutput`, which the standard
/// material's prepass fragment shader reads. Its slots are a uniform
/// at [`SURFACE_BINDING`]. The vertex reads the normal at location 3
/// when the pipeline sets [`PREPASS_NORMALS`], else faces nowhere.
pub fn prepass_shader(compiled: &Compiled) -> String {
    let code = material_code(compiled);
    format!(
        "import bevy_pbr::{{
    prepass::io::VertexOutput,
    render::{{
        mesh_bindings::mesh,
        mesh_functions,
        mesh_view_bindings::view,
        view_transformations::position_world_to_clip,
    }},
}};

{code}
{READ_VERTEX}
// Bevy's prepass Vertex, its normal also there when the pipeline
// gives it for the graph.
struct GraphVertex {{
    @builtin(instance_index) instance_index: u32,
    @if(VERTEX_POSITIONS_COMPRESSED)
    @location(0) compressed_position: vec4<f32>,
    @if(!VERTEX_POSITIONS_COMPRESSED)
    @location(0) position: vec3<f32>,
    @if(VERTEX_UVS_A && VERTEX_UVS_A_COMPRESSED)
    @location(1) compressed_uv: vec2<f32>,
    @if(VERTEX_UVS_A && !VERTEX_UVS_A_COMPRESSED)
    @location(1) uv: vec2<f32>,
    @if(VERTEX_UVS_B && VERTEX_UVS_B_COMPRESSED)
    @location(2) compressed_uv_b: vec2<f32>,
    @if(VERTEX_UVS_B && !VERTEX_UVS_B_COMPRESSED)
    @location(2) uv_b: vec2<f32>,
    @if({PREPASS_NORMALS} && {PREPASS_NORMALS_COMPRESSED})
    @location(3) compressed_normal: vec2<f32>,
    @if({PREPASS_NORMALS} && !{PREPASS_NORMALS_COMPRESSED})
    @location(3) normal: vec3<f32>,
    @if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS && \
         VERTEX_TANGENTS_COMPRESSED)
    @location(4) compressed_tangent: vec2<f32>,
    @if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS && \
         !VERTEX_TANGENTS_COMPRESSED)
    @location(4) tangent: vec4<f32>,
    @if(SKINNED)
    @location(5) joint_indices: vec4<u32>,
    @if(SKINNED)
    @location(6) joint_weights: vec4<f32>,
    @if(VERTEX_COLORS)
    @location(7) color: vec4<f32>,
    @if(MORPH_TARGETS)
    @builtin(vertex_index) index: u32,
}}

@vertex
fn vertex(v: GraphVertex) -> VertexOutput {{
    var out: VertexOutput;
    let instance_index = v.instance_index;
    let metadata = mesh_functions::get_metadata(instance_index);

    // The vertex as the mesh gives it.
    var position: vec3<f32>;
@if(VERTEX_POSITIONS_COMPRESSED)
    position = \
         bevy_render::utils::decompress_vertex_position(v.compressed_position, \
         metadata.aabb_center, metadata.aabb_half_extents);
@else
    position = v.position;
    let position_no_morph = position;
    var uv = vec2<f32>(0.0);
@if(VERTEX_UVS_A && VERTEX_UVS_A_COMPRESSED)
    uv = bevy_render::utils::decompress_vertex_uv(v.compressed_uv, \
         metadata.uv_channels_min_and_extents[0]);
@if(VERTEX_UVS_A && !VERTEX_UVS_A_COMPRESSED)
    uv = v.uv;
    var uv_b = vec2<f32>(0.0);
@if(VERTEX_UVS_B && VERTEX_UVS_B_COMPRESSED)
    uv_b = bevy_render::utils::decompress_vertex_uv(v.compressed_uv_b, \
         metadata.uv_channels_min_and_extents[1]);
@if(VERTEX_UVS_B && !VERTEX_UVS_B_COMPRESSED)
    uv_b = v.uv_b;
    var normal = vec3<f32>(0.0, 0.0, 0.0);
@if({PREPASS_NORMALS} && {PREPASS_NORMALS_COMPRESSED})
    normal = bevy_render::utils::decompress_vertex_normal(v.compressed_normal);
@if({PREPASS_NORMALS} && !{PREPASS_NORMALS_COMPRESSED})
    normal = v.normal;
    let normal_no_morph = normal;
    var tangent = vec4<f32>(0.0);
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS && \
         VERTEX_TANGENTS_COMPRESSED)
    tangent = bevy_render::utils::decompress_vertex_tangent(v.compressed_tangent);
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS && \
         !VERTEX_TANGENTS_COMPRESSED)
    tangent = v.tangent;
    var color = vec4<f32>(1.0);
@if(VERTEX_COLORS)
    color = v.color;

@if(MORPH_TARGETS) {{
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = v.index - first_vertex;
    let morph_descriptor_index = \
         mesh[instance_index].morph_descriptor_index;
    let weight_count = \
         bevy_pbr::render::morph::layer_count(morph_descriptor_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {{
        let weight = bevy_pbr::render::morph::weight_at(i, \
         morph_descriptor_index);
        if weight == 0.0 {{
            continue;
        }}
        position += weight * \
         bevy_pbr::render::morph::morph_position(vertex_index, i, \
         morph_descriptor_index);
        @if({PREPASS_NORMALS})
        normal += weight * \
         bevy_pbr::render::morph::morph_normal(vertex_index, i, \
         morph_descriptor_index);
        @if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS)
        tangent += vec4(weight * \
         bevy_pbr::render::morph::morph_tangent(vertex_index, i, \
         morph_descriptor_index), 0.0);
    }}
}}

    let mesh_world_from_local = \
         mesh_functions::get_world_from_local(instance_index);
@if(SKINNED)
    var world_from_local = bevy_pbr::render::skinning::skin_model(
        v.joint_indices,
        v.joint_weights,
        instance_index
    );
@else
    var world_from_local = mesh_world_from_local;

    // The graph's vertex stage.
    let local_from_world = mesh_functions::get_local_from_world(instance_index);
    let placed = hm_place(uv, uv_b, color, position, normal, \
         world_from_local, local_from_world);
    position = placed.position;
    normal = placed.normal;

    out.world_position = \
         mesh_functions::mesh_position_local_to_world(world_from_local, \
         vec4<f32>(position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {{
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}}

@if(VERTEX_UVS_A)
    out.uv = uv;

@if(VERTEX_UVS_B)
    out.uv_b = uv_b;

@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS) {{
@if(VERTEX_NORMALS && SKINNED)
    out.world_normal = \
         bevy_pbr::render::skinning::skin_normals(world_from_local, normal);
@elif(VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        normal,
        instance_index
    );

@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        tangent,
        instance_index
    );
}}

@if(VERTEX_COLORS)
    out.color = color;

@if(MOTION_VECTOR_PREPASS) {{
    var prev_position = position_no_morph;
@if(MORPH_TARGETS && HAS_PREVIOUS_MORPH) {{
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = v.index - first_vertex;
    let morph_descriptor_index = \
         mesh[instance_index].morph_descriptor_index;
    let weight_count = \
         bevy_pbr::render::morph::layer_count(morph_descriptor_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {{
        let weight = bevy_pbr::render::morph::prev_weight_at(i, \
         morph_descriptor_index);
        if weight == 0.0 {{
            continue;
        }}
        prev_position += weight * \
         bevy_pbr::render::morph::morph_position(vertex_index, i, \
         morph_descriptor_index);
    }}
}}

@if(SKINNED && HAS_PREVIOUS_SKIN)
    let prev_model = bevy_pbr::render::skinning::skin_prev_model(
        v.joint_indices,
        v.joint_weights,
        instance_index
    );
@else
    let prev_model = \
         mesh_functions::get_previous_world_from_local(instance_index);

    // Where the vertex stage put the vertex last frame, as far as the
    // graph reads the same now.
    let prev_placed = hm_place(uv, uv_b, color, prev_position, \
         normal_no_morph, prev_model, local_from_world);
    out.previous_world_position = \
         mesh_functions::mesh_position_local_to_world(
        prev_model,
        vec4<f32>(prev_placed.position, 1.0)
    );
}}

@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = instance_index;

@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = \
         mesh_functions::get_visibility_range_dither_level(
        instance_index, mesh_world_from_local[3]);

    return out;
}}
"
    )
}

/// `compiled` as a WESL fragment shader for a UI node: what the node
/// numbered in the `shown` uniform gives, on a sphere shaded by a
/// light from the top left. Its slots are at binding 0 and `shown` at
/// 1.
pub fn swatch_shader(compiled: &Compiled) -> String {
    let code = compiled
        .code
        .replace("GROUP", "1")
        .replace("SLOTS_BINDING", "0");
    format!(
        "import bevy_ui_render::ui_vertex_output::UiVertexOutput;

{code}
@group(1) @binding(1) var<uniform> shown: vec4<u32>;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {{
    {SPHERE}
}}
"
    )
}

/// The body of a swatch's fragment: the sphere's surface at the
/// pixel, and what `preview` shows there.
pub(crate) const SPHERE: &str =
    "let p = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let r = length(p);
    let edge = max(fwidth(r), 1e-3);
    let cover = 1.0 - smoothstep(1.0 - edge, 1.0, r);
    let n = vec3<f32>(p, sqrt(max(1.0 - r * r, 0.0)));
    let uv = vec2<f32>(atan2(n.x, n.z) / 6.2831853 + 0.5, \
     acos(clamp(-n.y, -1.0, 1.0)) / 3.1415927);
    let i = SurfaceIn(uv, n * 0.5, n, vec3<f32>(0.0, 0.0, 1.0), \
     vec4<f32>(0.8, 0.8, 0.8, 1.0), slots.v[0].x, n * 0.5, n, \
     vec3<f32>(0.0, 0.0, 1.0), uv);
    let c = preview(shown.x, i);
    let shade = 0.7 + 0.3 * max(dot(n, normalize(vec3<f32>(-0.4, \
     0.5, 0.8))), 0.0);
    return vec4<f32>(c.rgb * shade, c.a * cover);";
