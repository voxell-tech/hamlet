//! Helpers the tests of the Hub's parts share, and the window's.

use std::io::Cursor;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::math::UVec2;
use hamlet_core::fynix::dock::{DockAreaStyle, DockLeaf, DockTree};
use image::{ImageFormat, RgbaImage};

/// A window's panels on the fixture state, with no first-run setup.
pub fn panel_app() -> App {
    let mut app = hamlet_core::testing::panel_app();
    app.add_plugins(crate::asking::plugin);
    app.init_resource::<DockTree>()
        .init_resource::<crate::select::Selecting>();
    hub(&mut app);
    app
}

/// The first Hub in a dock of its own, following the world's
/// selection.
pub fn hub(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.get_resource_or_init::<DockTree>().set_root_leaf(
        DockLeaf::new("side", DockAreaStyle::TabBar)
            .with_windows(vec![crate::hub::HUB.into()]),
    );
    crate::hub::first(world)
}

/// The chat of the first Hub.
pub fn convo(app: &App) -> &crate::convo::Convo {
    let hub = crate::hub::following(app.world()).unwrap();
    app.world().get::<crate::convo::Convo>(hub).unwrap()
}

/// The Hub's chat with the Hand seated.
pub fn seat_hand(app: &mut App) -> Entity {
    let hand =
        hamlet_core::client::realm::named(app.world_mut(), "hand")
            .unwrap();
    let hub = hub(app);
    crate::convo::seat(app.world_mut(), hub, hand);
    hand
}

/// The Hub's chat with an agent named `agent#2` seated.
pub fn seat_ada(app: &mut App) -> Entity {
    let ada =
        hamlet_core::client::realm::named(app.world_mut(), "agent#2")
            .unwrap();
    let hub = hub(app);
    crate::convo::seat(app.world_mut(), hub, ada);
    ada
}

/// A PNG of `size`, one colour all over.
pub fn png(size: UVec2) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    RgbaImage::from_pixel(
        size.x,
        size.y,
        image::Rgba([200, 150, 60, 255]),
    )
    .write_to(&mut bytes, ImageFormat::Png)
    .unwrap();
    bytes.into_inner()
}
