//! The tabs the inspector shows, for the page that draws them.

use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};

use super::InspectorSection;
use crate::capability::{Capability, Granted, allowed};
use crate::dock::Panel;
use crate::icons::Glyph;
use crate::plugin::{ContributedBy, PluginInfo};

/// Every tab that may show, in the order a page lists them: by
/// plugin, then title.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct InspectorSections(pub Vec<Line>);

/// One tab.
#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    /// The entity of its [`InspectorSection`].
    pub item: Entity,
    pub target: String,
    pub title: String,
    pub panel: String,
    pub icon: Glyph,
    /// The id of the plugin that added it.
    pub plugin: String,
}

impl InspectorSections {
    /// The tabs of the page of the things of this kind.
    pub fn of<'a>(
        &'a self,
        kind: &'a str,
    ) -> impl Iterator<Item = &'a Line> {
        self.0.iter().filter(move |line| line.target == kind)
    }

    pub fn holds(&self, item: Entity) -> bool {
        self.0.iter().any(|line| line.item == item)
    }
}

pub(super) fn moved(
    changed: Query<
        (),
        Or<(
            Added<InspectorSection>,
            Added<Panel>,
            Changed<Granted>,
            Changed<PluginInfo>,
        )>,
    >,
    mut sections: RemovedComponents<InspectorSection>,
    mut panels: RemovedComponents<Panel>,
) -> bool {
    !changed.is_empty()
        || sections.read().count() > 0
        || panels.read().count() > 0
}

pub(super) fn keep(
    sections: Query<(Entity, &InspectorSection, &ContributedBy)>,
    panels: Query<&Panel>,
    plugins: Query<(&PluginInfo, Option<&Granted>)>,
    mut shown: ResMut<InspectorSections>,
) {
    let mut now = sections
        .iter()
        .filter_map(|(item, section, by)| {
            let (info, granted) = plugins.get(by.0).ok()?;
            if !allowed(info, granted, Capability::UiInspector) {
                return None;
            }
            let icon = panels
                .iter()
                .find(|panel| panel.id == section.panel)
                .map_or(Glyph::Menu, |panel| panel.icon);
            Some(Line {
                item,
                target: section.target.clone(),
                title: section.title.clone(),
                panel: section.panel.clone(),
                icon,
                plugin: info.id.clone(),
            })
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| {
        (&a.plugin, &a.title).cmp(&(&b.plugin, &b.title))
    });
    if shown.0 != now {
        shown.0 = now;
    }
}
