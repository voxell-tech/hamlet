//! What every panel is built from.

pub mod dial;
pub mod disclosure;
pub mod edges;
pub mod effects;
pub mod ellipsis;
pub mod floats;
pub mod hint;
pub mod icons;
pub mod pixels;
pub mod platform;
pub mod providers;
pub mod rainbow;
pub mod range;
pub mod spoken;
pub mod switch;
pub mod tabs;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        tabs::plugin,
        platform::plugin,
        icons::plugin,
        effects::plugin,
        switch::plugin,
        edges::plugin,
        floats::FloatsPlugin,
        pixels::plugin,
        ellipsis::plugin,
        hint::plugin,
        providers::plugin,
        rainbow::plugin,
        spoken::plugin,
    ));
}
