//! Water: one surface at the water's height for the sea, the rivers
//! and every pool, coloured by how deep it is under each point, with
//! light netted on its shallow bed, foam at every shore and on the
//! waves, and ripples that run with the wind. How it looks is in
//! `water.wesl`.

use bevy::asset::embedded_asset;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::window::RequestRedraw;

use crate::wind::{WindSettings, WindUniform};
use crate::{Now, VfxSystems};

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "water.wesl");
    app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
        .init_resource::<WaterSettings>()
        .add_systems(Update, flow.in_set(VfxSystems));
}

/// How the water looks, beside its colours.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaterSettings {
    /// Depth over which the water turns from its shallow colour to
    /// its deep one, and how clear it is where it is shallowest,
    /// from 0 for clear to 1 for none.
    pub fade: f32,
    pub clarity: f32,
    /// How far out from the shore, in depth, the foam's bands reach,
    /// how many bands there are to a metre of depth, and how fast
    /// they roll in.
    pub foam_reach: f32,
    pub bands: f32,
    pub foam_speed: f32,
    /// Depth over which the bright edge at the waterline fades.
    pub edge: f32,
    /// How rough the waves must be, from 0 to 1, before they cap
    /// with foam.
    pub caps: f32,
    /// The ripples: how many to a metre, how steep they are and how
    /// fast they run.
    pub ripples: f32,
    pub steep: f32,
    pub speed: f32,
    /// How bright the light netted on a shallow bed is, and how many
    /// of its cells there are to a metre.
    pub caustics: f32,
    pub net: f32,
    /// How rough the surface is between the ripples.
    pub roughness: f32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterExt {
    #[uniform(100)]
    pub water: WaterUniform,
    #[uniform(101)]
    pub wind: WindUniform,
}

/// Mirrors `Water` in `water.wesl`.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct WaterUniform {
    /// The colours of shallow water, deep water and foam, linear.
    pub shallow: Vec4,
    pub deep: Vec4,
    pub foam: Vec4,
    /// [`WaterSettings::fade`], `clarity`, `foam_reach` and
    /// `foam_speed`.
    pub depth: Vec4,
    /// `ripples`, `steep`, `speed` and `caustics`.
    pub waves: Vec4,
    /// `bands`, `edge`, `caps` and `net`.
    pub shore: Vec4,
}

impl WaterUniform {
    /// The water of `set`, in the colours of the shallows, the deep
    /// and the foam, linear.
    pub fn new(
        set: &WaterSettings,
        shallow: Vec3,
        deep: Vec3,
        foam: Vec3,
    ) -> Self {
        Self {
            shallow: shallow.extend(1.0),
            deep: deep.extend(1.0),
            foam: foam.extend(1.0),
            depth: Vec4::new(
                set.fade,
                set.clarity,
                set.foam_reach,
                set.foam_speed,
            ),
            waves: Vec4::new(
                set.ripples,
                set.steep,
                set.speed,
                set.caustics,
            ),
            shore: Vec4::new(set.bands, set.edge, set.caps, set.net),
        }
    }
}

impl MaterialExtension for WaterExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/water.wesl".into()
    }
}

/// The water's material over `base`.
pub fn material(
    water: WaterUniform,
    roughness: f32,
) -> WaterMaterial {
    ExtendedMaterial {
        base: StandardMaterial {
            perceptual_roughness: roughness,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: WaterExt {
            water,
            wind: WindUniform::default(),
        },
    }
}

/// A surface of water, kept moving while it is seen.
#[derive(Component)]
pub struct Waves;

/// Tells the water the wind and the time, and keeps frames coming
/// while water is in view.
fn flow(
    now: Res<Now>,
    wind: Res<WindSettings>,
    waves: Query<&ViewVisibility, With<Waves>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(now.0) == Some(now.0) && !wind.is_changed() {
        return;
    }
    let blowing = wind.uniform(now.0);
    for (_, water) in materials.iter_mut() {
        if water.extension.wind != blowing {
            water.extension.wind = blowing.clone();
        }
    }
    if waves.iter().any(|seen| seen.get()) {
        redraw.write(RequestRedraw);
    }
}
