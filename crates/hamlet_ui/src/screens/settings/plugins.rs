//! The Plugins page: every plugin there is, each with a switch, what
//! each asks for and what the chief chose, by plugin and by
//! capability.

mod capabilities;
mod control;
mod list;
mod permissions;

use bevy::app::{App, Update};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_sdk::capability::{Asks, Choices, Uninstalled};
use hamlet_sdk::plugin::{Blocked, PluginInfo, Status};
use hamlet_sdk::reload::Reload;

use super::switch;
use crate::icons::Glyph;
use crate::looks::bare;
use crate::{AnyView, View, tabs};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Tab>()
        .init_resource::<Open>()
        .add_plugins((
            list::plugin,
            permissions::plugin,
            capabilities::plugin,
        ))
        .add_systems(Update, tabs::mark::<Tab>);
}

/// The two ways the page lists permissions.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    /// Each plugin, with what it asks for.
    #[default]
    ByPlugin,
    /// Each capability, with the plugins that ask for it.
    ByCapability,
}

/// The plugin whose permissions are open, if one is.
#[derive(Resource, Clone, PartialEq, Eq, Default, Debug)]
pub struct Open(pub Option<String>);

const TABS: [(Tab, Glyph, &str); 2] = [
    (Tab::ByPlugin, Glyph::Power, "BY PLUGIN"),
    (Tab::ByCapability, Glyph::Shield, "BY CAPABILITY"),
];

/// Whether a plugin, what it asks for or what the chief chose for it
/// has come, changed or gone.
fn moved(
    changed: Query<
        (),
        Or<(
            Added<PluginInfo>,
            Changed<Status>,
            Changed<Asks>,
            Changed<Choices>,
            Changed<Reload>,
            Added<Uninstalled>,
            Added<Blocked>,
        )>,
    >,
    mut gone: RemovedComponents<PluginInfo>,
    mut installed: RemovedComponents<Uninstalled>,
    mut freed: RemovedComponents<Blocked>,
) -> bool {
    !changed.is_empty()
        || gone.read().count() > 0
        || installed.read().count() > 0
        || freed.read().count() > 0
}

/// The page's contents.
pub(super) fn page() -> impl View {
    keyed(resource::<Open, _>(|open| open.0.is_some()), |open| {
        match open {
            true => permissions::view().boxed(),
            false => column((
                tabs::strip(&TABS, |world: &mut World, tab| {
                    *world.resource_mut::<Tab>() = tab;
                }),
                by_tab(),
            ))
            .gap(12.0)
            .boxed(),
        }
    })
    .within(bare())
}

/// The list or the capabilities, as the tab says.
fn by_tab() -> AnyView {
    keyed(resource::<Tab, _>(|tab| *tab), |tab| match tab {
        Tab::ByPlugin => list::view().boxed(),
        Tab::ByCapability => capabilities::view().boxed(),
    })
    .within(bare())
    .boxed()
}
