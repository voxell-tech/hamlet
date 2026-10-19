//! A canvas: a box a plugin draws a few 2D shapes in. This is the
//! only drawing a plugin of WASM has. Nothing here reaches the world,
//! a shader or a texture.

use bevy::color::Color;

use crate::theme::Palette;

/// The colours of the theme a shape may be painted with, by name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Swatch {
    /// Text that is read.
    Ink,
    /// Text that stands back.
    Dim,
    Faint,
    Brass,
    /// What is live.
    Lamp,
    Flame,
    Ember,
    Rune,
    Card,
    Well,
    /// The panel's own dark.
    Panel,
    Seam,
    /// What is refused or at fault.
    Red,
    Moss,
}

impl Swatch {
    pub const ALL: [Swatch; 14] = [
        Swatch::Ink,
        Swatch::Dim,
        Swatch::Faint,
        Swatch::Brass,
        Swatch::Lamp,
        Swatch::Flame,
        Swatch::Ember,
        Swatch::Rune,
        Swatch::Card,
        Swatch::Well,
        Swatch::Panel,
        Swatch::Seam,
        Swatch::Red,
        Swatch::Moss,
    ];

    /// The name a plugin writes.
    pub fn name(self) -> &'static str {
        match self {
            Swatch::Ink => "ink",
            Swatch::Dim => "dim",
            Swatch::Faint => "faint",
            Swatch::Brass => "brass",
            Swatch::Lamp => "lamp",
            Swatch::Flame => "flame",
            Swatch::Ember => "ember",
            Swatch::Rune => "rune",
            Swatch::Card => "card",
            Swatch::Well => "well",
            Swatch::Panel => "panel",
            Swatch::Seam => "seam",
            Swatch::Red => "red",
            Swatch::Moss => "moss",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|swatch| swatch.name() == name)
    }

    /// The colour of the swatch in a theme's palette.
    pub fn colour(self, palette: &Palette) -> Color {
        match self {
            Swatch::Ink => palette.brass_1,
            Swatch::Dim => palette.grey_1,
            Swatch::Faint => palette.brass_2,
            Swatch::Brass => palette.brass_4,
            Swatch::Lamp => palette.lamp,
            Swatch::Flame => palette.flame,
            Swatch::Ember => palette.ember,
            Swatch::Rune => palette.rune,
            Swatch::Card => palette.card,
            Swatch::Well => palette.well,
            Swatch::Panel => palette.black,
            Swatch::Seam => palette.seam,
            Swatch::Red => palette.red,
            Swatch::Moss => palette.moss,
        }
    }
}

/// What a shape is painted with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paint {
    /// A colour of the theme, so the drawing follows it.
    Swatch(Swatch),
    Rgba([u8; 4]),
}

impl Paint {
    pub fn colour(self, palette: &Palette) -> Color {
        match self {
            Paint::Swatch(swatch) => swatch.colour(palette),
            Paint::Rgba([r, g, b, a]) => Color::srgba_u8(r, g, b, a),
        }
    }
}

/// One shape, in pixels from the top left of its canvas.
#[derive(Clone, PartialEq, Debug)]
pub enum Shape {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        paint: Paint,
    },
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        paint: Paint,
    },
    /// A disc about its centre.
    Circle {
        x: f32,
        y: f32,
        r: f32,
        paint: Paint,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        paint: Paint,
    },
}

/// A box of `width` by `height` pixels and the shapes in it, drawn
/// first to last. What falls outside it is cut off.
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Canvas {
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Shape>,
}
