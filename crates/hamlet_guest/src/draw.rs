//! Shapes for a canvas (see [`ui::canvas`](crate::ui::canvas)): a few
//! rectangles, lines, circles and words in a box of a panel. It needs
//! the capability `ui.canvas`, allowed when the chief lets the plugin
//! in. The paints are those of the marks over the world too
//! ([`overlay`](crate::overlay)).

use crate::bindings::exports::hamlet::plugin::plugin::{
    CircleShape, LineShape, Paint, RectShape, Shape, TextShape,
};

/// A colour of the theme by name, so the drawing follows the theme:
/// `ink`, `dim`, `faint`, `brass`, `lamp`, `flame`, `ember`, `rune`,
/// `card`, `well`, `panel`, `seam`, `red` or `moss`. A name that is
/// not one fails the panel.
pub fn theme(name: &str) -> Paint {
    Paint::Palette(name.into())
}

/// A colour of its own, with `a` for how much of it shows.
pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Paint {
    Paint::Rgba(u32::from_be_bytes([r, g, b, a]))
}

/// A rectangle with its corner at (`x`, `y`), in pixels from the top
/// left of the canvas.
pub fn rect(x: f32, y: f32, w: f32, h: f32, paint: Paint) -> Shape {
    Shape::Rect(RectShape { x, y, w, h, paint })
}

/// A line between two points, `width` pixels thick (from half a pixel
/// to 64).
pub fn line(
    from: (f32, f32),
    to: (f32, f32),
    width: f32,
    paint: Paint,
) -> Shape {
    Shape::Line(LineShape {
        x1: from.0,
        y1: from.1,
        x2: to.0,
        y2: to.1,
        width,
        paint,
    })
}

/// A disc about (`x`, `y`) of radius `r`.
pub fn circle(x: f32, y: f32, r: f32, paint: Paint) -> Shape {
    Shape::Circle(CircleShape { x, y, r, paint })
}

/// Words with their top left corner at (`x`, `y`), `size` pixels
/// high (from 6 to 128).
pub fn text(
    x: f32,
    y: f32,
    text: &str,
    size: f32,
    paint: Paint,
) -> Shape {
    Shape::Text(TextShape {
        x,
        y,
        text: text.into(),
        size,
        paint,
    })
}
