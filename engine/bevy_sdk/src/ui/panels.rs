//! Each panel's mounted views, kept beside the world and brought in
//! step with it after each call.

use core::time::Duration;
use std::collections::HashMap;

use bevy_ecs::world::World;
use fynix::{Cx, Mounted, Tick, View};

use crate::bindings::exports::hamlet::engine::ui_guest::{
    Action, Motions, UiEvent,
};
use crate::bindings::hamlet::engine::ui::Node;
use crate::ui::{AnyView, Hamlet, Key, States, Theme, Ui};

#[derive(Default)]
pub(crate) struct Panels {
    theme: Theme,
    mounted: HashMap<String, Shown>,
}

/// A panel built, and its root.
struct Shown {
    mounted: Mounted<Hamlet, Theme>,
    root: Key,
}

impl Panels {
    /// Brings every panel up to date with `world`, its transitions
    /// moved on by `delta`: whether something still moves.
    pub(crate) fn update(
        &mut self,
        world: &mut World,
        delta: Duration,
    ) -> bool {
        let tick = Tick {
            delta,
            reduced_motion: false,
        };
        let mut moving = false;
        for shown in self.mounted.values_mut() {
            shown.forget_gone(world);
            shown.mounted.run_effects(world);
            shown.mounted.update_structure(world, &self.theme);
            shown.forget_gone(world);
            moving |= shown.mounted.update_elements(
                world,
                &self.theme,
                tick,
            );
        }
        world.resource_mut::<Ui>().nodes.gone.clear();
        moving
    }

    /// Builds `view` as the panel `name` under `root`.
    pub(crate) fn mount(
        &mut self,
        world: &mut World,
        name: String,
        root: Node,
        motions: Motions,
        view: AnyView,
    ) {
        self.unmount(world, &name);
        let seconds = |seconds: f32| {
            Duration::try_from_secs_f32(seconds).unwrap_or_default()
        };
        self.theme = Theme {
            interact: seconds(motions.interact),
            expand: seconds(motions.expand),
        };
        let root = world.resource_mut::<Ui>().nodes.adopt(root);
        let mut mounted = Mounted::default();
        {
            let mut cx = Cx::new(world, &self.theme, &mut mounted);
            cx.under(root, |cx| view.build(cx));
        }
        self.mounted.insert(name, Shown { mounted, root });
    }

    /// Builds `view` as the card `name` under `root`, its frame, as
    /// the node `key`.
    pub(crate) fn mount_card(
        &mut self,
        world: &mut World,
        name: String,
        key: Key,
        root: Node,
        view: AnyView,
    ) {
        self.unmount(world, &name);
        world.resource_mut::<Ui>().nodes.adopt_as(key, root);
        let mut mounted = Mounted::default();
        {
            let mut cx = Cx::new(world, &self.theme, &mut mounted);
            cx.under(key, |cx| view.build(cx));
        }
        self.mounted.insert(name, Shown { mounted, root: key });
    }

    pub(crate) fn unmount(&mut self, world: &mut World, name: &str) {
        if let Some(shown) = self.mounted.remove(name) {
            world.resource_mut::<Ui>().despawn(shown.root);
        }
    }

    /// Takes in what the person did: states as the engine tells them,
    /// and the handlers of the rest.
    pub(crate) fn input(
        &mut self,
        world: &mut World,
        events: Vec<UiEvent>,
    ) {
        for event in events {
            let (states, on) = match event.action {
                Action::Hover(on) => (States::HOVER, on),
                Action::Press(on) => (States::PRESS, on),
                action => {
                    crate::ui::handle(world, event.key, &action);
                    continue;
                }
            };
            let mut ui = world.resource_mut::<Ui>();
            ui.nodes.enter(event.key, states, on);
            if states == States::HOVER {
                ui.point(event.key, on);
            }
            for shown in self.mounted.values_mut() {
                shown.mounted.mark_dirty(event.key);
            }
        }
    }
}

impl Shown {
    /// Drops the mounts of the nodes despawned since.
    fn forget_gone(&mut self, world: &World) {
        for &key in &world.resource::<Ui>().nodes.gone {
            self.mounted.unmount(key);
        }
    }
}
