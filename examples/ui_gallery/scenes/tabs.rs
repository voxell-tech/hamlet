//! The Inspector turned to a tab a plugin added.

use bevy::ecs::system::{Res, ResMut};
use hamlet_sdk::inspector::InspectorSections;
use hamlet_ui::inspector::Section;

/// Turns the Inspector to the tab titled `title`, once it is there.
pub(super) fn turn_to(
    title: &str,
    sections: &InspectorSections,
    section: &mut Section,
) {
    if let Some(line) =
        sections.0.iter().find(|line| line.title == title)
    {
        *section = Section::Contributed(line.item);
    }
}

/// The Skills tab.
pub(super) fn skills(
    sections: Res<InspectorSections>,
    mut section: ResMut<Section>,
) {
    turn_to("SKILLS", &sections, &mut section);
}
