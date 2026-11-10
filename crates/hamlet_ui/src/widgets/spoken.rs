//! The name a screen reader gives a button: the words on it, or the
//! name given to one that has only an icon.

use accesskit::{Node, Role};
use bevy::a11y::AccessibilityNode;
use bevy::app::{App, PostUpdate};
use bevy::ecs::component::Component;
use bevy::ecs::entity::{Entity, EntityHashSet};
use bevy::ecs::hierarchy::{ChildOf, Children};
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

/// The screen reader's node for a picture that stands for `words`,
/// as a status icon stands for its state.
pub fn picture(words: impl Into<String>) -> AccessibilityNode {
    let mut node = Node::new(Role::Image);
    node.set_label(words.into());
    AccessibilityNode::from(node)
}

/// Gives each button a node in the screen reader's tree, named by
/// its [`Spoken`] name, else by the words within it. A button is
/// named again when it or its name changes, or the words within it.
fn name_buttons(
    mut commands: Commands,
    changed: Query<
        Entity,
        (With<Button>, Or<(Changed<Button>, Changed<Spoken>)>),
    >,
    reworded: Query<Entity, Changed<Text>>,
    buttons: Query<Option<&Spoken>, With<Button>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    texts: Query<&Text>,
) {
    let mut named = EntityHashSet::default();
    let held = reworded.iter().filter_map(|text| {
        parents
            .iter_ancestors(text)
            .find(|above| buttons.contains(*above))
    });
    for button in changed.iter().chain(held) {
        if !named.insert(button) {
            continue;
        }
        let Ok(spoken) = buttons.get(button) else {
            continue;
        };
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
