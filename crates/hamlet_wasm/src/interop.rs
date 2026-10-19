//! Plugins working together. The host stands in the middle of every
//! meeting: a plugin never holds another's memory or storage, and
//! what crosses is a copy checked against the manifests and the
//! grants.

mod calls;
mod chief;
mod command;
mod hosted;
mod invoke;
mod mail;
mod publish;
mod route;
mod settle;
mod tools;

use bevy::prelude::*;
use hamlet_sdk::agents::{AgentList, SelectedAgent};

pub use self::calls::{Call, Calls};
use self::invoke::Invoking;
use self::publish::Flow;
use crate::PluginSet;
use crate::run::Done;

/// Carries commands and calls of interfaces from one plugin to
/// another and the answers back, gives events to the plugins that
/// import them, and answers the calls that end for another reason.
pub fn plugin(app: &mut App) {
    app.init_resource::<Calls>()
        .init_resource::<Invoking>()
        .init_resource::<Flow>()
        .add_observer(chief::forward)
        .add_observer(tools::called)
        .add_observer(hosted::turn_observed)
        .add_observer(invoke::lift)
        .add_systems(
            Update,
            (
                (
                    settle::settle.run_if(Calls::any),
                    hosted::agents_changed
                        .run_if(resource_changed::<AgentList>),
                    hosted::selection_changed
                        .run_if(resource_changed::<SelectedAgent>),
                )
                    .in_set(PluginSet::Collect),
                (
                    route::route.run_if(on_message::<Done>),
                    invoke::deliver.run_if(Invoking::any),
                )
                    .chain()
                    .in_set(PluginSet::Apply),
            ),
        );
}
