//! The leader's banner: cloth hung from its mast, blown along by a
//! vertex shader and turned down the wind.

use std::f32::consts::FRAC_PI_2;

use glam::{Quat, Vec2, Vec3};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    AlphaMode, Material, Shaded, Shader, ShaderKind,
};

use crate::build::mesh::Mb;
use crate::build::upload::Draw;

const VFX: &str = include_str!("../../../../../assets/vfx.toml");

/// The waving of a flag.
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
pub struct FlagSettings {
    /// Speed of the waves along the cloth, the swing of its fly end
    /// as a share of its length, and the number of waves it holds.
    pub speed: f32,
    pub swing: f32,
    pub waves: f32,
    /// Stripes along the cloth, and how far the fly end is cut back
    /// at its top and bottom, as a share of its height.
    pub stripes: usize,
    pub taper: f32,
}

/// The way the wind blows.
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
struct WindSettings {
    /// In degrees from north, and how far it veers to either side.
    heading: f32,
    veer: f32,
}

/// The sections of `assets/vfx.toml` a banner reads.
#[derive(serde::Deserialize)]
struct VfxTuning {
    flag: FlagSettings,
    wind: WindSettings,
}

/// The banners' shader, and how they wave.
pub struct Flags {
    pub set: FlagSettings,
    wind: WindSettings,
    shader: Handle<Shader>,
}

impl Flags {
    pub fn make(draw: &mut Draw) -> Result<Self, String> {
        let vfx = toml::from_str::<VfxTuning>(VFX)
            .map_err(|why| format!("assets/vfx.toml: {why}"))?;
        let shader = draw.assets.add(Shader::compile(
            ShaderKind::Material,
            include_str!("flag.wgsl"),
        )?);
        Ok(Self {
            set: vfx.flag,
            wind: vfx.wind,
            shader,
        })
    }

    /// The material of a flag `long` from hoist to fly that hangs at
    /// `at` in the realm.
    pub fn material(
        &self,
        long: f32,
        at: Vec3,
        draw: &mut Draw,
    ) -> Result<Handle<Material>, String> {
        let set = &self.set;
        // Flags in different places are at different moments of
        // their wave.
        let own = at.x * 0.37 + at.z * 0.23;
        draw.assets.add_shaded_material(
            &self.shader,
            Shaded {
                alpha_mode: AlphaMode::Opaque,
                double_sided: true,
                lit: true,
            },
            &[
                set.speed, set.swing, set.waves, long, own, 0.0, 0.0,
                0.0,
            ],
            None,
            None,
        )
    }

    /// The turn that lays a flag made along x down the wind, at clock
    /// time `now`. The wind veers slowly from side to side.
    pub fn downwind(&self, now: f32) -> Quat {
        let wind = &self.wind;
        let heading = (wind.heading
            + wind.veer * 0.5 * (now * 0.11).sin())
        .to_radians();
        // A heading of 0 blows north.
        Quat::from_rotation_y(heading - FRAC_PI_2)
    }
}

impl FlagSettings {
    /// The cloth of a flag `long` by `tall`, hung by its top corner
    /// at the origin and lying along x, in stripes of `colours`
    /// in turn.
    pub fn cloth(
        &self,
        long: f32,
        tall: f32,
        colours: &[Vec3],
    ) -> Mb {
        // Columns of points to a stripe, so that a stripe bends.
        const FINE: usize = 4;
        let mut mb = Mb::new();
        let stripes = self.stripes.max(1);
        for stripe in 0..stripes {
            let colour = colours
                .get(stripe % colours.len().max(1))
                .copied()
                .unwrap_or(Vec3::ONE);
            let column = (0..=FINE)
                .map(|step| {
                    let u = (stripe as f32
                        + step as f32 / FINE as f32)
                        / stripes as f32;
                    // Only the last stripe is cut back.
                    let cut = self.taper
                        * 0.5
                        * ((u * stripes as f32)
                            - (stripes - 1) as f32)
                            .max(0.0);
                    [cut, 1.0 - cut].map(|v| {
                        mb.vuv(
                            Vec3::new(u * long, -v * tall, 0.0),
                            Vec3::Z,
                            colour,
                            Vec2::new(u, v),
                        )
                    })
                })
                .collect::<Vec<_>>();
            for pair in column.windows(2) {
                mb.quad(
                    pair[0][0], pair[0][1], pair[1][1], pair[1][0],
                );
            }
        }
        mb
    }
}
