//! What a plugin for the engine is written with. A plugin is a type
//! that implements [`Plugin`], exported with [`export!`] and built
//! for `wasm32-wasip2`. Its panels and settings pages are views of
//! [`ui`], written with fynix:
//!
//! ```ignore
//! #[derive(Default)]
//! struct Hello;
//!
//! #[derive(Default)]
//! struct Count(u32);
//!
//! impl hamlet_engine_sdk::Plugin for Hello {
//!     fn declare(&self, entry: &mut Entry) {
//!         entry.panel("hello", "Hello");
//!     }
//!
//!     fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
//!         world.init_resource::<Count>();
//!         column((
//!             label(resource::<Count, _>(|count| count.0.to_string())),
//!             button("Add one").on_click(|world| {
//!                 world.resource_mut::<Count>().0 += 1;
//!             }),
//!         ))
//!         .boxed()
//!     }
//! }
//!
//! hamlet_engine_sdk::export!(Hello);
//! ```
//!
//! The standard library has the clocks and randomness, so `Instant`,
//! `SystemTime` and `HashMap` work. What it prints goes to the log,
//! stdout as info and stderr as warnings, within the plugin's log
//! budgets. A sleep ends with the call's time. Files, the
//! environment, arguments and the network are not there: stdin is
//! empty, there are no folders and no sockets; a plugin reaches the
//! network with [`fetch`].

pub mod assets;
mod entry;
pub mod ui;

/// The bindings of `engine/wit/hamlet_engine.wit`.
pub mod bindings {
    wit_bindgen::generate!({
        path: "../wit/hamlet_engine.wit",
        world: "ui-plugin",
        pub_export_macro: true,
        export_macro_name: "export_guest",
    });
}

pub use hamlet_engine_api as api;
pub use hamlet_engine_api::{
    AgentsEvent, AgentsReply, AgentsRequest, Asset, AssetId,
    AssetInfo, Axis, DesktopEvent, DesktopReply, DesktopRequest,
    Failure, FetchReply, FetchRequest, Layout, Message, Method,
    PanelId, Reply, Ticket, decode, encode,
};

use self::bindings::hamlet::engine::host::{self, Level};
pub use self::entry::Entry;
pub use self::ui::World;

/// The version of the engine this SDK comes with.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A plugin. One value of it lives as long as the plugin runs. Each
/// call is given `world`, the world its views are built in and read
/// from, and what it changes there is shown once the call ends.
pub trait Plugin: Default + 'static {
    /// Declares what the plugin adds, each time it is switched on.
    fn declare(&self, entry: &mut Entry);

    /// The view of its panel or settings page `name`, built once each
    /// time it is shown, with `world` holding what its signals read.
    fn mount(
        &mut self,
        name: &str,
        world: &mut World,
    ) -> ui::AnyView {
        let _ = (name, world);
        fynix::ViewExt::boxed(ui::column(()))
    }

    /// Runs its command `name`.
    fn run(&mut self, name: &str, world: &mut World) {
        let _ = (name, world);
    }

    /// Its state, handed to its next version when it is reloaded.
    fn save(&self, world: &World) -> Vec<u8> {
        let _ = world;
        Vec::new()
    }

    /// Takes the state its earlier version saved.
    fn restore(&mut self, state: &[u8], world: &mut World) {
        let _ = (state, world);
    }

    /// A message from the engine: a reply, an asset held that changed
    /// or went, or an event.
    fn receive(&mut self, message: Message, world: &mut World) {
        let _ = (message, world);
    }

    /// Serves a call of its service `service` by the plugin `caller`,
    /// with `argument`: the result, or why it failed. The argument
    /// and the result are bytes of the types its manifest names.
    fn serve(
        &mut self,
        service: &str,
        caller: &str,
        argument: &[u8],
        world: &mut World,
    ) -> Result<Vec<u8>, String> {
        let _ = (caller, argument, world);
        Err(format!("there is no service {service}"))
    }
}

/// Calls the service `service` (`plugin.id/name`) of another plugin
/// with `argument`: its result comes as [`Reply::Answered`], bytes of
/// the type the manifest names.
pub fn call(service: &str, argument: &[u8]) -> Ticket {
    host::call(service, argument)
}

/// Subscribes to the event `event` (`plugin.id/name`) of another
/// plugin: [`Reply::Subscribed`], and then each comes as a
/// [`Message::Event`].
pub fn subscribe(event: &str) -> Ticket {
    host::subscribe(event)
}

/// Sends the plugin's own event `event` with `payload`, bytes of the
/// type the manifest names. A reply comes only when it is refused.
pub fn emit(event: &str, payload: &[u8]) -> Ticket {
    host::emit(event, payload)
}

/// Fetches `request` from a host the manifest asks `net.fetch` for:
/// [`Reply::Fetched`].
pub fn fetch(request: &FetchRequest) -> Ticket {
    encode(request).map_or(0, |bytes| host::fetch(&bytes))
}

/// Asks the agents service `request`, within the agents capabilities
/// the manifest asks for: [`Reply::Agents`].
pub fn agents(request: &AgentsRequest) -> Ticket {
    encode(request).map_or(0, |bytes| host::agents(&bytes))
}

/// Asks the desktop service `request`, within the desktop
/// capabilities the manifest asks for: [`Reply::Desktop`].
pub fn desktop(request: &DesktopRequest) -> Ticket {
    encode(request).map_or(0, |bytes| host::desktop(&bytes))
}

/// Writes `text` to the engine's log.
pub fn info(text: &str) {
    host::log(Level::Info, text);
}

pub fn warn(text: &str) {
    host::log(Level::Warn, text);
}

pub fn error(text: &str) {
    host::log(Level::Error, text);
}

/// Exports the type `$plugin`, which implements [`Plugin`], as the
/// plugin of this crate.
#[macro_export]
macro_rules! export {
    ($plugin:ty) => {
        const _: () = {
            ::std::thread_local! {
                static PLUGIN: ::core::cell::RefCell<$plugin> =
                    ::core::cell::RefCell::new(
                        <$plugin as ::core::default::Default>::default(),
                    );
            }

            struct Exports;

            impl $crate::bindings::exports::hamlet::engine::guest::Guest
                for Exports
            {
                fn declare() -> ::std::vec::Vec<u8> {
                    PLUGIN.with_borrow(|plugin| $crate::glue::declare(plugin))
                }

                fn run(name: ::std::string::String) {
                    PLUGIN.with_borrow_mut(|plugin| {
                        $crate::glue::run(plugin, &name)
                    })
                }

                fn save() -> ::std::vec::Vec<u8> {
                    PLUGIN.with_borrow(|plugin| $crate::glue::save(plugin))
                }

                fn restore(state: ::std::vec::Vec<u8>) {
                    PLUGIN.with_borrow_mut(|plugin| {
                        $crate::glue::restore(plugin, &state)
                    })
                }

                fn receive(messages: ::std::vec::Vec<u8>) {
                    PLUGIN.with_borrow_mut(|plugin| {
                        $crate::glue::receive(plugin, &messages)
                    })
                }

                fn serve(
                    service: ::std::string::String,
                    caller: ::std::string::String,
                    argument: ::std::vec::Vec<u8>,
                ) -> ::core::result::Result<
                    ::std::vec::Vec<u8>,
                    ::std::string::String,
                > {
                    PLUGIN.with_borrow_mut(|plugin| {
                        $crate::glue::serve(
                            plugin, &service, &caller, &argument,
                        )
                    })
                }
            }

            impl $crate::bindings::exports::hamlet::engine::ui_guest::Guest
                for Exports
            {
                fn mount(
                    name: ::std::string::String,
                    root: $crate::bindings::hamlet::engine::ui::Node,
                    motions: $crate::bindings::exports::hamlet::engine::ui_guest::Motions,
                ) -> bool {
                    PLUGIN.with_borrow_mut(|plugin| {
                        $crate::ui::glue::mount(plugin, name, root, motions)
                    })
                }

                fn unmount(name: ::std::string::String) {
                    $crate::ui::glue::unmount(&name)
                }

                fn input(
                    events: ::std::vec::Vec<
                        $crate::bindings::exports::hamlet::engine::ui_guest::UiEvent,
                    >,
                ) -> bool {
                    $crate::ui::glue::input(events)
                }

                fn frame(micros: u32) -> bool {
                    $crate::ui::glue::frame(micros)
                }
            }

            $crate::bindings::export_guest!(
                Exports with_types_in $crate::bindings
            );
        };
    };
}

/// What [`export!`] calls: the exports, between the wire format and
/// a [`Plugin`], each with the world of its views.
#[doc(hidden)]
pub mod glue {
    use hamlet_engine_api::{Message, decode, encode};

    use crate::ui::glue::with_world;
    use crate::{Entry, Plugin};

    pub fn declare<P: Plugin>(plugin: &P) -> Vec<u8> {
        let mut entry = Entry::default();
        plugin.declare(&mut entry);
        encode(&entry.finish()).unwrap_or_else(|why| {
            crate::error(&why.to_string());
            Vec::new()
        })
    }

    pub fn run<P: Plugin>(plugin: &mut P, name: &str) {
        with_world(|world| plugin.run(name, world));
    }

    pub fn save<P: Plugin>(plugin: &P) -> Vec<u8> {
        with_world(|world| plugin.save(world))
    }

    pub fn restore<P: Plugin>(plugin: &mut P, state: &[u8]) {
        with_world(|world| plugin.restore(state, world));
    }

    pub fn receive<P: Plugin>(plugin: &mut P, messages: &[u8]) {
        match decode::<Vec<Message>>(messages) {
            Ok(messages) => with_world(|world| {
                for message in messages {
                    plugin.receive(message, world);
                }
            }),
            Err(why) => crate::error(&format!("messages: {why}")),
        }
    }

    pub fn serve<P: Plugin>(
        plugin: &mut P,
        service: &str,
        caller: &str,
        argument: &[u8],
    ) -> Result<Vec<u8>, String> {
        with_world(|world| {
            plugin.serve(service, caller, argument, world)
        })
    }
}
