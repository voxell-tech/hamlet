//! A plugin panel's contents, drawn as the native panels are.

mod area;
mod bind;
mod canvas;
mod markdown;
mod node;
mod stopped;
#[cfg(test)]
mod tests;

use bevy_fynix::views::column;
use bevy_fynix::{ViewExt, keyed, resource};

use self::bind::At;
use super::store::{PluginTrees, Shape};
use crate::looks::bare;
use crate::{AnyView, dock};

/// The contents of the plugin panel of this dock id.
pub fn panel(id: &str) -> AnyView {
    dock::panel(content(id))
}

/// What the plugin panel of this dock id shows, for a page of a host
/// that is not a dock panel.
pub fn content(id: &str) -> AnyView {
    let id = id.to_string();
    let shape = {
        let id = id.clone();
        resource::<PluginTrees, _>(move |trees| trees.shape(&id))
    };
    keyed(shape, move |shape| match shape {
        Shape::Waiting => column(()).boxed(),
        Shape::Tree(tag) => node::view(&At::root(&id), *tag),
        Shape::Stopped(why) => stopped::view(&id, why),
    })
    .within(bare())
    .boxed()
}
