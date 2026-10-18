//! The ids of a plugin's panels in the dock.

/// The id of a plugin's panel in the dock. Prefixed with the plugin's
/// id, so that no plugin can take the id of another panel.
pub fn dock_id(plugin: &str, panel: &str) -> String {
    format!("{plugin}/{panel}")
}

/// The id a plugin knows its panel by, from the dock's id.
pub fn local_id<'a>(
    plugin: &str,
    dock_id: &'a str,
) -> Option<&'a str> {
    dock_id.strip_prefix(plugin)?.strip_prefix('/')
}

/// The id of the plugin a panel of the dock belongs to.
pub fn plugin_of(dock_id: &str) -> &str {
    dock_id
        .split_once('/')
        .map_or(dock_id, |(plugin, _)| plugin)
}
