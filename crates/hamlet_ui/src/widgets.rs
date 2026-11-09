//! What every panel is built from.

pub mod cut;
pub mod dial;
pub mod disclosure;
pub mod edges;
pub mod effects;
pub mod ellipsis;
pub mod hint;
pub mod icons;
pub mod platform;
pub mod providers;
pub mod range;
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
        cut::CutPlugin,
        edges::plugin,
        ellipsis::plugin,
        hint::plugin,
        providers::plugin,
    ));
}
