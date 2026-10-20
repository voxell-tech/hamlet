//! What [`export!`](crate::export) calls for retained panels: each
//! panel's mounted views, kept in step with the world after each
//! input and frame.

use core::cell::RefCell;
use core::time::Duration;
use std::collections::HashMap;

use fynix::{Cx, Mounted, Tick, View};

use super::{Hamlet, Key, States, Theme, World};
use crate::Plugin;
use crate::bindings::exports::hamlet::engine::ui_guest::{
    Action, Motions, UiEvent,
};
use crate::bindings::hamlet::engine::ui::Node;

std::thread_local! {
    static UI: RefCell<Panels> = RefCell::new(Panels::default());
}

#[derive(Default)]
struct Panels {
    world: World,
    theme: Theme,
    mounted: HashMap<String, Shown>,
}

/// A panel built, and its root.
struct Shown {
    mounted: Mounted<Hamlet, Theme>,
    root: Key,
}

impl Panels {
    /// Brings every panel up to date, its transitions moved on by
    /// `delta`: whether something still moves.
    fn update(&mut self, delta: Duration) -> bool {
        let tick = Tick {
            delta,
            reduced_motion: false,
        };
        let mut moving = false;
        for shown in self.mounted.values_mut() {
            self.world.forget_gone(&mut shown.mounted);
            shown.mounted.run_effects(&mut self.world);
            shown
                .mounted
                .update_structure(&mut self.world, &self.theme);
            self.world.forget_gone(&mut shown.mounted);
            moving |= shown.mounted.update_elements(
                &mut self.world,
                &self.theme,
                tick,
            );
        }
        self.world.nodes.gone.clear();
        moving
    }

    fn unmount(&mut self, name: &str) {
        if let Some(shown) = self.mounted.remove(name) {
            self.world.despawn(shown.root);
        }
    }
}

impl World {
    /// Drops from `mounted` the mounts of the nodes despawned since.
    fn forget_gone(&self, mounted: &mut Mounted<Hamlet, Theme>) {
        for &key in &self.nodes.gone {
            mounted.unmount(key);
        }
    }
}

pub fn mount<P: Plugin>(
    plugin: &mut P,
    name: String,
    root: Node,
    motions: Motions,
) -> bool {
    UI.with_borrow_mut(|panels| {
        panels.unmount(&name);
        panels.theme = Theme {
            interact: Duration::from_secs_f32(motions.interact),
            expand: Duration::from_secs_f32(motions.expand),
        };
        let root = panels.world.nodes.adopt(root);
        let view = plugin.mount(&name, &mut panels.world);
        let mut mounted = Mounted::default();
        {
            let mut cx = Cx::new(
                &mut panels.world,
                &panels.theme,
                &mut mounted,
            );
            cx.under(root, |cx| view.build(cx));
        }
        panels.mounted.insert(name, Shown { mounted, root });
        panels.update(Duration::ZERO)
    })
}

/// Runs `call` with the world of the plugin's panels, and brings the
/// panels up to date with what it changed.
pub fn with_world<R>(call: impl FnOnce(&mut World) -> R) -> R {
    UI.with_borrow_mut(|panels| {
        let made = call(&mut panels.world);
        panels.update(Duration::ZERO);
        made
    })
}

pub fn unmount(name: &str) {
    UI.with_borrow_mut(|panels| panels.unmount(name));
}

pub fn input(events: Vec<UiEvent>) -> bool {
    UI.with_borrow_mut(|panels| {
        for event in events {
            let (states, on) = match event.action {
                Action::Hover(on) => (States::HOVER, on),
                Action::Press(on) => (States::PRESS, on),
                action => {
                    panels.world.handle(event.key, action);
                    continue;
                }
            };
            panels.world.nodes.enter(event.key, states, on);
            for shown in panels.mounted.values_mut() {
                shown.mounted.mark_dirty(event.key);
            }
        }
        panels.update(Duration::ZERO)
    })
}

pub fn frame(micros: u32) -> bool {
    UI.with_borrow_mut(|panels| {
        panels.update(Duration::from_micros(micros.into()))
    })
}
