//! The host of the installed plugins: WASM components run under
//! limits, one store each.

use bevy::prelude::*;
use hamlet_assets::TuningPlugin;
use hamlet_openui::Drawing;
use hamlet_realm_api::overlay::Overlays;
use hamlet_realm_api::survey::{Survey, SurveyWanted};
use hamlet_sdk::agents::Kept;
use hamlet_sdk::papers::Papers;

use self::engine::Host;
use self::limits::{Limits, WasmTuning};

pub mod acts;
pub mod add;
pub mod admit;
pub mod budget;
pub mod cache;
pub mod call;
pub mod command;
pub mod compile;
pub mod db;
pub mod draw;
pub mod enable;
pub mod engine;
pub mod event;
pub mod fault;
pub mod files;
pub mod folder;
pub mod host;
pub mod inbox;
pub mod install;
pub mod instance;
pub mod interface;
pub mod interop;
pub mod limits;
pub mod load;
pub mod manifest;
pub mod model;
pub mod net;
pub mod overlay;
pub mod panel;
pub mod pick;
pub mod rate;
pub mod reach;
pub mod reload;
pub mod reply;
pub mod run;
pub mod scene;
pub mod shader;
pub mod start;
pub mod steer;
pub mod stop;
pub mod storage;
pub mod survey;
pub mod uninstall;
pub mod warned;
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
        add::plugin,
        budget::plugin,
        compile::plugin,
        interop::plugin,
        reload::plugin,
        scene::plugin,
    ))
    .init_resource::<Limits>()
    .init_resource::<Papers>()
    .init_resource::<Survey>()
    .init_resource::<SurveyWanted>()
    .init_resource::<Overlays>()
    .init_resource::<overlay::Redrawn>()
    .init_resource::<pick::Picking>()
    .init_resource::<acts::Acted>()
    .init_resource::<net::Fetching>()
    .init_resource::<folder::Folders>()
    .init_resource::<model::Calling>()
    .init_resource::<steer::Points>()
    .add_message::<run::Done>()
    .add_message::<run::Said>()
    .add_observer(admit::admit)
    .add_observer(load::appeared)
    .add_observer(enable::restart)
    .add_observer(event::deliver)
    .add_observer(folder::answered)
    .add_observer(reply::reply)
    .add_observer(steer::steer)
    .add_observer(stop::stopped)
    .add_observer(stop::schedule)
    .add_observer(stop::release)
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
    .add_systems(
        Startup,
        load::installed.after(hamlet_sdk::capability::open_store),
    )
    .add_systems(
        Update,
        (
            inbox::tick.in_set(PluginSet::Collect),
            command::follow.in_set(PluginSet::Collect),
            draw::regrant.in_set(PluginSet::Collect),
            survey::want.in_set(PluginSet::Collect),
            stop::retry
                .in_set(PluginSet::Collect)
                .run_if(resource_exists::<Time>),
            net::finish
                .in_set(PluginSet::Collect)
                .run_if(net::Fetching::any),
            net::start
                .in_set(PluginSet::Apply)
                .run_if(on_message::<run::Done>),
            run::step.in_set(PluginSet::Step).run_if(inbox::has_mail),
            run::apply
                .in_set(PluginSet::Apply)
                .run_if(on_message::<run::Done>),
            acts::carry_out
                .in_set(PluginSet::Apply)
                .run_if(on_message::<run::Done>),
            overlay::take
                .in_set(PluginSet::Apply)
                .run_if(on_message::<run::Done>),
            overlay::prune
                .in_set(PluginSet::Collect)
                .run_if(overlay::any),
            pick::take
                .in_set(PluginSet::Collect)
                .run_if(pick::waiting),
            (
                pick::ask.run_if(on_message::<run::Done>),
                pick::open.run_if(pick::waits),
            )
                .chain()
                .in_set(PluginSet::Apply),
            folder::start
                .in_set(PluginSet::Apply)
                .run_if(on_message::<run::Done>),
            folder::finish
                .in_set(PluginSet::Collect)
                .run_if(folder::Folders::any),
            model::start
                .in_set(PluginSet::Apply)
                .run_if(on_message::<run::Done>),
            model::finish
                .in_set(PluginSet::Collect)
                .run_if(model::Calling::any),
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
