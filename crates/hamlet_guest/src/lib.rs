//! What a plugin built for WASM is written with. A plugin implements
//! [`Plugin`], exports it with [`export!`] and builds as a `cdylib`
//! for `wasm32-wasip2`.

#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../hamlet_wasm/wit/hamlet-plugin.wit",
        world: "hamlet",
        pub_export_macro: true,
        default_bindings_module: "hamlet_guest::bindings",
    });
}

pub use self::bindings::exports::hamlet::plugin::plugin::{
    Incoming, Intent, Outgoing,
};
pub use self::bindings::hamlet::plugin::host::Level;

/// What the host calls on a plugin.
pub trait Plugin {
    /// Runs once when the plugin starts.
    fn entry();

    /// Runs with what happened since the last call, and answers with
    /// what the plugin wants done.
    fn step(inbox: Vec<Incoming>) -> Vec<Intent>;
}

/// Writes a line to the host's log.
pub fn log(level: Level, message: &str) {
    bindings::hamlet::plugin::host::log(level, message);
}

/// Adds a panel to the dock. Only `entry` may call it. `icon` is the
/// name of one of the panels' icons.
pub fn panel(id: &str, title: &str, icon: &str) {
    bindings::hamlet::plugin::host::panel(id, title, icon);
}

/// Exports a [`Plugin`] as the component's `plugin` interface.
#[macro_export]
macro_rules! export {
    ($plugin:ty) => {
        const _: () = {
            struct Exported;

            impl $crate::bindings::exports::hamlet::plugin::plugin::Guest
                for Exported
            {
                fn entry() {
                    <$plugin as $crate::Plugin>::entry();
                }

                fn step(inbox: Vec<$crate::Incoming>) -> $crate::Outgoing {
                    $crate::Outgoing {
                        intents: <$plugin as $crate::Plugin>::step(inbox),
                    }
                }
            }

            $crate::bindings::export!(Exported);
        };
    };
}
