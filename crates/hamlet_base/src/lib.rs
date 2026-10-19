//! What every layer shares: the home the app opened, the runtime
//! background work runs on, and the rules of a plugin's names.

use bevy_ecs::resource::Resource;
use hamlet_home::Home;

/// Between a plugin's id and the name of its tool, in the name an
/// agent calls it by. No tool added by hand may have it, so a name
/// with it is always a plugin's.
pub const JOIN: &str = "__";

/// The tokio runtime that background work is spawned on.
#[derive(Resource)]
pub struct Tokio(pub tokio::runtime::Handle);

/// The home the app opened.
#[derive(Resource)]
pub struct HomeDir(pub Home);

/// Whether `name` can be a plugin's id or a tool's name in the name
/// an agent calls the tool by: no dot, no `__` and no underscore at
/// an edge, so that no two pairs join into the same name.
pub fn joinable(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('.')
        && !name.contains(JOIN)
        && !name.starts_with('_')
        && !name.ends_with('_')
}

/// `text` as one plain line: control characters become spaces and
/// brackets go, so that it cannot pass for a label or a line of
/// its own.
pub fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '[' | ']'))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}
