//! A plugin's WGSL, checked with naga before Bevy is given it. A
//! source is plain WGSL, without the preprocessor, that declares no
//! bindings and no entry point of its own but a compute shader's
//! `main`: the engine's bindings come before it under fixed names,
//! and its entry points after it, calling the plugin's functions. The
//! source is checked between a stand-in of each, so a name of the
//! engine's cannot be taken, and is then wrapped as Bevy draws it.

use std::error::Error;

use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{
    AddressSpace, Block, Module, Scalar, ShaderStage, Statement,
    TypeInner, VectorSize,
};

use crate::guard::GpuInfo;

/// `vec4<f32>`s of params a plugin's shader is given.
pub const PARAMS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ShaderKind {
    /// `fn shade(surface: Surface) -> vec4<f32>`, and optionally
    /// `fn displace(vertex: Vertex) -> vec3<f32>`.
    Material,
    /// `fn post(drawn: Pass) -> vec4<f32>`.
    Pass,
    /// `@compute @workgroup_size(..) fn main(..)`.
    Compute,
    /// `fn shade(decal: Decal) -> vec4<f32>`.
    Decal,
}

/// What a source is checked against.
#[derive(Clone, Copy, Debug)]
pub struct ShaderRules {
    /// Bytes of the source.
    pub bytes: usize,
    /// Whether wgpu bounds every loop on the device's backend. Where
    /// it does not, a source with a loop is refused.
    pub loops_bounded: bool,
    /// Invocations of one workgroup.
    pub invocations: u32,
    /// Invocations of one workgroup along each axis.
    pub workgroup_size: [u32; 3],
    /// Bytes of a workgroup's shared memory.
    pub workgroup_memory: u32,
}

impl ShaderRules {
    /// The rules of `gpu`, for sources of `bytes` at most.
    pub fn of(gpu: &GpuInfo, bytes: usize) -> Self {
        Self {
            bytes,
            loops_bounded: gpu.loops_bounded,
            invocations: gpu.invocations,
            workgroup_size: gpu.workgroup_size,
            workgroup_memory: gpu.workgroup_memory,
        }
    }
}

/// A source that passed, as Bevy is given it.
#[derive(Clone, PartialEq, Debug)]
pub struct CheckedShader {
    pub kind: ShaderKind,
    pub wgsl: String,
}

/// The bindings of a material and of a decal, their group left to
/// fill in.
const BINDINGS: &str = "
@group(GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(GROUP) @binding(1) var picture: texture_2d<f32>;
@group(GROUP) @binding(2) var picture_sampler: sampler;
@group(GROUP) @binding(3) var<storage, read> data: array<vec4<f32>>;
";

const MATERIAL: &str = "
struct Surface {
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    color: vec4<f32>,
    tag: u32,
    time: f32,
}

struct Vertex {
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    tag: u32,
    time: f32,
}
";

const DECAL: &str = "
struct Decal {
    position: vec3<f32>,
    local: vec3<f32>,
    uv: vec2<f32>,
    time: f32,
}
";

const PASS: &str = "
struct Frame {
    size: vec2<f32>,
    time: f32,
    delta: f32,
}

struct Pass {
    uv: vec2<f32>,
    color: vec4<f32>,
    size: vec2<f32>,
    time: f32,
}

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> params: array<vec4<f32>, 16>;
@group(0) @binding(3) var<uniform> frame: Frame;
";

const COMPUTE: &str = "
struct Frame {
    time: f32,
    delta: f32,
    count: u32,
    unused: u32,
}

@group(0) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(0) @binding(1) var<storage, read_write> data: \
                       array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> extra: \
                       array<vec4<f32>>;
@group(0) @binding(3) var<uniform> frame: Frame;
";

/// The names of the modules the wrapped material imports, taken in
/// the stand-in so the source cannot use them.
const MATERIAL_IMPORTS: &str = "
var<private> mesh_functions: f32;
var<private> forward_io: f32;
var<private> view_transformations: f32;
var<private> pbr_types: f32;
var<private> pbr_functions: f32;
var<private> mesh_view_bindings: f32;
";

const DECAL_IMPORTS: &str = "
var<private> mesh_functions: f32;
var<private> forward_io: f32;
var<private> view_transformations: f32;
var<private> prepass_utils: f32;
var<private> mesh_view_bindings: f32;
var<private> tonemapping: f32;
";

const PASS_IMPORTS: &str = "
var<private> fullscreen_vertex_shader: f32;
";

/// The material's entry points in the stand-in, calling `displace`
/// where the source has it.
fn material_stand_in(displaces: bool) -> String {
    let moved = if displaces {
        "displace(Vertex(vec3<f32>(0.0), vec3<f32>(0.0, 1.0, 0.0), \
         vec2<f32>(0.0), index, 0.0))"
    } else {
        "vec3<f32>(0.0)"
    };
    format!(
        "
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) \
         vec4<f32> {{
    return vec4<f32>({moved}, 1.0);
}}

@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> @location(0) \
         vec4<f32> {{
    let seen = textureSample(picture, picture_sampler, at.xy);
    return shade(Surface(at.xyz, vec3<f32>(0.0, 1.0, 0.0), at.xy, \
         seen + data[0] + params[0], 0u, 0.0));
}}
"
    )
}

const DECAL_STAND_IN: &str = "
@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> @location(0) \
                              vec4<f32> {
    let seen = textureSample(picture, picture_sampler, at.xy);
    return shade(Decal(at.xyz, (seen + data[0] + params[0]).xyz, \
                              at.xy, 0.0));
}
";

const PASS_STAND_IN: &str = "
@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> @location(0) \
                             vec4<f32> {
    let seen = textureSample(screen, screen_sampler, at.xy);
    return post(Pass(at.xy, seen + params[0], frame.size, \
                             frame.time));
}
";

/// What Bevy is given for a material, around the source.
const MATERIAL_HEAD: &str = "
#import bevy_pbr::mesh_functions
#import bevy_pbr::forward_io
#import bevy_pbr::view_transformations
#import bevy_pbr::pbr_types
#import bevy_pbr::pbr_functions
#import bevy_pbr::mesh_view_bindings
";

fn material_tail(displaces: bool) -> String {
    let moved = if displaces {
        "local = local + displace(Vertex(local, normal, uv, tag, \
         mesh_view_bindings::globals.time));"
    } else {
        ""
    };
    format!(
        "
@vertex
fn vertex(input: forward_io::Vertex) -> forward_io::VertexOutput {{
    var out: forward_io::VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(input.instance_index);
    var local = input.position;
#ifdef VERTEX_NORMALS
    let normal = input.normal;
#else
    let normal = vec3<f32>(0.0, 1.0, 0.0);
#endif
#ifdef VERTEX_UVS_A
    let uv = input.uv;
#else
    let uv = vec2<f32>(0.0);
#endif
    let tag = mesh_functions::get_tag(input.instance_index);
    {moved}
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(local, 1.0));
    out.position = view_transformations::position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(normal, input.instance_index);
#ifdef VERTEX_UVS_A
    out.uv = uv;
#endif
#ifdef VERTEX_COLORS
    out.color = input.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = input.instance_index;
#endif
    return out;
}}

@fragment
fn fragment(input: forward_io::VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {{
    var normal = normalize(input.world_normal);
    if !front {{
        normal = -normal;
    }}
#ifdef VERTEX_UVS_A
    let uv = input.uv;
#else
    let uv = vec2<f32>(0.0);
#endif
#ifdef VERTEX_COLORS
    let color = input.color;
#else
    let color = vec4<f32>(1.0);
#endif
    let tag = mesh_functions::get_tag(input.instance_index);
    let shaded = shade(Surface(input.world_position.xyz, normal, uv, color, tag, mesh_view_bindings::globals.time));
    var pbr = pbr_types::pbr_input_new();
    pbr.material.base_color = shaded;
    pbr.material.flags = pbr.material.flags | pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    pbr.frag_coord = input.position;
    pbr.world_position = input.world_position;
    pbr.world_normal = normal;
    pbr.N = normal;
    pbr.is_orthographic = mesh_view_bindings::view.clip_from_view[3].w == 1.0;
    pbr.V = pbr_functions::calculate_view(input.world_position, pbr.is_orthographic);
#ifdef HAMLET_LIT
    let lit = pbr_functions::apply_pbr_lighting(pbr);
#else
    let lit = shaded;
#endif
    return pbr_functions::main_pass_post_lighting_processing(pbr, lit);
}}
"
    )
}

const PASS_HEAD: &str = "
#import bevy_core_pipeline::fullscreen_vertex_shader
";

const PASS_TAIL: &str = "
@fragment
fn fragment(input: fullscreen_vertex_shader::FullscreenVertexOutput) \
                         -> @location(0) vec4<f32> {
    let seen = textureSample(screen, screen_sampler, input.uv);
    return post(Pass(input.uv, seen, frame.size, frame.time));
}
";

const DECAL_HEAD: &str = "
#import bevy_pbr::mesh_functions
#import bevy_pbr::forward_io
#import bevy_pbr::view_transformations
#import bevy_pbr::prepass_utils
#import bevy_pbr::mesh_view_bindings
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif
";

/// The decal's box is drawn wherever it stands; each pixel is of what
/// the depth prepass saw there, and is left out when that lies
/// outside the box.
const DECAL_TAIL: &str = "
@fragment
fn fragment(input: forward_io::VertexOutput) -> @location(0) vec4<f32> {
#ifdef DEPTH_PREPASS
    let depth = prepass_utils::prepass_depth(input.position, 0u);
    let seen = view_transformations::position_ndc_to_world(view_transformations::frag_coord_to_ndc(vec4<f32>(input.position.xy, depth, 1.0)));
#else
    let seen = input.world_position.xyz;
#endif
    let local = (mesh_functions::get_local_from_world(input.instance_index) * vec4<f32>(seen, 1.0)).xyz;
    // Before anything is left out, so that `shade` may take
    // derivatives.
    var color = shade(Decal(seen, local, local.xz + 0.5, mesh_view_bindings::globals.time));
    if any(abs(local) > vec3<f32>(0.5)) {
        discard;
    }
#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, mesh_view_bindings::view.color_grading);
#endif
#ifdef HAMLET_ADD
    color = vec4<f32>(color.rgb * color.a, 0.0);
#endif
#ifdef BLEND_MULTIPLY
    color = vec4<f32>(color.rgb * color.a, color.a);
#endif
    return color;
}
";

/// The shader def a lit material's shader is specialized with.
pub const LIT: &str = "HAMLET_LIT";

/// The shader def of a decal that adds its light to what is there.
pub const ADD: &str = "HAMLET_ADD";

/// `source` checked as a shader of `kind` within `rules`: what Bevy
/// is given, or why it is refused.
pub fn check(
    kind: ShaderKind,
    source: &str,
    rules: &ShaderRules,
) -> Result<CheckedShader, String> {
    if source.len() > rules.bytes {
        return Err(format!(
            "the source has {} bytes, the most is {}",
            source.len(),
            rules.bytes
        ));
    }
    if source.contains('#') {
        return Err("a source has no `#`: it is plain WGSL".into());
    }
    let (bindings, imports) = match kind {
        ShaderKind::Material => (
            format!("{MATERIAL}{}", BINDINGS.replace("GROUP", "2")),
            MATERIAL_IMPORTS.to_owned(),
        ),
        ShaderKind::Pass => {
            (PASS.to_owned(), PASS_IMPORTS.to_owned())
        }
        ShaderKind::Compute => (COMPUTE.to_owned(), String::new()),
        ShaderKind::Decal => (
            format!("{DECAL}{}", BINDINGS.replace("GROUP", "2")),
            DECAL_IMPORTS.to_owned(),
        ),
    };
    let head = format!("{bindings}{imports}\n");
    let before = head.lines().count();
    let lines = source.lines().count();
    let at = Lines { before, lines };
    let first = at.parse(&format!("{head}{source}\n"))?;
    let displaces = has(&first, "displace");
    // Before the stand-in calls them, so a function of the wrong
    // shape is named rather than failing where it is called.
    own(&first, kind, displaces, rules)?;
    let stand_in = match kind {
        ShaderKind::Material => material_stand_in(displaces),
        ShaderKind::Pass => PASS_STAND_IN.to_owned(),
        ShaderKind::Compute => String::new(),
        ShaderKind::Decal => DECAL_STAND_IN.to_owned(),
    };
    let whole = format!("{head}{source}\n{stand_in}");
    let module = at.parse(&whole)?;
    Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .map_err(|error| {
            at.say(
                error.location(&whole).map(|at| at.line_number),
                &chain(error.as_inner()),
            )
        })?;
    let shared = workgroup_bytes(&module);
    if shared > u64::from(rules.workgroup_memory) {
        return Err(format!(
            "{shared} bytes of workgroup memory, the most is {}",
            rules.workgroup_memory
        ));
    }
    if !rules.loops_bounded && module_loops(&module) {
        return Err("this GPU cannot bound a loop, so a source has \
                    none"
            .into());
    }
    let group = BINDINGS.replace("GROUP", "#{MATERIAL_BIND_GROUP}");
    let wgsl = match kind {
        ShaderKind::Material => format!(
            "{MATERIAL_HEAD}{MATERIAL}{group}\n{source}\n{}",
            material_tail(displaces)
        ),
        ShaderKind::Pass => {
            format!("{PASS_HEAD}{PASS}\n{source}\n{PASS_TAIL}")
        }
        ShaderKind::Compute => format!("{COMPUTE}\n{source}\n"),
        ShaderKind::Decal => format!(
            "{DECAL_HEAD}{DECAL}{group}\n{source}\n{DECAL_TAIL}"
        ),
    };
    Ok(CheckedShader { kind, wgsl })
}

/// Where the source sits in what is checked.
struct Lines {
    /// Lines of the engine's before it.
    before: usize,
    lines: usize,
}

impl Lines {
    fn parse(&self, text: &str) -> Result<Module, String> {
        naga::front::wgsl::parse_str(text).map_err(|error| {
            self.say(
                error.location(text).map(|at| at.line_number),
                error.message(),
            )
        })
    }

    /// `message`, with the line of the source it is about.
    fn say(&self, line: Option<u32>, message: &str) -> String {
        match line.map(|line| line as usize) {
            Some(line)
                if line > self.before
                    && line <= self.before + self.lines =>
            {
                format!("line {}: {message}", line - self.before)
            }
            _ => message.to_owned(),
        }
    }
}

/// An error and the errors under it, on one line.
fn chain(error: &dyn Error) -> String {
    let mut words = error.to_string();
    let mut under = error.source();
    while let Some(next) = under {
        words.push_str(": ");
        words.push_str(&next.to_string());
        under = next.source();
    }
    words
}

fn has(module: &Module, name: &str) -> bool {
    module
        .functions
        .iter()
        .any(|(_, function)| function.name.as_deref() == Some(name))
}

/// Whether the module holds only what a source of `kind` may: the
/// engine's bindings and none of its own, the entry points the kind
/// has, and its functions as the kind calls them.
fn own(
    module: &Module,
    kind: ShaderKind,
    displaces: bool,
    rules: &ShaderRules,
) -> Result<(), String> {
    let engine: &[&str] = match kind {
        ShaderKind::Material | ShaderKind::Decal => {
            &["params", "picture", "picture_sampler", "data"]
        }
        ShaderKind::Pass => {
            &["screen", "screen_sampler", "params", "frame"]
        }
        ShaderKind::Compute => &["params", "data", "extra", "frame"],
    };
    for (_, global) in module.global_variables.iter() {
        let name = global.name.as_deref().unwrap_or("a global");
        let shared = kind == ShaderKind::Compute
            && global.space == AddressSpace::WorkGroup;
        if global.space != AddressSpace::Private
            && !shared
            && !engine.contains(&name)
        {
            return Err(format!(
                "`{name}` is a binding; a source has none of its own"
            ));
        }
    }
    if !module.overrides.is_empty() {
        return Err("a source has no `override`".into());
    }
    let entries: &[&str] = match kind {
        ShaderKind::Material => &["vertex", "fragment"],
        ShaderKind::Pass | ShaderKind::Decal => &["fragment"],
        ShaderKind::Compute => &["main"],
    };
    if let Some(entry) = module
        .entry_points
        .iter()
        .find(|entry| !entries.contains(&entry.name.as_str()))
    {
        return Err(format!(
            "`{}` is an entry point, which a source of this kind \
             has not",
            entry.name
        ));
    }
    let vector = |size| TypeInner::Vector {
        size,
        scalar: Scalar::F32,
    };
    match kind {
        ShaderKind::Material => {
            takes(
                module,
                "shade",
                "Surface",
                vector(VectorSize::Quad),
            )?;
            if displaces {
                takes(
                    module,
                    "displace",
                    "Vertex",
                    vector(VectorSize::Tri),
                )?;
            }
        }
        ShaderKind::Pass => {
            takes(module, "post", "Pass", vector(VectorSize::Quad))?;
        }
        ShaderKind::Decal => {
            takes(
                module,
                "shade",
                "Decal",
                vector(VectorSize::Quad),
            )?;
        }
        ShaderKind::Compute => {
            let Some(main) =
                module.entry_points.first().filter(|entry| {
                    entry.name == "main"
                        && entry.stage == ShaderStage::Compute
                })
            else {
                return Err("no `@compute @workgroup_size(..) fn \
                            main(..)`"
                    .into());
            };
            let [x, y, z] = main.workgroup_size;
            let invocations = x.saturating_mul(y).saturating_mul(z);
            if invocations > rules.invocations {
                return Err(format!(
                    "a workgroup of {invocations} invocations, the \
                     most is {}",
                    rules.invocations
                ));
            }
            let [most_x, most_y, most_z] = rules.workgroup_size;
            if x > most_x || y > most_y || z > most_z {
                return Err(format!(
                    "a workgroup of {x} by {y} by {z}, the most is \
                     {most_x} by {most_y} by {most_z}"
                ));
            }
        }
    }
    Ok(())
}

/// Whether the module has `fn name(_: argument) -> result`.
fn takes(
    module: &Module,
    name: &str,
    argument: &str,
    result: TypeInner,
) -> Result<(), String> {
    let wanted = || {
        let shape = match result {
            TypeInner::Vector {
                size: VectorSize::Tri,
                ..
            } => "vec3<f32>",
            _ => "vec4<f32>",
        };
        format!("no `fn {name}(_: {argument}) -> {shape}`")
    };
    let function = module
        .functions
        .iter()
        .map(|(_, function)| function)
        .find(|function| function.name.as_deref() == Some(name))
        .ok_or_else(wanted)?;
    let named =
        |ty| module.types[ty].name.as_deref() == Some(argument);
    let fits = function.arguments.len() == 1
        && named(function.arguments[0].ty)
        && function.result.as_ref().is_some_and(|made| {
            module.types[made.ty].inner == result
        });
    if fits { Ok(()) } else { Err(wanted()) }
}

/// Bytes of the module's workgroup memory, each variable rounded up
/// to 16 as backends lay it out. The module is validated, so each
/// size fits.
fn workgroup_bytes(module: &Module) -> u64 {
    module
        .global_variables
        .iter()
        .filter(|(_, global)| global.space == AddressSpace::WorkGroup)
        .map(|(_, global)| {
            let size =
                module.types[global.ty].inner.size(module.to_ctx());
            u64::from(size).next_multiple_of(16)
        })
        .sum()
}

/// Whether a function of the module loops.
fn module_loops(module: &Module) -> bool {
    module
        .functions
        .iter()
        .map(|(_, function)| function)
        .chain(
            module.entry_points.iter().map(|entry| &entry.function),
        )
        .any(|function| loops(&function.body))
}

fn loops(block: &Block) -> bool {
    block.iter().any(|statement| match statement {
        Statement::Loop { .. } => true,
        Statement::Block(inner) => loops(inner),
        Statement::If { accept, reject, .. } => {
            loops(accept) || loops(reject)
        }
        Statement::Switch { cases, .. } => {
            cases.iter().any(|case| loops(&case.body))
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: ShaderRules = ShaderRules {
        bytes: 4_096,
        loops_bounded: true,
        invocations: 256,
        workgroup_size: [256, 256, 64],
        workgroup_memory: 16_384,
    };

    fn refused(kind: ShaderKind, source: &str) -> String {
        check(kind, source, &RULES).expect_err(source)
    }

    #[test]
    fn a_material_is_checked_against_the_engines_names() {
        let shade = "fn shade(surface: Surface) -> vec4<f32> { \
                     return surface.color * params[0]; }";
        assert!(check(ShaderKind::Material, shade, &RULES).is_ok());
        let binding = format!(
            "{shade}\n@group(0) @binding(9) var<uniform> mine: f32;"
        );
        assert!(
            refused(ShaderKind::Material, &binding).contains("mine")
        );
        let taken = format!("{shade}\nfn vertex() {{}}");
        assert!(!refused(ShaderKind::Material, &taken).is_empty());
        let preprocessed =
            format!("#import bevy_pbr::pbr_types\n{shade}");
        assert!(
            refused(ShaderKind::Material, &preprocessed)
                .contains('#')
        );
        let wrong = "fn shade(surface: Surface) -> vec3<f32> { \
                     return surface.normal; }";
        assert!(
            refused(ShaderKind::Material, wrong).contains("shade")
        );
    }

    #[test]
    fn a_decal_is_checked_against_the_engines_names() {
        let shade = "fn shade(decal: Decal) -> vec4<f32> { let edge \
                     = fwidth(length(decal.uv - 0.5)); return \
                     params[0] * edge + data[0] * decal.position.y; \
                     }";
        assert!(check(ShaderKind::Decal, shade, &RULES).is_ok());
        let material = "fn shade(surface: Surface) -> vec4<f32> { \
                        return surface.color; }";
        assert!(check(ShaderKind::Decal, material, &RULES).is_err());
        let binding = format!(
            "{shade}\n@group(2) @binding(9) var<uniform> mine: f32;"
        );
        assert!(
            refused(ShaderKind::Decal, &binding).contains("mine")
        );
        let taken = format!("{shade}\nfn tonemapping() {{}}");
        assert!(!refused(ShaderKind::Decal, &taken).is_empty());
        let vertex = format!(
            "{shade}\n@vertex fn vertex() -> @builtin(position) \
             vec4<f32> {{ return vec4<f32>(0.0); }}"
        );
        assert!(
            refused(ShaderKind::Decal, &vertex).contains("vertex")
        );
    }

    #[test]
    fn errors_name_the_line_of_the_source() {
        let broken = "fn shade(surface: Surface) -> vec4<f32> \
                      {\nreturn nothing;\n}";
        assert!(
            refused(ShaderKind::Material, broken)
                .starts_with("line 2")
        );
    }

    #[test]
    fn loops_and_workgroups_are_held_to_the_rules() {
        let looping = "@compute @workgroup_size(64) fn \
                       main(@builtin(global_invocation_id) id: \
                       vec3<u32>) { for (var i = 0u; i < 4u; i++) { \
                       data[id.x] += params[0]; } }";
        assert!(check(ShaderKind::Compute, looping, &RULES).is_ok());
        let unbounded = ShaderRules {
            loops_bounded: false,
            ..RULES
        };
        assert!(
            check(ShaderKind::Compute, looping, &unbounded).is_err()
        );
        let wide = "@compute @workgroup_size(32, 32) fn main() {}";
        assert!(refused(ShaderKind::Compute, wide).contains("1024"));
        let deep = "@compute @workgroup_size(1, 1, 128) fn main() {}";
        assert!(refused(ShaderKind::Compute, deep).contains("64"));
        let shared = "var<workgroup> kept: array<vec4<f32>, \
                      2048>;\n@compute @workgroup_size(1) fn main() \
                      { kept[0] = params[0]; }";
        assert!(
            refused(ShaderKind::Compute, shared).contains("16384")
        );
    }
}
