//! A plugin panel's contents, drawn as the native panels are.

mod bind;
mod node;
mod stopped;

use bevy_fynix::views::column;
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_sdk::looks::bare;
use hamlet_sdk::{AnyView, dock};

use self::bind::At;
use crate::trees::{PluginTrees, Shape};

/// The contents of the plugin panel of this dock id.
pub fn panel(id: &str) -> AnyView {
    let id = id.to_string();
    let shape = {
        let id = id.clone();
        resource::<PluginTrees, _>(move |trees| trees.shape(&id))
    };
    dock::panel(
        keyed(shape, move |shape| match shape {
            Shape::Waiting => column(()).boxed(),
            Shape::Tree(tag) => node::view(&At::root(&id), *tag),
            Shape::Stopped(why) => stopped::view(&id, why),
        })
        .within(bare()),
    )
}
