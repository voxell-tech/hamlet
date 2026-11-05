//! The ground's material: coloured by its shader point by point, as
//! [`SoilUniform`] and the vertex colours direct, then shaded as
//! short grass seen from above: clumps of blades, each blade lighter
//! than the gaps between them and rounded across as the ridge it
//! is. The blades are one baked texture that the shader lays down
//! in hexagons, each moved and turned at random and blended by
//! height, so that it never shows as repeating.
//!
//! A ground mesh's vertex colours are not colours: red is the share
//! of woods there, green the share of heath, and blue how far the
//! ground stands above where the vertex is drawn.

use std::f32::consts::TAU;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{
    ImageAddressMode, ImageSampler, ImageSamplerDescriptor,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension,
    TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::shader::{ShaderRef, load_shader_library};

pub type GroundMaterial =
    ExtendedMaterial<StandardMaterial, GroundExt>;

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "noise.wesl");
    load_shader_library!(app, "ground_shape.wesl");
    load_shader_library!(app, "soil.wesl");
    embedded_asset!(app, "ground.wesl");
    embedded_asset!(app, "ground_prepass.wesl");
    app.add_plugins(MaterialPlugin::<GroundMaterial>::default())
        .init_resource::<GroundSettings>()
        .add_systems(
            Update,
            retune.run_if(resource_changed::<GroundSettings>),
        );
}

/// The blades in the ground's colour and in its light.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundSettings {
    /// World units across one copy of the texture.
    pub repeat: f32,
    /// Slope of the steepest blade's side. Zero leaves the ground
    /// flat.
    pub strength: f32,
    /// Share by which the gaps between the blades are darkened.
    pub cavity: f32,
    /// World units across the patches the blades stand out more and
    /// less in, and the share of the strength at the least.
    pub patches: f32,
    pub calm: f32,
    /// Share of a full turn each copy may be turned by. Below 1 the
    /// blades keep to one way.
    pub turn: f32,
    /// What the ground's own colour is multiplied by in the gaps
    /// between the blades and on the lightest of them.
    pub dark: [f32; 3],
    pub light: [f32; 3],
    /// Share of the strength in the gaps. On the blades it is whole.
    pub bare: f32,
    pub blades: BladeSettings,
}

/// The grass baked into the texture, as shares of its side.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BladeSettings {
    /// Texels across the texture.
    pub side: usize,
    /// Clumps of blades in it, and blades in each.
    pub clumps: usize,
    pub blades: usize,
    /// How far a clump's blades rise from about its middle.
    pub spread: f32,
    /// Least and most length and width of a blade.
    pub length: (f32, f32),
    pub width: (f32, f32),
    /// Share of a blade's way that is its clump's; the rest is its
    /// own.
    pub together: f32,
    /// The speckle over everything.
    pub grain: f32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct GroundExt {
    #[uniform(100)]
    pub u: GroundUniform,
    #[texture(101)]
    #[sampler(102)]
    pub bumps: Handle<Image>,
    #[uniform(103)]
    pub soil: SoilUniform,
    /// The land's pages, a layer for each size of patch, as the
    /// realm makes them: in each texel the height's bits, and the
    /// woods and the heath as two shares of 16 bits.
    #[texture(104, dimension = "2d_array", sample_type = "u_int")]
    pub pages: Handle<Image>,
    #[uniform(105)]
    pub pages_at: PagesUniform,
    /// The flowers it is painted with where the grass's own are too
    /// small to show, as the grass has them.
    #[uniform(106)]
    pub blooms: BloomsUniform,
}

/// Mirrors `Blooms` in `ground.wesl`: the flowers the ground is
/// painted with, as [`GrassUniform::blooms`] has them.
///
/// [`GrassUniform::blooms`]: crate::grass::GrassUniform::blooms
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct BloomsUniform {
    /// The share of tufts in flower where flowers grow, how far from
    /// the eye the grass reaches, how far apart its tufts are, and
    /// the share of its reach it shortens over.
    pub grow: Vec4,
    /// How brightly they glow, as far into the night as it is; the
    /// breadth of the patches their glow swells in; how fast those
    /// drift; and [`NowOffset`](crate::NowOffset).
    pub glow: Vec4,
}

/// Sizes of patch the ground is drawn in, each twice the last, and
/// so the layers of its pages.
pub const LEVELS: usize = 8;

/// Mirrors `Pages` in `ground_shape.wesl`: where the pages lie and
/// how the patches stand on them.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct PagesUniform {
    /// Each page's corner on the map and the width of its texel.
    pub at: [Vec4; LEVELS],
    /// The side of the smallest patch, the height of the plane the
    /// patches are chosen on, and the depth a patch of the second
    /// size is sunk and the most any is.
    pub shape: Vec4,
    /// The height of the water, which ground below stays at.
    pub water: Vec4,
}

/// Mirrors `Soil` in `soil.wesl`: the colours the ground and the
/// flowers on it are painted with, linear, the height of the water,
/// which the shore follows, and the seed of the land's noise.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct SoilUniform {
    pub grass_shade: Vec4,
    pub grass: Vec4,
    pub grass_sun: Vec4,
    pub grass_high: Vec4,
    pub rock_dark: Vec4,
    pub rock: Vec4,
    pub rock_warm: Vec4,
    pub snow: Vec4,
    pub dirt: Vec4,
    pub dirt_shade: Vec4,
    pub mud: Vec4,
    pub flowers: [Vec4; 5],
    pub flower_heart: Vec4,
    pub water: f32,
    pub seed: u32,
}

impl MaterialExtension for GroundExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/ground.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/ground.wesl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/ground_prepass.wesl".into()
    }
}

/// Mirrors `Ground` in `ground.wesl`; the fields are those of
/// [`GroundSettings`].
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct GroundUniform {
    pub dark: Vec4,
    pub light: Vec4,
    pub repeat: f32,
    pub strength: f32,
    pub cavity: f32,
    pub patches: f32,
    pub calm: f32,
    pub turn: f32,
    pub bare: f32,
}

impl From<&GroundSettings> for GroundUniform {
    fn from(set: &GroundSettings) -> Self {
        Self {
            dark: Vec3::from(set.dark).extend(1.0),
            light: Vec3::from(set.light).extend(1.0),
            repeat: set.repeat,
            strength: set.strength,
            cavity: set.cavity,
            patches: set.patches,
            calm: set.calm,
            turn: set.turn,
            bare: set.bare,
        }
    }
}

/// The ground's material over `base`, painted with `soil`, with its
/// texture baked anew.
pub fn material(
    images: &mut Assets<Image>,
    set: &GroundSettings,
    soil: SoilUniform,
    base: StandardMaterial,
) -> GroundMaterial {
    ExtendedMaterial {
        base,
        extension: GroundExt {
            u: set.into(),
            bumps: images.add(bumps(&set.blades)),
            soil,
            pages: images.add(blank_pages()),
            pages_at: PagesUniform::default(),
            blooms: BloomsUniform::default(),
        },
    }
}

/// Pages of nothing, until the land's are made.
pub(crate) fn blank_pages() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: LEVELS as u32,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rg32Uint,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image
}

/// Follows the settings: the uniform at once, the texture baked anew
/// when its blades change.
fn retune(
    set: Res<GroundSettings>,
    mut materials: ResMut<Assets<GroundMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut baked: Local<Option<BladeSettings>>,
) {
    let u = GroundUniform::from(&*set);
    let rebake = baked.as_ref().is_some_and(|was| *was != set.blades);
    *baked = Some(set.blades.clone());
    let image = rebake.then(|| bumps(&set.blades));
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

/// A square that tiles with itself, with all its mip levels. Red and
/// green are the blades' slope along each side, from -1 at 0 to 1 at
/// 255, blue is their height and alpha their tone, 0 in the gaps.
fn bumps(set: &BladeSettings) -> Image {
    let side = set.side.next_power_of_two().max(4);
    let (height, tone) = turf(set, side);
    let at =
        |x: usize, y: usize| height[(y % side) * side + x % side];
    let mut steepest = 0f32;
    let mut level = Vec::with_capacity(side * side);
    for y in 0..side {
        for x in 0..side {
            let dx = (at(x + 1, y) - at(x + side - 1, y)) * 0.5;
            let dy = (at(x, y + 1) - at(x, y + side - 1)) * 0.5;
            steepest = steepest.max(dx.abs()).max(dy.abs());
            level.push([dx, dy, at(x, y), tone[y * side + x]]);
        }
    }
    for texel in &mut level {
        texel[0] /= steepest;
        texel[1] /= steepest;
    }
    let mut data = Vec::new();
    let (mut across, mut levels) = (side, 0);
    loop {
        for [dx, dy, h, a] in &level {
            let byte =
                |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            data.extend([
                byte(dx * 0.5 + 0.5),
                byte(dy * 0.5 + 0.5),
                byte(*h),
                byte(*a),
            ]);
        }
        levels += 1;
        if across == 1 {
            break;
        }
        // Slopes, heights and coverage all average truly, so each
        // level is the mean of four texels of the one before.
        let half = across / 2;
        level = (0..half * half)
            .map(|i| {
                let (x, y) = (i % half * 2, i / half * 2);
                let mut sum = [0.0; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let t = level[(y + dy) * across + x + dx];
                    for k in 0..4 {
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

/// The height and the tone, each from 0 to 1, of short grass seen
/// from above on a square `side` texels across: clumps of blades,
/// each blade rising from its root to its tip and rounded across, the
/// higher blade hiding the lower, and nothing in the gaps. A blade
/// that runs off one side comes back on the other, so the square
/// tiles.
fn turf(set: &BladeSettings, side: usize) -> (Vec<f32>, Vec<f32>) {
    let n = side as f32;
    let mut height = vec![0f32; side * side];
    let mut tone = vec![0f32; side * side];
    for c in 0..set.clumps as u32 {
        let r = |k: u32| hash(c, k, 1);
        let middle = Vec2::new(r(0), r(1)) * n;
        let way = Vec2::from_angle(r(2) * TAU);
        // Each clump its own shade and height, so they show as clumps
        // where the blades are too fine to.
        let (shade, tall) = (r(3), 0.6 + 0.4 * r(4));
        for b in 0..set.blades as u32 {
            let r = |k: u32| hash(c, b * 16 + k, 2);
            let root = middle
                + Vec2::from_angle(r(0) * TAU)
                    * r(1).sqrt()
                    * set.spread
                    * n;
            let along = (way * set.together
                + Vec2::from_angle(r(2) * TAU)
                    * (1.0 - set.together))
                .normalize_or(Vec2::X);
            let length = lerp(set.length.0, set.length.1, r(3)) * n;
            let half_width =
                lerp(set.width.0, set.width.1, r(4)) * n * 0.5;
            let top = tall * (0.75 + 0.25 * r(5));
            let own = (0.6 * shade + 0.4 * r(6)).clamp(0.0, 1.0);
            let reach = length.ceil() as i32 + 1;
            let (cx, cy) = (root.x as i32, root.y as i32);
            for py in cy - reach..=cy + reach {
                for px in cx - reach..=cx + reach {
                    let d =
                        Vec2::new(px as f32 + 0.5, py as f32 + 0.5)
                            - root;
                    let u = d.dot(along) / length;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    // Narrowing to its tip, never under half a texel.
                    let half =
                        (half_width * (1.0 - u).powf(0.6)).max(0.5);
                    let across = along.perp_dot(d).abs() / half;
                    if across > 1.0 {
                        continue;
                    }
                    let ridge = 1.0 - across * across;
                    let h = top
                        * (0.35 + 0.65 * u * (2.0 - u))
                        * (0.6 + 0.4 * ridge);
                    let at = py.rem_euclid(side as i32) as usize
                        * side
                        + px.rem_euclid(side as i32) as usize;
                    if h > height[at] {
                        height[at] = h;
                        // Lighter toward the tip and along the ridge.
                        tone[at] = own
                            * (0.7 + 0.3 * u)
                            * (0.75 + 0.25 * ridge);
                    }
                }
            }
        }
    }
    for (at, t) in tone.iter_mut().enumerate() {
        *t += (hash(at as u32, 0, 4) - 0.5) * set.grain;
    }
    (normalised(height), normalised(tone))
}

/// `values` stretched to run from 0 to 1.
fn normalised(values: Vec<f32>) -> Vec<f32> {
    let (lo, hi) =
        values.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    values
        .iter()
        .map(|v| (v - lo) / (hi - lo).max(1e-6))
        .collect()
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
