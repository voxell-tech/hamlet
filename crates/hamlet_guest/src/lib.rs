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

pub mod storage;
pub mod ui;

pub use self::bindings::exports::hamlet::plugin::plugin::{
    Incoming, Intent, Outgoing, Tree, View as Wired,
};
pub use self::bindings::hamlet::plugin::host::{
    Agent, Level, RequestResult,
};

/// What the host calls on a plugin.
pub trait Plugin {
    /// Runs once when the plugin starts.
    fn entry();

    /// Runs with what happened since the last call, and answers with
    /// what the plugin wants done.
    fn step(inbox: Vec<Incoming>) -> Vec<Intent>;

    /// The panel `panel`, as it is now. The host asks again only
    /// after an event reached the panel or the plugin asked for a
    /// redraw.
    fn panel_view(_panel: &str) -> ui::View {
        ui::column([]).into()
    }
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

/// The living agents, each with its hamlet. It needs the capability
/// `read.agents`, and is an error without it.
pub fn agents() -> Result<Vec<Agent>, String> {
    bindings::hamlet::plugin::host::agents()
}

/// Asks to use a capability the manifest names, with a reason. On
/// [`RequestResult::Pending`] the chief has been asked, and the
/// answer reaches a later `step` as `Incoming::Granted` or
/// `Incoming::Denied`.
pub fn request(capability: &str, reason: &str) -> RequestResult {
    bindings::hamlet::plugin::host::request(capability, reason)
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

                fn panel_view(panel: String) -> $crate::Wired {
                    <$plugin as $crate::Plugin>::panel_view(&panel)
                        .into_wire()
                }
            }

            $crate::bindings::export!(Exported);
        };
    };
}
