//! The Inspector turned to a tab a plugin added.

use bevy::ecs::system::{Res, ResMut};
use hamlet_sdk::inspector::InspectorSections;
use hamlet_ui::inspector::Section;

use super::Scene;

/// Turns the Inspector to the tab of the scene, once it is there.
pub(super) fn turn(
    scene: Res<Scene>,
    sections: Res<InspectorSections>,
    mut section: ResMut<Section>,
) {
    let title = match *scene {
        Scene::SkillsTab => "SKILLS",
        _ => "NOTES",
    };
    if let Some(line) =
        sections.0.iter().find(|line| line.title == title)
    {
        *section = Section::Contributed(line.item);
    }
}
