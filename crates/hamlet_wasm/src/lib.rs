//! The host of the installed plugins: WASM components run under
//! limits, one store each.

use bevy::prelude::*;
use hamlet_assets::TuningPlugin;
use hamlet_openui::Drawing;
use hamlet_sdk::agents::Kept;
use hamlet_sdk::papers::Papers;

use self::engine::Host;
use self::limits::{Limits, WasmTuning};

pub mod admit;
pub mod cache;
pub mod call;
pub mod draw;
pub mod enable;
pub mod engine;
pub mod event;
pub mod fault;
pub mod host;
pub mod inbox;
pub mod install;
pub mod instance;
pub mod interface;
pub mod limits;
pub mod load;
pub mod manifest;
pub mod panel;
pub mod reload;
pub mod reply;
pub mod run;
pub mod start;
pub mod stop;
pub mod storage;
pub mod uninstall;
pub mod wire;

#[cfg(any(test, feature = "fixture"))]
pub mod fixture;
#[cfg(test)]
mod tests;

/// Loads the installed plugins, steps the ones that have
/// mail and draws their panels. Add it after the core and the SDK.
pub fn plugin(app: &mut App) {
    app.add_plugins((
        TuningPlugin::<WasmTuning>::default(),
        hamlet_openui::plugin,
        reload::plugin,
    ))
    .init_resource::<Limits>()
    .init_resource::<Papers>()
    .add_message::<run::Done>()
    .add_message::<run::Said>()
    .add_observer(admit::admit)
    .add_observer(enable::restart)
    .add_observer(event::deliver)
    .add_observer(reply::reply)
    .add_observer(stop::stopped)
    .add_observer(uninstall::removed)
    .configure_sets(
        Update,
        (
            PluginSet::Collect,
            PluginSet::Step,
            PluginSet::Apply,
            PluginSet::Draw,
        )
            .chain(),
    )
    .configure_sets(
        Update,
        (
            Kept.before(PluginSet::Step),
            Drawing.after(PluginSet::Draw),
        ),
    )
    .add_systems(Startup, load::installed)
    .add_systems(
        Update,
        (
            inbox::tick.in_set(PluginSet::Collect),
            run::step.in_set(PluginSet::Step).run_if(inbox::has_mail),
            run::apply
                .in_set(PluginSet::Apply)
                .run_if(on_message::<run::Done>),
            draw::draw.in_set(PluginSet::Draw).run_if(draw::due),
        ),
    );
    match Host::new(&Limits::default()) {
        Ok(host) => {
            app.insert_resource(host);
        }
        Err(e) => error!("wasm: no engine: {e}"),
    }
}

/// The parts of a frame of the installed plugins, in order.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PluginSet {
    /// What happened is put in the inboxes.
    Collect,
    /// The plugins with mail are called.
    Step,
    /// What the plugins asked for is carried out.
    Apply,
    /// The panels that are due are drawn.
    Draw,
}
