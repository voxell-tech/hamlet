//! Shader graphs: where a mesh's vertices go, and a surface's base
//! colour, roughness, metallic, emission and normal, as nodes and
//! wires, compiled to vertex and fragment shaders that extend the
//! realm's PBR standard material.

mod compile;
mod kinds;
mod wrap;

pub use compile::{Compiled, LIBRARY, SLOTS, compile};
pub use kinds::{ShaderKind, registry};
pub use wrap::{
    PREPASS_NORMALS, PREPASS_NORMALS_COMPRESSED, SURFACE_BINDING,
    prepass_shader, surface_shader, swatch_shader,
};

#[cfg(test)]
mod tests;
