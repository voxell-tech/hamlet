//! What a screen reader says a button is: the words on it, or the
//! name given to one that has only an icon.

use accesskit::{Node, Role};
use bevy::a11y::AccessibilityNode;
use bevy::app::{App, PostUpdate};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::{Changed, Or, With};
use bevy::ecs::system::{Commands, Query};
use bevy::ui::widget::Text;
use bevy::ui_widgets::Button;

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, name_buttons);
}

/// The name a screen reader gives a button that has no words, only
/// an icon: what its tooltip says.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Spoken(pub String);

impl Spoken {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

/// Gives each button a node in the screen reader's tree, named by
/// its [`Spoken`] name, else by the words within it.
fn name_buttons(
    mut commands: Commands,
    buttons: Query<
        (Entity, Option<&Spoken>),
        (With<Button>, Or<(Changed<Button>, Changed<Spoken>)>),
    >,
    children: Query<&Children>,
    texts: Query<&Text>,
) {
    for (button, spoken) in &buttons {
        let name =
            spoken.map(|spoken| spoken.0.clone()).or_else(|| {
                let words = children
                    .iter_descendants(button)
                    .filter_map(|inside| texts.get(inside).ok())
                    .map(|text| text.0.trim())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>();
                (!words.is_empty()).then(|| words.join(" "))
            });
        let mut node = Node::new(Role::Button);
        if let Some(name) = name {
            node.set_label(name);
        }
        commands
            .entity(button)
            .try_insert(AccessibilityNode::from(node));
    }
}
