//! A plugin for the engine written as a Bevy plugin. Each plugin runs
//! one Bevy [`App`] in its sandbox, with no rendering and no threads,
//! and [`export!`] drives it through the WIT:
//!
//! ```ignore
//! #[derive(Default)]
//! struct Hello;
//!
//! #[derive(Resource, Default)]
//! struct Count(u32);
//!
//! impl Plugin for Hello {
//!     fn build(&self, app: &mut App) {
//!         app.init_resource::<Count>()
//!             .add_panel("hello", "Hello", hello);
//!     }
//! }
//!
//! fn hello() -> AnyView {
//!     column((
//!         label(resource::<Count, _>(|count| count.0.to_string())),
//!         button("Add one").on_click(|world| {
//!             world.resource_mut::<Count>().0 += 1;
//!         }),
//!     ))
//!     .boxed()
//! }
//!
//! hamlet_engine_bevy::export!(Hello);
//! ```

mod app;
#[doc(hidden)]
pub mod glue;
mod host;
pub mod scene;
pub mod ui;

/// The bindings of `engine/wit/hamlet_engine.wit`.
pub mod bindings {
    wit_bindgen::generate!({
        path: "../wit/hamlet_engine.wit",
        world: "render-plugin",
        pub_export_macro: true,
        export_macro_name: "export_guest",
    });
}

pub use bevy_app;
pub use bevy_ecs;
pub use bevy_math;
pub use bevy_time;
pub use bevy_transform;
pub use hamlet_engine_api as api;
pub use hamlet_engine_api::{
    AgentsEvent, AgentsReply, AgentsRequest, Asset, AssetId,
    AssetInfo, Axis, Contribution, DesktopEvent, DesktopReply,
    DesktopRequest, Failure, FetchReply, FetchRequest, Layout, Reply,
    Ticket, decode, encode,
};
pub use serde;

pub use crate::app::{
    Declare, HamletAppExt, RunCommand, ServiceCall,
};
use crate::bindings::hamlet::engine::host::{self as wit, Level};
pub use crate::host::{
    AgentsEventReceived, AssetChanged, AssetGone,
    DesktopEventReceived, EventReceived, HostExt, Replied,
};

/// What a plugin is usually written with.
pub mod prelude {
    // The derives of `Component`, `Resource`, `Event` and
    // `Message` name `bevy_ecs`, found here by a plugin that
    // has only the SDK.
    pub use ::bevy_ecs;
    pub use bevy_app::prelude::*;
    pub use bevy_ecs::prelude::*;
    pub use bevy_math::prelude::*;
    pub use bevy_time::Time;
    pub use bevy_transform::components::Transform;

    pub use crate::scene::{
        Animate, ForwardDecal, Handle, Instances, Mesh3d,
        MeshMaterial3d, Pickable, PostPass, SceneAssets, SceneCamera,
        SceneClick, SceneKey, SceneLabel, SceneLight, SceneOut,
        SceneOver, ScenePointer, ScenePress, SceneResize, SceneWheel,
        Shadows,
    };
    pub use crate::ui::{AnyView, ViewExt};
    pub use crate::{
        AgentsEventReceived, AssetChanged, AssetGone, Declare,
        DesktopEventReceived, EventReceived, HamletAppExt, HostExt,
        Replied, RunCommand, ServiceCall,
    };
}

/// The version of the engine this SDK comes with.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Writes `text` to the engine's log.
pub fn info(text: &str) {
    wit::log(Level::Info, text);
}

pub fn warn(text: &str) {
    wit::log(Level::Warn, text);
}

pub fn error(text: &str) {
    wit::log(Level::Error, text);
}

/// Exports the Bevy plugin `$plugin`, a `Plugin + Default`, as the
/// plugin of this crate.
#[macro_export]
macro_rules! export {
    ($plugin:ty) => {
        const _: () = {
            use $crate::bindings::exports::hamlet::engine::{
                guest, scene_guest, ui_guest,
            };

            struct Exports;

            impl guest::Guest for Exports {
                fn declare() -> ::std::vec::Vec<u8> {
                    $crate::glue::declare::<$plugin>()
                }

                fn run(name: ::std::string::String) {
                    $crate::glue::run::<$plugin>(&name)
                }

                fn save() -> ::std::vec::Vec<u8> {
                    $crate::glue::save::<$plugin>()
                }

                fn restore(state: ::std::vec::Vec<u8>) {
                    $crate::glue::restore::<$plugin>(&state)
                }

                fn receive(messages: ::std::vec::Vec<u8>) {
                    $crate::glue::receive::<$plugin>(&messages)
                }

                fn serve(
                    service: ::std::string::String,
                    caller: ::std::string::String,
                    argument: ::std::vec::Vec<u8>,
                ) -> ::core::result::Result<
                    ::std::vec::Vec<u8>,
                    ::std::string::String,
                > {
                    $crate::glue::serve::<$plugin>(
                        service, caller, argument,
                    )
                }
            }

            impl ui_guest::Guest for Exports {
                fn mount(
                    name: ::std::string::String,
                    root: $crate::bindings::hamlet::engine::ui::Node,
                    motions: ui_guest::Motions,
                ) -> bool {
                    $crate::glue::mount::<$plugin>(
                        name, root, motions,
                    )
                }

                fn unmount(name: ::std::string::String) {
                    $crate::glue::unmount::<$plugin>(&name)
                }

                fn input(
                    events: ::std::vec::Vec<ui_guest::UiEvent>,
                ) -> bool {
                    $crate::glue::input::<$plugin>(events)
                }

                fn frame(micros: u32) -> bool {
                    $crate::glue::frame::<$plugin>(micros)
                }
            }

            impl scene_guest::Guest for Exports {
                fn input(
                    inputs: ::std::vec::Vec<scene_guest::SceneInput>,
                ) {
                    $crate::glue::scene::<$plugin>(inputs)
                }

                fn tick(micros: u32) {
                    $crate::glue::tick::<$plugin>(micros)
                }
            }

            $crate::bindings::export_guest!(
                Exports with_types_in $crate::bindings
            );
        };
    };
}
