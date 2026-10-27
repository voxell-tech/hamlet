//! The ground's material: the vertex colours of `StandardMaterial`
//! lit as if painted on in thick strokes. The strokes are one baked
//! texture that the shader lays down in hexagons, each moved and
//! turned at random and blended by height, so that it never shows as
//! repeating.

use std::f32::consts::TAU;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{
    ImageAddressMode, ImageSampler, ImageSamplerDescriptor,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension,
    TextureFormat,
};
use bevy::shader::ShaderRef;

pub type GroundMaterial =
    ExtendedMaterial<StandardMaterial, GroundExt>;

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "ground.wesl");
    app.add_plugins(MaterialPlugin::<GroundMaterial>::default())
        .init_resource::<GroundSettings>()
        .add_systems(
            Update,
            retune.run_if(resource_changed::<GroundSettings>),
        );
}

/// The brush strokes in the ground's light.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundSettings {
    /// World units across one copy of the strokes.
    pub repeat: f32,
    /// Slope of the steepest stroke's edge. Zero leaves the ground
    /// flat.
    pub strength: f32,
    /// Share by which the thinnest paint is darkened.
    pub cavity: f32,
    /// World units across the patches the strokes are stronger and
    /// weaker in, and the share of the strength at the weakest.
    pub patches: f32,
    pub calm: f32,
    /// Share of a full turn each copy may be turned by. Below 1 the
    /// strokes keep to one way.
    pub turn: f32,
    pub strokes: StrokeSettings,
}

/// The strokes baked into the texture, as shares of its side.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeSettings {
    /// Texels across the texture.
    pub side: usize,
    /// Strokes laid one over another.
    pub count: usize,
    /// Least and most length and width of a stroke.
    pub length: (f32, f32),
    pub width: (f32, f32),
    /// Share of a stroke's angle that is random. At 0 every stroke
    /// runs the same way.
    pub scatter: f32,
    /// Bristles across a stroke, and the share of its length its
    /// tail breaks into them over.
    pub bristles: f32,
    pub dry: f32,
    /// Height of the streaks the bristles leave, of the paint piled
    /// at a stroke's sides, and of the grain over everything.
    pub streaks: f32,
    pub lip: f32,
    pub grain: f32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct GroundExt {
    #[uniform(100)]
    pub u: GroundUniform,
    #[texture(101)]
    #[sampler(102)]
    pub bumps: Handle<Image>,
}

impl MaterialExtension for GroundExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/ground.wesl".into()
    }
}

/// Mirrors `Ground` in `ground.wesl`; the fields are those of
/// [`GroundSettings`].
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct GroundUniform {
    pub repeat: f32,
    pub strength: f32,
    pub cavity: f32,
    pub patches: f32,
    pub calm: f32,
    pub turn: f32,
}

impl From<&GroundSettings> for GroundUniform {
    fn from(set: &GroundSettings) -> Self {
        Self {
            repeat: set.repeat,
            strength: set.strength,
            cavity: set.cavity,
            patches: set.patches,
            calm: set.calm,
            turn: set.turn,
        }
    }
}

/// The ground's material over `base`, with its strokes baked anew.
pub fn material(
    images: &mut Assets<Image>,
    set: &GroundSettings,
    base: StandardMaterial,
) -> GroundMaterial {
    ExtendedMaterial {
        base,
        extension: GroundExt {
            u: set.into(),
            bumps: images.add(bumps(&set.strokes)),
        },
    }
}

/// Follows the settings: the uniform at once, the texture baked anew
/// when its strokes change.
fn retune(
    set: Res<GroundSettings>,
    mut materials: ResMut<Assets<GroundMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut baked: Local<Option<StrokeSettings>>,
) {
    let u = GroundUniform::from(&*set);
    let rebake =
        baked.as_ref().is_some_and(|was| *was != set.strokes);
    *baked = Some(set.strokes.clone());
    let image = rebake.then(|| bumps(&set.strokes));
    for (_, material) in materials.iter_mut() {
        if material.extension.u != u {
            material.extension.u = u.clone();
        }
        if let Some(image) = &image
            && let Some(mut old) =
                images.get_mut(&material.extension.bumps)
        {
            *old = image.clone();
        }
    }
}

/// A square of strokes that tiles with itself, with all its mip
/// levels. Red and green are the slope along each side, from -1 at 0
/// to 1 at 255, blue is the height.
fn bumps(set: &StrokeSettings) -> Image {
    let side = set.side.next_power_of_two().max(4);
    let height = strokes(set, side);
    let at =
        |x: usize, y: usize| height[(y % side) * side + x % side];
    let mut slopes = Vec::with_capacity(side * side);
    for y in 0..side {
        for x in 0..side {
            let dx = (at(x + 1, y) - at(x + side - 1, y)) * 0.5;
            let dy = (at(x, y + 1) - at(x, y + side - 1)) * 0.5;
            slopes.push([dx, dy, at(x, y)]);
        }
    }
    // The grain's single texels are far steeper than any stroke's
    // edge, so the slopes are scaled to all but the steepest few.
    let mut steep = slopes
        .iter()
        .map(|[dx, dy, _]| dx.abs().max(dy.abs()))
        .collect::<Vec<_>>();
    steep.sort_by(f32::total_cmp);
    let steepest = steep[steep.len() * 98 / 100].max(1e-6);
    let mut level = slopes
        .iter()
        .map(|&[dx, dy, h]| [dx / steepest, dy / steepest, h])
        .collect::<Vec<_>>();
    let mut data = Vec::new();
    let (mut across, mut levels) = (side, 0);
    loop {
        for [dx, dy, h] in &level {
            let byte =
                |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            data.extend([
                byte(dx * 0.5 + 0.5),
                byte(dy * 0.5 + 0.5),
                byte(*h),
                255,
            ]);
        }
        levels += 1;
        if across == 1 {
            break;
        }
        // A slope and a height both average truly, so each level is
        // the mean of four texels of the one before.
        let half = across / 2;
        level = (0..half * half)
            .map(|i| {
                let (x, y) = (i % half * 2, i / half * 2);
                let mut sum = [0.0; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let t = level[(y + dy) * across + x + dx];
                    for k in 0..3 {
                        sum[k] += t[k] * 0.25;
                    }
                }
                sum
            })
            .collect();
        across = half;
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(
        ImageSamplerDescriptor {
            anisotropy_clamp: 16,
            ..ImageSamplerDescriptor::linear()
        }
        .set_address_mode(ImageAddressMode::Repeat)
        .clone(),
    );
    image
}

/// Heights from 0 to 1 of thick paint laid in strokes, each over the
/// ones before, on a square `side` texels across. A stroke that runs
/// off one side comes back on the other, so the square tiles.
fn strokes(set: &StrokeSettings, side: usize) -> Vec<f32> {
    let n = side as f32;
    let mut height = vec![0f32; side * side];
    for i in 0..set.count as u32 {
        let r = |k: u32| hash(i, k, 1);
        let centre = Vec2::new(r(0), r(1)) * n;
        let angle = (r(2) - 0.5) * set.scatter * TAU;
        let along = Vec2::from_angle(angle);
        let half_length =
            lerp(set.length.0, set.length.1, r(3)) * n * 0.5;
        let half_width =
            lerp(set.width.0, set.width.1, r(4)) * n * 0.5;
        // Each stroke is its own thickness of paint.
        let paint = lerp(0.35, 1.0, r(5));
        let reach = half_length.hypot(half_width).ceil() as i32 + 1;
        let (cx, cy) = (centre.x as i32, centre.y as i32);
        for py in cy - reach..=cy + reach {
            for px in cx - reach..=cx + reach {
                let d = Vec2::new(px as f32 + 0.5, py as f32 + 0.5)
                    - centre;
                let u = d.dot(along) / half_length;
                let w = along.perp_dot(d) / half_width;
                if u.abs() > 1.0 || w.abs() > 1.2 {
                    continue;
                }
                // A side that wanders, a rounded head, and a tail
                // that breaks into bristles of uneven length.
                let edge = 1.0 - 0.22 * wobble(u * 5.0, i);
                let side_in =
                    1.0 - smoothstep(edge - 0.12, edge, w.abs());
                let head = smoothstep(-1.0, -0.75, u + 0.25 * w * w);
                let bristle =
                    ((w * 0.5 + 0.5) * set.bristles).floor() as u32;
                let end = 1.0 - 2.0 * set.dry * hash(i, bristle, 2);
                let tail = 1.0 - smoothstep(end - 0.12, end, u);
                let cover = side_in * head * tail;
                if cover <= 0.0 {
                    continue;
                }
                let streak =
                    (hash(i, bristle, 3) - 0.5) * set.streaks;
                let lip = set.lip * smoothstep(0.5, 0.9, w.abs());
                let at = py.rem_euclid(side as i32) as usize * side
                    + px.rem_euclid(side as i32) as usize;
                height[at] =
                    lerp(height[at], paint + streak + lip, cover);
            }
        }
    }
    for (at, h) in height.iter_mut().enumerate() {
        *h += (hash(at as u32, 0, 4) - 0.5) * set.grain;
    }
    let (lo, hi) =
        height.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &h| {
            (lo.min(h), hi.max(h))
        });
    height
        .iter()
        .map(|h| (h - lo) / (hi - lo).max(1e-6))
        .collect()
}

/// Smooth noise from 0 to 1 along one line, one of many by `seed`.
fn wobble(t: f32, seed: u32) -> f32 {
    let i = t.floor();
    let f = t - i;
    let a = hash(i as i32 as u32, seed, 5);
    let b = hash((i as i32 + 1) as u32, seed, 5);
    lerp(a, b, f * f * (3.0 - 2.0 * f))
}

fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A number from 0 to 1 that follows from `x`, `y` and `seed` alone.
fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x
        .wrapping_mul(0x8da6_b343)
        .wrapping_add(y.wrapping_mul(0xd816_3841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32
}
