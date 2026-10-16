//! The models of the home: its endpoints, its catalogue, and each
//! hamlet's own list of the allowed ones. A hamlet runs a model only
//! when both say so.

use std::collections::BTreeMap;

mod choice;
mod home;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use hamlet_core::agents::{Charter, Settlement};
use hamlet_home::limits::Limits;

pub(crate) use self::choice::{assign, of_agent, of_hamlet};
pub(crate) use self::home::narrowed;
pub use self::home::{endpoints, shelf};

/// The most models the catalogue shows at once.
const SHOWN: usize = 40;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Shelf>()
        .init_resource::<Draft>()
        .add_systems(Update, follow);
}

/// Each hamlet's own list of models as the window shows it, by the
/// hamlet's name.
#[derive(Resource, Default)]
pub struct Shelf {
    lists: BTreeMap<String, Vec<String>>,
    /// Each hamlet's default model and the model its chief chose
    /// for an agent, by the hamlet's name.
    defaults: BTreeMap<String, Option<String>>,
    pinned: BTreeMap<String, BTreeMap<String, String>>,
    /// The window wrote a hamlet's limits and has not read them
    /// again.
    wrote: bool,
    /// The reason the last change could not be made.
    problem: Option<String>,
}

impl Shelf {
    /// Shows `limits` as those of the hamlet `name`.
    fn show(&mut self, name: &str, limits: Limits) {
        if self.lists.get(name) != Some(&limits.models) {
            self.lists.insert(name.to_string(), limits.models);
        }
        if self.defaults.get(name) != Some(&limits.model) {
            self.defaults.insert(name.to_string(), limits.model);
        }
        if self.pinned.get(name) != Some(&limits.pinned) {
            self.pinned.insert(name.to_string(), limits.pinned);
        }
    }
}

/// The words the chief types to narrow the catalogue.
#[derive(Resource, Default)]
pub struct Draft {
    narrow: String,
}

/// Shows each hamlet's list as its limits were last read, when they
/// were read again.
pub fn follow(
    hamlets: Query<(&Settlement, Ref<Charter>)>,
    mut closed: RemovedComponents<Settlement>,
    mut shelf: ResMut<Shelf>,
) {
    let closed = closed.read().count() > 0;
    let wrote = shelf.wrote;
    let read = hamlets.iter().any(|(_, limits)| limits.is_changed());
    if !wrote && !closed && !read {
        return;
    }
    for (hamlet, charter) in &hamlets {
        let limits = if wrote {
            // The core has not read the window's own change yet.
            Limits::load(hamlet.record().root()).ok()
        } else if charter.is_changed() {
            charter.0.clone()
        } else {
            continue;
        };
        shelf.show(&hamlet.name, limits.unwrap_or_default());
    }
    if closed {
        let stands = |name: &String| {
            hamlets.iter().any(|(hamlet, _)| hamlet.name == *name)
        };
        shelf.lists.retain(|name, _| stands(name));
        shelf.defaults.retain(|name, _| stands(name));
        shelf.pinned.retain(|name, _| stands(name));
    }
    if wrote {
        shelf.wrote = false;
    }
}
