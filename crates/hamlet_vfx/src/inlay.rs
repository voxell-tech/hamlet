//! The inlay: one custom material over `StandardMaterial`, driven by
//! uniforms.

use bevy::asset::embedded_asset;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use hamlet_assets::tint::hex;

/// The inlay's material and its shader.
pub struct InlayPlugin;

impl Plugin for InlayPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "inlay.wgsl");
        app.add_plugins(MaterialPlugin::<InlayMaterial>::default());
    }
}

pub type InlayMaterial = ExtendedMaterial<StandardMaterial, InlayExt>;

/// Emissive strengths, relative to sunlit white paper at 1.
pub const EMBER: f32 = 1.5;
pub const LIT: f32 = 4.0;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct InlayExt {
    #[uniform(100)]
    pub u: InlayUniform,
    #[uniform(101)]
    pub stone: InlayStone,
}

impl InlayExt {
    /// An inlay whose unlit groove has a `bed` and a `strip` of
    /// these colours.
    pub fn new(u: InlayUniform, bed: Vec3, strip: Vec3) -> Self {
        Self {
            u,
            stone: InlayStone {
                bed: bed.extend(1.0),
                strip: strip.extend(1.0),
            },
        }
    }
}

/// The colours of the unlit groove. Mirrors `Stone` in `inlay.wgsl`.
#[derive(ShaderType, Reflect, Debug, Clone, Default)]
pub struct InlayStone {
    pub bed: Vec4,
    pub strip: Vec4,
}

/// Mirrors the `Inlay` struct in `inlay.wgsl`; the fields are
/// explained there.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct InlayUniform {
    pub hue: Vec4,
    pub range: Vec4,
    pub anim: Vec4,
    pub shape: Vec4,
}

impl MaterialExtension for InlayExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/inlay.wgsl".into()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InlayState {
    Unlit,
    Current,
    Fill,
    Signal,
    Fault,
}

impl InlayState {
    pub fn next(self) -> Self {
        match self {
            Self::Unlit => Self::Current,
            Self::Current => Self::Fill,
            Self::Fill => Self::Signal,
            Self::Signal => Self::Fault,
            Self::Fault => Self::Unlit,
        }
    }
}

pub fn current() -> Vec3 {
    hex("#3FD8E8")
}

pub fn signal() -> Vec3 {
    hex("#FF8A1F")
}

pub fn fault() -> Vec3 {
    hex("#FF5247")
}

pub fn candle() -> Vec3 {
    hex("#FFD08A")
}
