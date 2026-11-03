//! Lily pads: leaves lying on the water, painted with the leaves'
//! strokes and cut to them, each riding the water's waves as one, as
//! they move the water's surface at its middle. At night their
//! flowers glow.

use bevy::asset::embedded_asset;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::{ShaderRef, load_shader_library};

use crate::grass::GrassSettings;
use crate::ground::{PagesUniform, blank_pages};
use crate::water::{WaterExt, WaterUniform};

pub type PadMaterial = ExtendedMaterial<StandardMaterial, PadExt>;

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "pads_ride.wesl");
    embedded_asset!(app, "pads.wesl");
    app.add_plugins(MaterialPlugin::<PadMaterial>::default())
        .init_resource::<PadSettings>();
}

/// How the lily pads glow at night.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadSettings {
    /// How brightly a pad's flower glows at the height of the night,
    /// swelling and fading as the grass's flowers do.
    pub glow: f32,
}

impl PadSettings {
    /// [`PadExt::glow`] for `night` of the way into the night, the
    /// glow swelling as `grass` has the grass's flowers swell.
    pub fn glow(&self, grass: &GrassSettings, night: f32) -> Vec4 {
        Vec4::new(
            self.glow * night,
            grass.pulse.0.max(1e-3),
            grass.pulse.1,
            0.0,
        )
    }
}

/// What a pad rides and is painted with: the water's waves, as
/// [`WaterExt`] has them, which the world keeps the same as the
/// water's, and the leaves' strokes; and how it glows.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct PadExt {
    #[uniform(100)]
    pub water: WaterUniform,
    #[texture(101)]
    #[sampler(102)]
    pub noise: Handle<Image>,
    #[texture(110)]
    #[sampler(111)]
    pub strokes: Handle<Image>,
    #[texture(115, dimension = "2d_array", sample_type = "u_int")]
    pub pages: Handle<Image>,
    #[uniform(116)]
    pub pages_at: PagesUniform,
    /// How brightly the flowers glow, as far into the night as it
    /// is, the breadth in metres of the patches their glow swells
    /// in, and how fast those drift.
    #[uniform(117)]
    pub glow: Vec4,
}

impl PadExt {
    /// Takes the water's waves from `water`, telling whether they
    /// had changed.
    pub fn follow(&mut self, water: &WaterExt) -> bool {
        let changed = self.water != water.water
            || self.noise != water.noise
            || self.pages != water.pages
            || self.pages_at != water.pages_at;
        if changed {
            self.water = water.water.clone();
            self.noise = water.noise.clone();
            self.pages = water.pages.clone();
            self.pages_at = water.pages_at.clone();
        }
        changed
    }
}

impl MaterialExtension for PadExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/pads.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/pads.wesl".into()
    }

    // The water reads the depth the prepass leaves to tell how deep
    // it is, so a pad just under it would read as a shore and turn
    // the water over it white.
    fn enable_prepass() -> bool {
        false
    }

    // A pad rides the waves only as far as they are from the eye,
    // which the sun's shadow pass would take to be the sun: its
    // shadow would lie still under the pad as the pad rose and fell
    // through it.
    fn enable_shadows() -> bool {
        false
    }
}

/// The pads' material, painted with `strokes`, on still water until
/// the world gives it the water's waves.
pub fn material(
    images: &mut Assets<Image>,
    strokes: Handle<Image>,
) -> PadMaterial {
    ExtendedMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.55,
            reflectance: 0.3,
            double_sided: true,
            cull_mode: None,
            // The shader cuts each pad to its stroke.
            alpha_mode: AlphaMode::Mask(0.0),
            ..default()
        },
        extension: PadExt {
            water: WaterUniform::default(),
            noise: Handle::default(),
            strokes,
            pages: images.add(blank_pages()),
            pages_at: PagesUniform::default(),
            glow: Vec4::ZERO,
        },
    }
}
