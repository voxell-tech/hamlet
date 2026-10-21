//! The materials nearly everything is drawn with. The colour of
//! matte and brass things is in their vertices.

use glam::Vec3;
use hamlet_engine_sdk::scene::{AlphaMode, Material, Pbr, Rgba};

use crate::palette::Palette;
use crate::tint::to_linear;

/// The colour of a candle's flame, in sRGB, and how bright it burns.
const CANDLE: Vec3 = Vec3::new(1.0, 0.816, 0.541);
const EMBER: f32 = 1.5;

/// The shared materials.
pub struct Stock {
    pub matte: Material,
    pub brass: Material,
    /// Every window pane.
    pub windows: Material,
}

/// `colour` with an alpha of 1.
pub fn rgba(colour: Vec3) -> Rgba {
    (colour.x, colour.y, colour.z, 1.0)
}

/// A material that gives off `colour` times `strength` and is not
/// lit, added to what is behind it.
pub fn glow(colour: Vec3, strength: f32) -> Pbr {
    Pbr {
        base_color: rgba(colour * strength),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        fog: false,
        ..Pbr::colored(rgba(Vec3::ONE))
    }
}

impl Stock {
    pub fn make(pal: &Palette) -> Result<Self, String> {
        let white = rgba(Vec3::ONE);
        Ok(Self {
            matte: Material::make(Pbr {
                roughness: 0.92,
                ..Pbr::colored(white)
            })?,
            brass: Material::make(Pbr {
                metallic: 0.75,
                roughness: 0.42,
                ..Pbr::colored(white)
            })?,
            windows: Material::make(Self::pane(pal, 0.0))?,
        })
    }

    /// A window pane with candles burning `candle` strong behind it.
    fn pane(pal: &Palette, candle: f32) -> Pbr {
        let glow = to_linear(CANDLE) * EMBER * candle;
        let base = if candle > 0.0 {
            pal.pane_lit
        } else {
            pal.pane_dark
        };
        Pbr {
            emissive: rgba(glow),
            roughness: 0.3,
            ..Pbr::colored(rgba(base))
        }
    }

    /// Lights every window pane as the hour has it.
    pub fn light_windows(&self, pal: &Palette, candle: f32) {
        self.windows.set(Self::pane(pal, candle));
    }
}
