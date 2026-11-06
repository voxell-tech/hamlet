//! Shader graphs: a surface's base colour, roughness, metallic,
//! emission and normal as nodes and wires, compiled to a fragment
//! shader that extends the realm's PBR standard material.

mod compile;
mod kinds;
mod wrap;

pub use compile::{Compiled, LIBRARY, SLOTS, compile};
pub use kinds::{ShaderKind, registry};
pub use wrap::{SURFACE_BINDING, surface_shader, swatch_shader};

#[cfg(test)]
mod tests;
