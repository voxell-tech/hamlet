//! What a plugin's views are built in and read from: its nodes, the
//! handlers of what the person does to them, and resources of its
//! own, each with a version its signals check.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use fynix::Signal;

use super::Key;
use super::nodes::Nodes;
use crate::bindings::exports::hamlet::engine::ui_guest::Action;

/// What runs, with the whole world, when the person acts on a node.
pub(crate) enum Handler {
    Click(Box<dyn Fn(&mut World) + Send + Sync>),
    Toggle(Box<dyn Fn(&mut World, bool) + Send + Sync>),
    Commit(Box<dyn Fn(&mut World, String) + Send + Sync>),
    Pick(Box<dyn Fn(&mut World, u32) + Send + Sync>),
}

struct Held {
    value: Box<dyn Any + Send + Sync>,
    /// Bumped each time it is lent out to be changed.
    version: u64,
}

/// The world of a plugin's panels: fynix's backend world.
#[derive(Default)]
pub struct World {
    pub(crate) nodes: Nodes,
    pub(crate) handlers: HashMap<Key, Handler>,
    resources: HashMap<TypeId, Held>,
}

impl World {
    /// Puts in `value`, in place of the one of its type.
    pub fn insert<R: Any + Send + Sync>(&mut self, value: R) {
        let version = self.version::<R>().map_or(0, |v| v + 1);
        self.resources.insert(
            TypeId::of::<R>(),
            Held {
                value: Box::new(value),
                version,
            },
        );
    }

    /// Puts in the default of `R` unless there is one.
    pub fn init_resource<R: Any + Default + Send + Sync>(&mut self) {
        if self.get::<R>().is_none() {
            self.insert(R::default());
        }
    }

    /// The resource of the type `R`, to change, put in at its default
    /// first when there is none.
    pub fn resource_or_init<R: Any + Default + Send + Sync>(
        &mut self,
    ) -> &mut R {
        self.init_resource::<R>();
        self.resource_mut::<R>()
    }

    /// The resource of the type `R`. Panics when there is none.
    pub fn resource<R: Any>(&self) -> &R {
        self.get::<R>().unwrap_or_else(|| {
            panic!("no resource {}", core::any::type_name::<R>())
        })
    }

    pub fn get<R: Any>(&self) -> Option<&R> {
        self.resources
            .get(&TypeId::of::<R>())
            .and_then(|held| held.value.downcast_ref())
    }

    /// The resource of the type `R`, to change: the signals that read
    /// it are read again. Panics when there is none.
    pub fn resource_mut<R: Any>(&mut self) -> &mut R {
        self.resources
            .get_mut(&TypeId::of::<R>())
            .and_then(|held| {
                held.version += 1;
                held.value.downcast_mut()
            })
            .unwrap_or_else(|| {
                panic!("no resource {}", core::any::type_name::<R>())
            })
    }

    fn version<R: Any>(&self) -> Option<u64> {
        self.resources
            .get(&TypeId::of::<R>())
            .map(|held| held.version)
    }

    /// Takes `key` away, and what was under it.
    pub(crate) fn despawn(&mut self, key: Key) {
        for key in self.nodes.despawn(key) {
            self.handlers.remove(&key);
        }
    }

    /// Runs the handler of `key` for `action`, if it has one for it.
    pub(crate) fn handle(&mut self, key: Key, action: Action) {
        let Some(handler) = self.handlers.remove(&key) else {
            return;
        };
        match (&handler, action) {
            (Handler::Click(run), Action::Click) => run(self),
            (Handler::Toggle(run), Action::Toggled(on)) => {
                run(self, on)
            }
            (Handler::Commit(run), Action::Committed(text)) => {
                run(self, text)
            }
            (Handler::Pick(run), Action::Picked(at)) => run(self, at),
            _ => {}
        }
        self.handlers.entry(key).or_insert(handler);
    }
}

/// A signal reading the resource `R`, read again when it changes.
pub fn resource<R: Any + Send + Sync, T>(
    read: impl Fn(&R) -> T + Send + Sync + 'static,
) -> Signal<World, T> {
    let mut seen = None;
    Signal::new(
        move |world: &World| read(world.resource::<R>()),
        move |world: &World| {
            let now = world.version::<R>();
            let changed = seen != now;
            seen = now;
            changed
        },
    )
}
