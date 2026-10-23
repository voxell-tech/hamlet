//! The materials nearly everything is drawn with. The colour of
//! matte and brass things is in their vertices.

use glam::Vec3;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    AlphaMode, Material, Rgba, StandardMaterial,
};

use crate::build::upload::Draw;
use crate::world::palette::Palette;
use crate::world::tint::to_linear;

/// The colour of a candle's flame, in sRGB, and how bright it burns.
const CANDLE: Vec3 = Vec3::new(1.0, 0.816, 0.541);
const EMBER: f32 = 1.5;

/// The shared materials.
#[derive(Resource)]
pub struct Stock {
    pub matte: Handle<Material>,
    pub brass: Handle<Material>,
    /// Every window pane.
    pub windows: Handle<Material>,
}

/// `colour` with an alpha of 1.
pub fn rgba(colour: Vec3) -> Rgba {
    (colour.x, colour.y, colour.z, 1.0)
}

/// A material that gives off `colour` times `strength` and is not
/// lit, added to what is behind it.
pub fn glow(colour: Vec3, strength: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: rgba(colour * strength),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        fog_enabled: false,
        ..StandardMaterial::colored(rgba(Vec3::ONE))
    }
}

impl Stock {
    pub fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let white = rgba(Vec3::ONE);
        Ok(Self {
            matte: draw.material(StandardMaterial {
                perceptual_roughness: 0.92,
                reflectance: 0.18,
                ..StandardMaterial::colored(white)
            })?,
            brass: draw.material(StandardMaterial {
                metallic: 0.75,
                perceptual_roughness: 0.42,
                ..StandardMaterial::colored(white)
            })?,
            windows: draw.material(Self::pane(pal, 0.0))?,
        })
    }

    /// A window pane with candles burning `candle` strong behind it.
    fn pane(pal: &Palette, candle: f32) -> StandardMaterial {
        let glow = to_linear(CANDLE) * EMBER * candle;
        let base = if candle > 0.0 {
            pal.pane_lit
        } else {
            pal.pane_dark
        };
        StandardMaterial {
            emissive: rgba(glow),
            perceptual_roughness: 0.3,
            ..StandardMaterial::colored(rgba(base))
        }
    }

    /// Lights every window pane as the hour has it.
    pub fn light_windows(
        &self,
        pal: &Palette,
        candle: f32,
        draw: &Draw,
    ) {
        draw.set_material(&self.windows, Self::pane(pal, candle));
    }
}
