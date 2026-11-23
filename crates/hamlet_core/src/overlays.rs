//! The views a part lays over the whole window, such as its popups.

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use hamlet_ui::{AnyView, Cx};

pub fn plugin(app: &mut App) {
    app.init_resource::<Overlays>().init_resource::<Covers>();
}

/// The checks of whether a part has something open over the panels,
/// such as a menu, that keys typed are for.
#[derive(Resource, Default)]
pub struct Covers(Vec<fn(&World) -> bool>);

/// Whether a part has something open over the panels.
pub fn covered(world: &World) -> bool {
    world
        .get_resource::<Covers>()
        .is_some_and(|covers| covers.0.iter().any(|open| open(world)))
}

/// A view laid over the window, kept above the ones with a lower
/// `order`.
#[derive(Clone, Copy)]
pub struct Overlay {
    pub order: u32,
    pub view: fn() -> AnyView,
}

/// The views laid over the window, in their order.
#[derive(Resource, Default)]
pub struct Overlays(Vec<Overlay>);

impl Overlays {
    pub fn iter(&self) -> impl Iterator<Item = &Overlay> {
        self.0.iter()
    }
}

/// Adds a view over the window from a part's own plugin.
pub trait OverlayAppExt {
    fn add_overlay(
        &mut self,
        order: u32,
        view: fn() -> AnyView,
    ) -> &mut Self;

    /// Adds the check of whether the part has something open over
    /// the panels.
    fn add_cover(&mut self, open: fn(&World) -> bool) -> &mut Self;
}

impl OverlayAppExt for App {
    fn add_overlay(
        &mut self,
        order: u32,
        view: fn() -> AnyView,
    ) -> &mut Self {
        self.init_resource::<Overlays>();
        let mut overlays =
            self.world_mut().resource_mut::<Overlays>();
        let at =
            overlays.0.partition_point(|other| other.order <= order);
        overlays.0.insert(at, Overlay { order, view });
        self
    }

    fn add_cover(&mut self, open: fn(&World) -> bool) -> &mut Self {
        self.init_resource::<Covers>();
        self.world_mut().resource_mut::<Covers>().0.push(open);
        self
    }
}

/// The overlays built, in their order. They are laid in the window's
/// own column, after what they cover.
pub fn overlays(cx: &Cx) -> Vec<AnyView> {
    let made = cx
        .world
        .resource::<Overlays>()
        .iter()
        .map(|overlay| overlay.view)
        .collect::<Vec<_>>();
    made.into_iter().map(|view| view()).collect()
}
