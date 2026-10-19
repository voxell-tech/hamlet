//! What other plugins use of the limits: the panes of the home's
//! models and of each hamlet's rules, which the limits plugin sets,
//! and how a list of models is narrowed.

pub mod models;
pub mod rules;

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy_fynix::ViewExt;
use bevy_fynix::views::column;
use hamlet_sdk::{AnyView, with_cx};

/// The panes of the limits that other plugins show. The limits
/// plugin sets it.
#[derive(Resource, Clone, Copy)]
pub struct Panes {
    pub endpoints: fn() -> AnyView,
    pub shelf: fn() -> AnyView,
    pub of_agent: fn(Entity) -> AnyView,
    pub realm: fn() -> AnyView,
    pub rules: fn(Entity) -> AnyView,
}

/// The view `pane` makes of the [`Panes`], or nothing where the
/// limits are not there.
fn shown(pane: impl FnOnce(&Panes) -> AnyView + 'static) -> AnyView {
    with_cx(move |cx| match cx.world.get_resource::<Panes>() {
        Some(panes) => pane(panes),
        None => column(()).boxed(),
    })
}
