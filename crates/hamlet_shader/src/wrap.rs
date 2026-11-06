//! A compiled graph wrapped as a whole shader: the realm's PBR
//! surface, or a card's swatch.

use crate::compile::Compiled;

/// The binding the realm's material reads its slots from, after those
/// of the standard material.
pub const SURFACE_BINDING: u32 = 100;

/// `compiled` as a WESL fragment shader extending the standard
/// material: the graph's surface goes through the realm's PBR
/// lighting, shadows and fog, or is written unlit. Its slots are a
/// uniform at [`SURFACE_BINDING`].
pub fn surface_shader(compiled: &Compiled) -> String {
    let code = compiled
        .code
        .replace("GROUP", "constants::MATERIAL_BIND_GROUP")
        .replace("SLOTS_BINDING", &SURFACE_BINDING.to_string());
    format!(
        "import bevy_pbr::render::{{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{{apply_pbr_lighting, \
         main_pass_post_lighting_processing}},
    forward_io::{{VertexOutput, FragmentOutput}},
}};

{code}
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) \
         -> FragmentOutput {{
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var uv = vec2<f32>(0.0);
    @if(VERTEX_UVS_A) {{
        uv = in.uv;
    }}
    let i = SurfaceIn(uv, in.world_position.xyz, pbr_input.N, \
         pbr_input.V, pbr_input.material.base_color, slots.v[0].x);
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
     vec4<f32>(0.8, 0.8, 0.8, 1.0), slots.v[0].x);
    let c = preview(shown.x, i);
    let shade = 0.7 + 0.3 * max(dot(n, normalize(vec3<f32>(-0.4, \
     0.5, 0.8))), 0.0);
    return vec4<f32>(c.rgb * shade, c.a * cover);";
