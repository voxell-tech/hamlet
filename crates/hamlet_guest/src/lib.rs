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

pub mod calls;
pub mod collection;
pub mod events;
pub mod state;
pub mod storage;
pub mod ui;

pub use self::bindings::exports::hamlet::plugin::plugin::{
    CallAnswer, CallRequest, CommandCall, Incoming, Intent, Outgoing,
    Publication, Published, Tree, View as Wired,
};
pub use self::bindings::hamlet::plugin::host::{
    Agent, AgentPapers, Level, RequestResult,
};
pub use self::calls::Calls;
pub use self::state::State;

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

/// Declares a command the plugin exports, which other plugins and the
/// chief run by `id` once the manifest lists it under
/// `exports.commands`. Only `entry` may call it. `title` is what a
/// menu or a key shows, and `icon` optionally names one of the
/// panels' icons.
pub fn command(id: &str, title: &str, icon: Option<&str>) {
    bindings::hamlet::plugin::host::command(id, title, icon);
}

/// Adds a line to a menu of the top bar, `hamlet` or `view`, that
/// runs the plugin's command `command`. It needs the capability
/// `ui.menu`. Only `entry` may call it.
pub fn menu_item(
    menu: &str,
    section: u32,
    label: &str,
    command: &str,
) {
    bindings::hamlet::plugin::host::menu_item(
        menu, section, label, command,
    );
}

/// Binds a shortcut such as `Cmd+Shift+N` to the plugin's command
/// `command`. It needs the capability `ui.keys`. Only `entry` may
/// call it.
pub fn key(chord: &str, command: &str) {
    bindings::hamlet::plugin::host::key(chord, command);
}

/// The intent that brings the plugin's own panel `panel` to the front
/// of the dock, opening it if it is closed. It needs no capability.
/// To be returned from `step`.
pub fn open_panel(panel: &str) -> Intent {
    Intent::OpenPanel(panel.into())
}

/// The living agents, each with its hamlet. It needs the capability
/// `read.agents`, and is an error without it.
pub fn agents() -> Result<Vec<Agent>, String> {
    bindings::hamlet::plugin::host::agents()
}

/// The selected agent's notes, character and the names of its
/// skills, empty while no agent is selected. It needs the capability
/// `read.papers`, and is an error without it.
pub fn papers() -> Result<AgentPapers, String> {
    bindings::hamlet::plugin::host::papers()
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
