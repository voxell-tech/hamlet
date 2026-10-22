//! What a plugin's views are built in and read from: its nodes, the
//! handlers of what the person does to them, and resources of its
//! own, each with a version its signals check.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use fynix::Signal;
use hamlet_engine_api::DesktopRequest;

use super::Key;
use super::nodes::Nodes;
use crate::bindings::exports::hamlet::engine::ui_guest::Action;

/// What runs, with the whole world, when the person acts on a node.
pub(crate) enum Handler {
    Click(Box<dyn Fn(&mut World) + Send + Sync>),
    Toggle(Box<dyn Fn(&mut World, bool) + Send + Sync>),
    Commit(Box<dyn Fn(&mut World, String) + Send + Sync>),
    Pick(Box<dyn Fn(&mut World, u32) + Send + Sync>),
    Submit(Box<dyn Fn(&mut World, String) + Send + Sync>),
    Top(Box<dyn Fn(&mut World) + Send + Sync>),
    Chose(Box<dyn Fn(&mut World, u32) + Send + Sync>),
    Folder(Box<dyn Fn(&mut World, String) + Send + Sync>),
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
    pub(crate) handlers: HashMap<Key, Vec<Handler>>,
    /// The address each node leads to, told of while the pointer is
    /// over it.
    pub(crate) addresses: HashMap<Key, String>,
    /// The node whose address was told of last.
    pub(crate) pointed: Option<Key>,
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
            self.addresses.remove(&key);
            if self.pointed == Some(key) {
                self.point(key, false);
            }
        }
    }

    /// Tells the desktop service of the address of `key` as the
    /// pointer comes onto it, and that there is none as it leaves.
    pub(crate) fn point(&mut self, key: Key, on: bool) {
        if on && let Some(address) = self.addresses.get(&key) {
            self.pointed = Some(key);
            crate::desktop(&DesktopRequest::Hover(Some(
                address.clone(),
            )));
        } else if !on && self.pointed == Some(key) {
            self.pointed = None;
            crate::desktop(&DesktopRequest::Hover(None));
        }
    }

    /// Runs the handlers of `key` for `action`, those it has for it.
    pub(crate) fn handle(&mut self, key: Key, action: Action) {
        let Some(handlers) = self.handlers.remove(&key) else {
            return;
        };
        for handler in &handlers {
            handler.run(self, &action);
        }
        // A node built again in the meantime has handlers of its own.
        if self.nodes.has(key) {
            self.handlers.entry(key).or_insert(handlers);
        }
    }
}

impl Handler {
    fn run(&self, world: &mut World, action: &Action) {
        match (self, action) {
            (Handler::Click(run), Action::Click)
            | (Handler::Top(run), Action::ReachedTop) => run(world),
            (Handler::Toggle(run), Action::Toggled(on)) => {
                run(world, *on)
            }
            (Handler::Commit(run), Action::Committed(text))
            | (Handler::Submit(run), Action::Submitted(text))
            | (Handler::Folder(run), Action::Chosen(text)) => {
                run(world, text.clone())
            }
            (Handler::Pick(run), Action::Picked(at))
            | (Handler::Chose(run), Action::Chose(at)) => {
                run(world, *at)
            }
            _ => {}
        }
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
