//! Material graphs: what a surface looks like, written as nodes and
//! wires, compiled to a shader the realm lights like everything else.
//!
//! A material graph is a [`hamlet_blueprint::Graph`] of the kinds in
//! [`registry`]. [`compile`] turns it into WGSL, and
//! [`surface_shader`] and [`swatch_shader`] wrap that for the realm
//! and for a card's swatch. Values set on the graph are uniforms,
//! filled by [`Compiled::values`].

mod compile;
mod kinds;
mod wrap;

pub use compile::{Compiled, LIBRARY, SLOTS, compile};
pub use kinds::{MaterialKind, registry};
pub use wrap::{SURFACE_BINDING, surface_shader, swatch_shader};

#[cfg(test)]
mod tests;
