//! What a plugin for the engine is written with. A plugin is a type
//! that implements [`Plugin`], exported with [`export!`] and built
//! for `wasm32-wasip2`:
//!
//! ```ignore
//! #[derive(Default)]
//! struct Hello {
//!     count: u32,
//! }
//!
//! impl hamlet_engine_sdk::Plugin for Hello {
//!     fn declare(&self, entry: &mut Entry) {
//!         entry.panel("hello", "Hello");
//!     }
//!
//!     fn draw(&self, _: &str, ui: &mut Ui) {
//!         ui.text("count", format!("{}", self.count))
//!             .button("add", "Add one");
//!     }
//!
//!     fn event(&mut self, _: WidgetEvent) {
//!         self.count += 1;
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
mod tree;
#[cfg(feature = "ui")]
pub mod ui;

/// The bindings of `engine/wit/hamlet_engine.wit`.
#[cfg(not(feature = "ui"))]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../wit/hamlet_engine.wit",
        world: "plugin",
        pub_export_macro: true,
        export_macro_name: "export_guest",
    });
}

/// The bindings of `engine/wit/hamlet_engine.wit`, with retained
/// panels.
#[cfg(feature = "ui")]
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
    Action, Asset, AssetId, AssetInfo, Axis, Failure, FetchReply,
    FetchRequest, Layout, Message, Method, PanelId, Reply, Ticket,
    WidgetEvent, WidgetId, decode, encode,
};

use self::bindings::hamlet::engine::host::{self, Level};
pub use self::entry::Entry;
pub use self::tree::Ui;

/// A plugin. One value of it lives as long as the plugin runs.
pub trait Plugin: Default + 'static {
    /// Declares what the plugin adds, each time it is switched on.
    fn declare(&self, entry: &mut Entry);

    /// Draws its panel or settings page `name`.
    #[cfg(not(feature = "ui"))]
    fn draw(&self, name: &str, ui: &mut Ui);

    /// Draws its settings page `name`; its panels are retained.
    #[cfg(feature = "ui")]
    fn draw(&self, name: &str, ui: &mut Ui) {
        let _ = (name, ui);
    }

    /// The view of its panel `name`, built once each time the panel
    /// is shown, with `world` holding what its signals read.
    #[cfg(feature = "ui")]
    fn mount(
        &mut self,
        name: &str,
        world: &mut ui::World,
    ) -> ui::AnyView;

    /// What the person did on one of its panels. The panel is drawn
    /// again after it.
    fn event(&mut self, event: WidgetEvent) {
        let _ = event;
    }

    /// Runs its command `name`. Its panels are drawn again after it.
    fn run(&mut self, name: &str) {
        let _ = name;
    }

    /// Its state, handed to its next version when it is reloaded.
    fn save(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Takes the state its earlier version saved.
    fn restore(&mut self, state: &[u8]) {
        let _ = state;
    }

    /// A message from the engine. Its panels are drawn again after
    /// each batch of them.
    fn receive(&mut self, message: Message) {
        let _ = message;
    }

    /// Serves a call of its service `service` by the plugin `caller`,
    /// with `argument`: the result, or why it failed. The argument
    /// and the result are bytes of the types its manifest names.
    fn serve(
        &mut self,
        service: &str,
        caller: &str,
        argument: &[u8],
    ) -> Result<Vec<u8>, String> {
        let _ = (caller, argument);
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

/// Asks for the panel or settings page `name` to be drawn again.
pub fn redraw(name: &str) {
    host::redraw(name);
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

                fn draw(name: ::std::string::String) -> ::std::vec::Vec<u8> {
                    PLUGIN.with_borrow(|plugin| $crate::glue::draw(plugin, &name))
                }

                fn event(event: ::std::vec::Vec<u8>) {
                    PLUGIN.with_borrow_mut(|plugin| {
                        $crate::glue::event(plugin, &event)
                    })
                }

                fn run(name: ::std::string::String) {
                    PLUGIN.with_borrow_mut(|plugin| {
                        <$plugin as $crate::Plugin>::run(plugin, &name)
                    })
                }

                fn save() -> ::std::vec::Vec<u8> {
                    PLUGIN.with_borrow(|plugin| {
                        <$plugin as $crate::Plugin>::save(plugin)
                    })
                }

                fn restore(state: ::std::vec::Vec<u8>) {
                    PLUGIN.with_borrow_mut(|plugin| {
                        <$plugin as $crate::Plugin>::restore(plugin, &state)
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
                        <$plugin as $crate::Plugin>::serve(
                            plugin, &service, &caller, &argument,
                        )
                    })
                }
            }

            $crate::ui_exports!(Exports, $plugin);

            $crate::bindings::export_guest!(
                Exports with_types_in $crate::bindings
            );
        };
    };
}

/// The exports of retained panels, for [`export!`].
#[cfg(feature = "ui")]
#[doc(hidden)]
#[macro_export]
macro_rules! ui_exports {
    ($exports:ident, $plugin:ty) => {
        impl $crate::bindings::exports::hamlet::engine::ui_guest::Guest
            for $exports
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
    };
}

#[cfg(not(feature = "ui"))]
#[doc(hidden)]
#[macro_export]
macro_rules! ui_exports {
    ($exports:ident, $plugin:ty) => {};
}

/// What [`export!`] calls: the exports, between the wire format and
/// a [`Plugin`].
#[doc(hidden)]
pub mod glue {
    use hamlet_engine_api::{Message, WidgetEvent, decode, encode};
    use serde::Serialize;

    use crate::{Entry, Plugin, Ui};

    pub fn declare<P: Plugin>(plugin: &P) -> Vec<u8> {
        let mut entry = Entry::default();
        plugin.declare(&mut entry);
        bytes(&entry.finish())
    }

    pub fn draw<P: Plugin>(plugin: &P, name: &str) -> Vec<u8> {
        let mut ui = Ui::default();
        plugin.draw(name, &mut ui);
        bytes(&ui.finish())
    }

    pub fn event<P: Plugin>(plugin: &mut P, event: &[u8]) {
        match decode::<WidgetEvent>(event) {
            Ok(event) => plugin.event(event),
            Err(why) => crate::error(&format!("an event: {why}")),
        }
    }

    pub fn receive<P: Plugin>(plugin: &mut P, messages: &[u8]) {
        match decode::<Vec<Message>>(messages) {
            Ok(messages) => {
                for message in messages {
                    plugin.receive(message);
                }
            }
            Err(why) => crate::error(&format!("messages: {why}")),
        }
    }

    fn bytes(value: &impl Serialize) -> Vec<u8> {
        encode(value).unwrap_or_else(|why| {
            crate::error(&why.to_string());
            Vec::new()
        })
    }
}
