//! The host side of `wit/hamlet-plugin.wit`, generated.

/// The version of the package in the WIT file.
pub const VERSION: &str = "0.1.0";

wasmtime::component::bindgen!({
    path: "wit/hamlet-plugin.wit",
    world: "hamlet",
});
