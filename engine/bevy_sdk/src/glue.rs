//! What [`export!`](crate::export) calls: the WIT's exports, each
//! driving the one [`App`] of the plugin and bringing its panels in
//! step with the world after it.

use core::cell::RefCell;
use core::time::Duration;

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_time::Time;
use hamlet_engine_api::{Message as Mail, decode, encode};

use crate::app::{
    CommandSystems, Declare, Declared, RunCommand, SavedResources,
    ServiceCall, Services, Views,
};
use crate::bindings::exports::hamlet::engine::scene_guest::SceneInput;
use crate::bindings::exports::hamlet::engine::ui_guest::{
    Motions, UiEvent,
};
use crate::bindings::hamlet::engine::ui::Node;
use crate::scene::Animate;
use crate::ui::panels::Panels;
use crate::ui::{AnyView, ViewExt, column};

std::thread_local! {
    // The guest is called on one thread, one export at a time.
    static GUEST: RefCell<Option<Guest>> = const { RefCell::new(None) };
}

/// The plugin's app, and its panels beside its world.
struct Guest {
    app: App,
    panels: Panels,
    declared: bool,
    updated: bool,
}

impl Guest {
    fn new<P: Plugin + Default>() -> Self {
        let mut app = App::new();
        app.init_resource::<Time>().add_plugins((
            crate::app::plugin,
            crate::host::plugin,
            crate::ui::plugin,
            crate::scene::plugin,
            crate::audio::plugin,
        ));
        app.add_plugins(P::default());
        app.finish();
        app.cleanup();
        Self {
            app,
            panels: Panels::default(),
            declared: false,
            updated: false,
        }
    }

    /// Runs the app's schedules with `delta` passed, and brings the
    /// panels up to date, their transitions moved on by `moved`:
    /// whether something still moves.
    fn update(&mut self, delta: Duration, moved: Duration) -> bool {
        let world = self.app.world_mut();
        world.resource_mut::<Time>().advance_by(delta);
        self.app.update();
        self.updated = true;
        self.panels.update(self.app.world_mut(), moved)
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }
}

/// Runs `call` with the plugin's guest, made first if there is none.
fn with<P: Plugin + Default, R>(
    call: impl FnOnce(&mut Guest) -> R,
) -> R {
    GUEST.with_borrow_mut(|guest| {
        call(guest.get_or_insert_with(Guest::new::<P>))
    })
}

pub fn declare<P: Plugin + Default>() -> Vec<u8> {
    with::<P, _>(|guest| {
        let declared =
            encode(&guest.world().resource::<Declared>().0)
                .unwrap_or_else(|why| {
                    crate::error(&why.to_string());
                    Vec::new()
                });
        if !guest.declared {
            guest.declared = true;
            guest.world().run_schedule(Declare);
        }
        declared
    })
}

pub fn run<P: Plugin + Default>(name: &str) {
    with::<P, _>(|guest| {
        let world = guest.world();
        world.trigger(RunCommand { name: name.into() });
        let id =
            world.resource::<CommandSystems>().0.get(name).copied();
        if let Some(id) = id
            && let Err(why) = world.run_system(id)
        {
            crate::error(&format!("{name}: {why}"));
        }
        guest.update(Duration::ZERO, Duration::ZERO);
    });
}

pub fn save<P: Plugin + Default>() -> Vec<u8> {
    with::<P, _>(|guest| SavedResources::save(guest.world()))
}

pub fn restore<P: Plugin + Default>(state: &[u8]) {
    with::<P, _>(|guest| {
        SavedResources::restore(guest.world(), state)
    });
}

pub fn receive<P: Plugin + Default>(messages: &[u8]) {
    with::<P, _>(|guest| {
        match decode::<Vec<Mail>>(messages) {
            Ok(messages) => {
                for mail in messages {
                    crate::host::deliver(guest.world(), mail);
                }
            }
            Err(why) => crate::error(&format!("messages: {why}")),
        }
        guest.update(Duration::ZERO, Duration::ZERO);
    });
}

pub fn serve<P: Plugin + Default>(
    service: String,
    caller: String,
    argument: Vec<u8>,
) -> Result<Vec<u8>, String> {
    with::<P, _>(|guest| {
        let world = guest.world();
        let id = world
            .resource::<Services>()
            .0
            .get(&service)
            .copied()
            .ok_or_else(|| {
                format!("there is no service {service}")
            })?;
        let call = ServiceCall {
            service,
            caller,
            argument,
        };
        let served = world
            .run_system_with(id, call)
            .map_err(|why| why.to_string())
            .flatten();
        guest.update(Duration::ZERO, Duration::ZERO);
        served
    })
}

pub fn mount<P: Plugin + Default>(
    name: String,
    root: Node,
    motions: Motions,
) -> bool {
    with::<P, _>(|guest| {
        // What the view reads is made as the app starts.
        if !guest.updated {
            guest.update(Duration::ZERO, Duration::ZERO);
        }
        let world = guest.world();
        let view = world.resource::<Views>().0.get(&name).copied();
        let view = match view.map(|id| world.run_system(id)) {
            Some(Ok(view)) => view,
            Some(Err(why)) => {
                crate::error(&format!("{name}: {why}"));
                empty()
            }
            None => empty(),
        };
        guest.panels.mount(
            guest.app.world_mut(),
            name,
            root,
            motions,
            view,
        );
        guest.update(Duration::ZERO, Duration::ZERO)
    })
}

pub fn unmount<P: Plugin + Default>(name: &str) {
    with::<P, _>(|guest| {
        guest.panels.unmount(guest.app.world_mut(), name);
    });
}

pub fn input<P: Plugin + Default>(events: Vec<UiEvent>) -> bool {
    with::<P, _>(|guest| {
        guest.panels.input(guest.app.world_mut(), events);
        guest.update(Duration::ZERO, Duration::ZERO)
    })
}

pub fn frame<P: Plugin + Default>(micros: u32) -> bool {
    with::<P, _>(|guest| {
        let delta = Duration::from_micros(micros.into());
        // While it animates, `tick` moves time on in the same frame.
        let time = if guest.world().resource::<Animate>().0 {
            Duration::ZERO
        } else {
            delta
        };
        guest.update(time, delta)
    })
}

pub fn scene<P: Plugin + Default>(inputs: Vec<SceneInput>) {
    with::<P, _>(|guest| {
        for input in inputs {
            crate::scene::deliver(guest.world(), input);
        }
        guest.update(Duration::ZERO, Duration::ZERO);
    });
}

pub fn tick<P: Plugin + Default>(micros: u32) {
    with::<P, _>(|guest| {
        guest.update(
            Duration::from_micros(micros.into()),
            Duration::ZERO,
        );
    });
}

fn empty() -> AnyView {
    column(()).boxed()
}
