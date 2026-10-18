//! The pages the settings show, for the panel that draws them.

use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};

use super::SettingsPage;
use crate::capability::{Capability, Granted, allowed};
use crate::icons::Glyph;
use crate::plugin::{ContributedBy, PluginInfo};

/// Every page that may show, in the order the doors are listed: the
/// built-in plugins' first, then by plugin and title.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct SettingsPages(pub Vec<Line>);

/// One page.
#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    /// The entity of its [`SettingsPage`].
    pub item: Entity,
    pub title: String,
    pub icon: Glyph,
    pub holds: String,
    pub panel: String,
    /// The id of the plugin that added it.
    pub plugin: String,
    /// Whether that plugin is built in.
    pub builtin: bool,
}

impl SettingsPages {
    pub fn holds(&self, item: Entity) -> bool {
        self.0.iter().any(|line| line.item == item)
    }
}

pub(super) fn moved(
    changed: Query<
        (),
        Or<(
            Added<SettingsPage>,
            Changed<Granted>,
            Changed<PluginInfo>,
        )>,
    >,
    mut pages: RemovedComponents<SettingsPage>,
) -> bool {
    !changed.is_empty() || pages.read().count() > 0
}

pub(super) fn keep(
    pages: Query<(Entity, &SettingsPage, &ContributedBy)>,
    plugins: Query<(&PluginInfo, Option<&Granted>)>,
    mut shown: ResMut<SettingsPages>,
) {
    let mut now = pages
        .iter()
        .filter_map(|(item, page, by)| {
            let (info, granted) = plugins.get(by.0).ok()?;
            if !allowed(info, granted, Capability::UiSettings) {
                return None;
            }
            Some(Line {
                item,
                title: page.title.clone(),
                icon: page.icon,
                holds: page.holds.clone(),
                panel: page.panel.clone(),
                plugin: info.id.clone(),
                builtin: info.builtin,
            })
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| {
        (!a.builtin, &a.plugin, &a.title)
            .cmp(&(!b.builtin, &b.plugin, &b.title))
    });
    if shown.0 != now {
        shown.0 = now;
    }
}
