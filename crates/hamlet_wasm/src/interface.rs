//! The host side of `wit/hamlet-plugin.wit`, generated.

/// The version of the package in the WIT file.
pub const VERSION: &str = "0.4.0";

/// The package of the interface, which a component names in the
/// interfaces it imports and exports.
const PACKAGE: &str = "hamlet:plugin/";

/// The interface version an import or export name was built for, if
/// it names this package at another version.
pub fn built_for(name: &str) -> Option<&str> {
    let (_, version) = name.strip_prefix(PACKAGE)?.split_once('@')?;
    (version != VERSION).then_some(version)
}

wasmtime::component::bindgen!({
    path: "wit/hamlet-plugin.wit",
    world: "hamlet",
    additional_derives: [PartialEq],
});
