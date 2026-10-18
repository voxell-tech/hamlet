//! Whether what a plugin contributed may show.

use super::granted::Granted;
use super::name::Capability;
use crate::plugin::PluginInfo;

/// Whether a contribution of `plugin` that needs `capability` is
/// live: a built-in plugin always may, any other only while the chief
/// allows it.
pub fn allowed(
    plugin: &PluginInfo,
    granted: Option<&Granted>,
    capability: Capability,
) -> bool {
    plugin.builtin || granted.is_some_and(|g| g.holds(capability))
}
