//! The look of the panels (docs/ui-warmth.md): warm near-black
//! surfaces, brass frames, and lamplight as the one colour that
//! glows.

use std::time::Duration;

use bevy::color::{Color, LinearRgba, Luminance};
use bevy::ui::{BoxShadow, px};
use bevy_fynix::tokens::{
    Curve, Motion, MotionTokens, SpacingTokens, SurfaceTokens,
    TextTokens, Tone,
};

/// The panels' theme.
pub struct Lamplight;

const fn hex(rgb: u32) -> Color {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// A panel: soot, the darkest warm surface but for a well.
pub const BLACK: Color = hex(0x120e0a);
/// Text that stands back.
pub const GREY_1: Color = hex(0xab9f8b);
/// The wood of a button.
pub(crate) const GREY_4: Color = hex(0x2a2018);
/// A seam between two surfaces, and the tint of what is selected.
pub(crate) const SEAM: Color = hex(0x2c231b);
/// Parchment, for what is read.
pub const BRASS_1: Color = hex(0xf2e7d0);
/// Brass, from what is lettered in it down to a hairline.
pub(crate) const BRASS_2: Color = hex(0xd9c391);
pub const BRASS_3: Color = hex(0xcaa666);
pub(crate) const BRASS_4: Color = hex(0xc9983f);
pub const BRASS_5: Color = hex(0x7c5d2f);
pub const BRASS_6: Color = hex(0x45351f);
/// Lamplight: what is live, what waits, and every glow.
pub(crate) const LAMP: Color = hex(0xf2b35a);
/// The heart of a flame.
pub(crate) const FLAME: Color = hex(0xffd58a);
/// A spark that rises from what was just done.
pub const EMBER: Color = hex(0xff9a3c);
/// The glass of a lamp that burns low, away from its flame and near
/// it.
pub(crate) const LAMP_LOW: Color = hex(0x3e2810);
pub(crate) const LAMP_HIGH: Color = hex(0x7a5220);
/// The agents' own magic, as the world draws it.
pub(crate) const RUNE: Color = hex(0x6fd6d2);
/// A card in a list: a shade above the panel, and no more.
pub const CARD: Color = hex(0x1b1510);
/// The floor of a text field, sunk below the panel.
pub(crate) const WELL: Color = hex(0x0b0806);
/// The plate under code in a reply.
pub(crate) const PLATE: Color = hex(0x211a13);
/// Red, for what is refused, stopped or at fault.
pub(crate) const RED: Color = hex(0xf47560);
/// A refusing button's plate, and its glow.
pub(crate) const RED_DEEP: Color = hex(0x3a1711);
/// The frame of a button that ends something, while it rests, and
/// its plate under the pointer.
pub(crate) const RED_LOW: Color = hex(0x8f4034);
pub(crate) const RED_HOVER: Color = hex(0x5a2219);
pub(crate) const COAL: Color = hex(0xd9482f);
/// Moss, for a line put in a file, and the plate under it.
pub(crate) const MOSS: Color = hex(0xa9d18a);
pub(crate) const MOSS_DEEP: Color = hex(0x1d2a16);
/// The warmth that gathers at the foot of a surface.
pub(crate) const HEARTH: Color = hex(0x1f150c);
/// A button's fill under the pointer.
pub(crate) const BRASS_HOVER: Color = hex(0x3b2d1e);

/// The shadow under what floats over the panels and the world.
pub(crate) const SHADOW: Color =
    Color::srgba(0.02, 0.012, 0.005, 0.55);

/// The contrast of two colours, as WCAG 2 has it.
pub(crate) fn contrast(one: Color, other: Color) -> f32 {
    let light =
        |color: Color| LinearRgba::from(color).luminance() + 0.05;
    let (a, b) = (light(one), light(other));
    a.max(b) / a.min(b)
}

impl Lamplight {
    /// The window background, behind the panels.
    pub fn background() -> Color {
        BLACK
    }

    /// The corner of a control and of a card, worn round. Frames
    /// stay square.
    pub fn round() -> f32 {
        4.0
    }

    /// The corner of what floats: a menu, a card over the world.
    pub fn soft() -> f32 {
        6.0
    }

    /// The shadow of what floats, cast by the lamps.
    pub fn shadow() -> BoxShadow {
        BoxShadow::new(SHADOW, px(0.0), px(6.0), px(0.0), px(18.0))
    }
}

impl TextTokens for Lamplight {
    fn tone(&self, tone: Tone) -> Color {
        match tone {
            Tone::Body => BRASS_1,
            Tone::Dim => GREY_1,
            // Brass 2 marks what can be pressed.
            Tone::Faint => BRASS_2,
            Tone::Accent => LAMP,
            Tone::OnAccent => BLACK,
            Tone::Critical => RED,
        }
    }

    fn body_size(&self) -> f32 {
        14.0
    }

    fn small_size(&self) -> f32 {
        12.0
    }
}

impl SurfaceTokens for Lamplight {
    fn fill(&self) -> Color {
        GREY_4
    }

    fn hover(&self) -> Color {
        hex(0x34291f)
    }

    fn panel(&self) -> Color {
        BLACK
    }

    fn hairline(&self) -> Color {
        BRASS_6
    }

    fn selection(&self) -> Color {
        BRASS_6
    }

    fn accent(&self) -> Color {
        BRASS_4
    }
}

impl SpacingTokens for Lamplight {
    fn gap(&self) -> f32 {
        8.0
    }

    fn row(&self) -> f32 {
        28.0
    }

    fn radius(&self) -> f32 {
        Self::round()
    }

    fn divider(&self) -> f32 {
        1.0
    }

    fn divider_grip(&self) -> f32 {
        8.0
    }

    fn tab_padding(&self) -> f32 {
        12.0
    }

    fn menu_radius(&self) -> f32 {
        Self::soft()
    }
}

impl MotionTokens for Lamplight {
    fn motion(&self, motion: Motion) -> Curve {
        let millis = match motion {
            Motion::Interact => 140,
            Motion::Expand => 220,
        };
        Curve {
            duration: Duration::from_millis(millis),
            ease: |t| 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_reads_on_every_surface_it_meets() {
        let tones = [
            Tone::Body,
            Tone::Dim,
            Tone::Faint,
            Tone::Accent,
            Tone::Critical,
        ];
        let surfaces = [
            BLACK, WELL, CARD, PLATE, GREY_4, HEARTH, RED_DEEP, SEAM,
        ];
        for tone in tones {
            for surface in surfaces {
                let ratio = contrast(Lamplight.tone(tone), surface);
                assert!(
                    ratio >= 4.5,
                    "{tone:?} on {surface:?}: {ratio}"
                );
            }
        }
        // Under the pointer a button is lettered in parchment, and a
        // row keeps its dim words.
        assert!(contrast(BRASS_1, BRASS_HOVER) >= 4.5);
        assert!(contrast(GREY_1, BRASS_HOVER) >= 4.5);
        assert!(contrast(BRASS_1, Lamplight.selection()) >= 4.5);
        // The good action is lettered on its glass, lit or not.
        assert!(contrast(BRASS_1, LAMP_LOW) >= 4.5);
        assert!(contrast(BRASS_1, LAMP_HIGH) >= 4.5);
        assert!(contrast(BRASS_2, LAMP_LOW) >= 4.5);
        assert!(contrast(RUNE, BLACK) >= 4.5);
        // A line put in a file, on its own plate and on that of the
        // change.
        assert!(contrast(MOSS, MOSS_DEEP) >= 4.5);
        assert!(contrast(MOSS, PLATE) >= 4.5);
        // The chief's own words in the chat, and their hour.
        assert!(contrast(BRASS_1, BRASS_6) >= 4.5);
        assert!(contrast(BRASS_2, BRASS_6) >= 4.5);
    }
}
