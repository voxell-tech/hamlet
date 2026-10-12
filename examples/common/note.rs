//! A line of dark text on paper in a corner of the window.

use bevy::prelude::*;

/// A note that says `text` at the left of the window, at its top or
/// its bottom.
pub fn note(text: &str, top: bool) -> impl Bundle {
    let (above, below) = if top {
        (px(10), auto())
    } else {
        (auto(), px(10))
    };
    (
        Text::new(text),
        TextFont::from_font_size(12.0),
        TextColor(Color::srgb(0.165, 0.129, 0.106)),
        Node {
            position_type: PositionType::Absolute,
            left: px(10),
            top: above,
            bottom: below,
            padding: UiRect::axes(px(8), px(4)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.957, 0.922, 0.847)),
    )
}
