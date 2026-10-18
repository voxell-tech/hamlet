//! Plugins working together. The host stands in the middle of every
//! meeting: a plugin never holds another's memory or storage, and
//! what crosses is a copy checked against the manifests and the
//! grants.

mod calls;
mod command;
mod mail;
mod settle;

use bevy::prelude::*;

pub use self::calls::{Call, Calls};
use crate::PluginSet;
use crate::run::Done;

/// Carries commands from one plugin to another and the answers
/// back, and answers the calls that end for another reason.
pub fn plugin(app: &mut App) {
    app.init_resource::<Calls>().add_systems(
        Update,
        (
            settle::settle
                .in_set(PluginSet::Collect)
                .run_if(Calls::any),
            command::route
                .in_set(PluginSet::Apply)
                .run_if(on_message::<Done>),
        ),
    );
}
