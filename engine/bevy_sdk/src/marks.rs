//! Balloons over the entities of the scene, and the words beside the
//! pointer while it is over one of the plugin's views. Each is sent
//! as it changes, in [`PostUpdate`].

use bevy_app::{App, PostUpdate};
use bevy_ecs::prelude::*;

pub use crate::bindings::hamlet::engine::scene::PointerLabel;
use crate::bindings::hamlet::engine::scene::{
    Balloon, set_pointer_label,
};
use crate::scene::{HostEntity, SceneSync};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<PointerWords>().add_systems(
        PostUpdate,
        (balloon, pointer_words).after(SceneSync),
    );
}

/// A balloon of speech or thought over an entity. It pops in as it
/// comes and shrinks away as it is taken off.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct SceneBalloon {
    /// Its words, or, with none, dots that swell in turn.
    pub text: String,
    /// Dimmer, trailing rounds in place of a point.
    pub thought: bool,
    /// Pixels between the entity and the balloon's point.
    pub gap: f32,
    /// The farthest the camera is from the entity for it to be
    /// shown, in the world's units, or any distance at 0.
    pub far: f32,
}

/// The words beside the pointer while it is over one of the plugin's
/// views, or none.
#[derive(Resource, Clone, Default, Debug)]
pub struct PointerWords(pub Option<PointerLabel>);

fn balloon(
    shown: Query<
        (&HostEntity, &SceneBalloon),
        Or<(Changed<SceneBalloon>, Added<HostEntity>)>,
    >,
    hosts: Query<&HostEntity>,
    mut gone: RemovedComponents<SceneBalloon>,
) {
    for (host, balloon) in &shown {
        host.entity.set_balloon(Some(&Balloon {
            text: balloon.text.clone(),
            thought: balloon.thought,
            gap: balloon.gap,
            far: balloon.far,
        }));
    }
    for entity in gone.read() {
        if let Ok(host) = hosts.get(entity) {
            host.entity.set_balloon(None);
        }
    }
}

fn pointer_words(words: Res<PointerWords>) {
    if words.is_changed() {
        set_pointer_label(words.0.as_ref());
    }
}
