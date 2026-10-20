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
//! impl hamlet_plugin::Plugin for Hello {
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
//! hamlet_plugin::export!(Hello);
//! ```
//!
//! The standard library's calls to the system, such as the clock,
//! randomness or printing, stop the plugin: a `HashMap` needs
//! randomness, so a plugin keeps a `BTreeMap` instead.

pub mod assets;
mod entry;
mod ui;

/// The bindings of `engine/wit/hamlet_engine.wit`.
pub mod bindings {
    wit_bindgen::generate!({
        path: "../wit/hamlet_engine.wit",
        world: "plugin",
        pub_export_macro: true,
        export_macro_name: "export_guest",
    });
}

pub use hamlet_engine_api as api;
pub use hamlet_engine_api::{
    Action, Asset, AssetId, AssetInfo, Axis, Failure, Layout,
    Message, PanelId, Reply, Ticket, WidgetEvent, WidgetId,
};

use self::bindings::hamlet::engine::host::{self, Level};
pub use self::entry::Entry;
pub use self::ui::Ui;

/// A plugin. One value of it lives as long as the plugin runs.
pub trait Plugin: Default + 'static {
    /// Declares what the plugin adds, each time it is switched on.
    fn declare(&self, entry: &mut Entry);

    /// Draws its panel or settings page `name`.
    fn draw(&self, name: &str, ui: &mut Ui);

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
            }

            $crate::bindings::export_guest!(
                Exports with_types_in $crate::bindings
            );
        };
    };
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
