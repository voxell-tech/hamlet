//! A plugin that runs as the app, with no limits.

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;

use super::Entry;

/// On the entity of a native plugin: its library is code that runs as
/// the app. No capability gates it. The text is the hash of the
/// library the chief let in, in hex.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct FullAccess(pub String);

/// What a native plugin's entry declared: the function that adds its
/// panels, commands and menus. The host takes it out of the world
/// after the entry returns. It lives here, not in the crate that
/// loads libraries, so that the type is the same whether or not a
/// library was built with the loader.
#[derive(Resource, Clone, Copy)]
pub struct Declared(pub fn(&mut Entry));

/// On the entity of a native plugin whose library was not loaded: the
/// reason, as Settings shows it.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct NotLoaded(pub String);

/// Whether the text of a `manifest.toml` says `kind = "native"`.
/// Whoever reads the other kinds of plugin leaves these folders to
/// whoever loads native ones.
pub fn is_native(manifest: &str) -> bool {
    #[derive(serde::Deserialize)]
    struct Kind {
        kind: Option<String>,
    }
    toml::from_str::<Kind>(manifest)
        .is_ok_and(|found| found.kind.as_deref() == Some("native"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_kind_native_is_native() {
        assert!(is_native("id = \"a\"\nkind = \"native\"\n"));
        assert!(!is_native("id = \"a\"\nkind = \"wasm\"\n"));
        assert!(!is_native("id = \"a\"\n"));
        assert!(!is_native("not toml ["));
    }
}
