//! Water: one surface at the water's height for the sea, the rivers
//! and every pool, moved by the waves of `waves.wesl`, with the foam
//! `foam.rs` keeps on it. How it looks is in `water.wesl`.
//!
//! Ported from an MIT-licensed water shader; its notice is in
//! `water/LICENSE`.

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{
    ImageAddressMode, ImageSampler, ImageSamplerDescriptor,
};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension,
    TextureFormat,
};
use bevy::shader::{ShaderRef, load_shader_library};
use bevy::window::RequestRedraw;

use crate::wind::WindSettings;
use crate::{Now, VfxSystems};

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "waves.wesl");
    embedded_asset!(app, "water.wesl");
    app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
        .init_resource::<WaterSettings>()
        .add_systems(Update, flow.in_set(VfxSystems));
}

/// How the water looks.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaterSettings {
    /// The water's colours, in sRGB, in bands by its depth: in the
    /// shallows, in the middle and deep.
    pub shallow: [f32; 3],
    pub mid: [f32; 3],
    pub deep: [f32; 3],
    /// How deep the water is, in metres against `scale`, where the
    /// middle band begins and where the deep one does.
    pub depths: (f32, f32),
    /// How much of the ground the shallows show through them, from 0
    /// to 1.
    pub clear: f32,
    /// How far apart, in metres, the points the surface's slope is
    /// found between are near the eye. Far off they are twenty times
    /// as far.
    pub sampling: f32,
    /// How large the waves are, against their own size at 1.
    pub scale: f32,
    /// How far out from the land, in metres, the waves begin to turn
    /// to run in to it, as shallowing water turns them. At most
    /// [`SHORE_REACH`].
    pub bend: f32,
    /// How far out from the land, in metres, the waves begin to die
    /// down as shallow water stills them, and the share of their
    /// height they keep at the water's edge. At most [`SHORE_REACH`].
    pub calm: (f32, f32),
    /// How deep the water is, in metres, where it has faded in from
    /// nothing at its edge, so the ground runs into it softly.
    pub soft: f32,
    /// How far the waves tilt the light on the surface, from 0, flat,
    /// to 1, as steep as they are.
    pub normals: f32,
    /// How much of the land and sky the water mirrors, against its
    /// true fresnel; it never mirrors more than half.
    pub mirror: f32,
    pub foam: FoamSettings,
}

/// The foam the waves leave.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoamSettings {
    /// Its colour, in sRGB.
    pub color: [f32; 3],
    /// The metres across the near map of it about the eye, and the
    /// far one.
    pub near: f32,
    pub far: f32,
    /// How fast it fades, a share a second, and how fast noise eats
    /// it away.
    pub decay: f32,
    pub erosion: f32,
    /// How far along x and z noise draws it into streaks.
    pub streak: (f32, f32),
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterExt {
    #[uniform(100)]
    pub water: WaterUniform,
    /// The noise the waves are lifted by, and the foam born and eaten
    /// by.
    #[texture(101)]
    #[sampler(102)]
    pub noise: Handle<Image>,
    /// The noise the strip of foam along the water's edge breathes by.
    #[texture(103)]
    #[sampler(104)]
    pub edge_noise: Handle<Image>,
    /// What foam looks like.
    #[texture(105)]
    #[sampler(106)]
    pub sea_foam: Handle<Image>,
    /// The near map of the foam on it and the far one, as `foam.rs`
    /// keeps them.
    #[texture(107)]
    #[sampler(108)]
    pub foam_near: Handle<Image>,
    #[texture(109)]
    #[sampler(110)]
    pub foam_far: Handle<Image>,
    /// The field of the distance to the shore, as `shore` in
    /// `waves.wesl` reads it.
    #[texture(111)]
    #[sampler(112)]
    pub shore: Handle<Image>,
}

/// Mirrors `Water` in `water.wesl`.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct WaterUniform {
    /// [`WaterSettings::shallow`], linear, and `sampling`.
    pub shallow: Vec4,
    /// `mid`, linear, and the depth its band begins at.
    pub mid: Vec4,
    /// `deep`, linear, and the depth its band begins at.
    pub deep: Vec4,
    /// The foam's colour, linear, and `clear`.
    pub foam_color: Vec4,
    /// The middle of the foam's maps on the world's map, and the
    /// metres across the near one and the far one. None is drawn
    /// while they are 0 across.
    pub foam: Vec4,
    /// The time, `scale` and `bend`.
    pub time: Vec4,
    /// The corner of the shore's field on the map, and its size.
    pub field: Vec4,
    /// `calm`, `soft` and `normals`.
    pub calm: Vec4,
    /// The way the wind blows over the ground, which the waves run.
    pub wind: Vec4,
    /// The colour of the sky over it, linear, which it mirrors where
    /// it finds nothing nearer, and `mirror`.
    pub sky: Vec4,
}

impl WaterUniform {
    /// The water of `set`, with no shore near until its field is
    /// baked.
    pub fn new(set: &WaterSettings) -> Self {
        let linear =
            |c: [f32; 3]| LinearRgba::from(Color::srgb(c[0], c[1], c[2])).to_vec3();
        Self {
            shallow: linear(set.shallow).extend(set.sampling),
            mid: linear(set.mid).extend(set.depths.0),
            deep: linear(set.deep).extend(set.depths.1),
            foam_color: linear(set.foam.color).extend(set.clear),
            foam: Vec4::ZERO,
            time: Vec4::new(0.0, set.scale, set.bend, 0.0),
            field: Vec4::new(0.0, 0.0, 1.0, 1.0),
            calm: Vec4::new(set.calm.0, set.calm.1, set.soft, set.normals),
            wind: Vec4::new(0.0, 1.0, 0.0, 0.0),
            sky: Vec4::new(0.5, 0.6, 0.7, set.mirror),
        }
    }
}

/// The farthest from the shore its field tells, in metres. Beyond, it
/// tells this.
pub const SHORE_REACH: f32 = 16.0;

/// The field of the distance to the shore, `width` by `height`, its
/// rows from the corner of least x and z: at each point the distance
/// in metres, above zero over water; the way to the shore over x and
/// z; and how much of it is sea.
pub fn shore_field(
    width: u32,
    height: u32,
    points: &[[f32; 4]],
) -> Image {
    let snorm = |v: f32| {
        ((v.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes()
    };
    let data = points
        .iter()
        .flat_map(|&[d, x, z, sea]| {
            [d / SHORE_REACH, x, z, sea].into_iter().flat_map(snorm)
        })
        .collect::<Vec<_>>();
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Snorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler =
        ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

impl MaterialExtension for WaterExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/water.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/water.wesl".into()
    }

    // Out of the depth prepass, so that the depth under the water is
    // the ground's.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// The sea's surface, flat, to be raised in waves by its shader: a
/// square reaching `far` from its middle each way, in quads a quarter
/// of a metre wide about the middle that grow wider toward its edges,
/// where they are far off and small to the eye. Kept under the camera,
/// moved a whole metre at a time so that the waves do not swim. Its
/// vertex colours carry to the fragments where each point lay before
/// the waves moved it, and how far they lifted it.
pub fn sea(far: f32) -> Mesh {
    /// Quads along a side, and the metres the even part of the square
    /// reaches each way.
    const QUADS: u32 = 640;
    const NEAR: f32 = 80.0;
    let reach = |i: u32| {
        let u = i as f32 / QUADS as f32 * 2.0 - 1.0;
        u.signum()
            * (NEAR * u.abs()
                + (far - NEAR).max(0.0) * u.abs().powi(6))
    };
    let side = QUADS + 1;
    let mut at = Vec::with_capacity((side * side) as usize);
    for j in 0..side {
        for i in 0..side {
            at.push([reach(i), 0.0, reach(j)]);
        }
    }
    let mut indices =
        Vec::with_capacity((QUADS * QUADS * 6) as usize);
    for j in 0..QUADS {
        for i in 0..QUADS {
            let a = j * side + i;
            indices.extend([
                a,
                a + side,
                a + 1,
                a + 1,
                a + side,
                a + side + 1,
            ]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; at.len()];
    let colours = vec![[1.0; 4]; at.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, at)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
    .with_inserted_indices(Indices::U32(indices))
}

/// The water's material for the water of `set`.
pub fn material(
    images: &mut Assets<Image>,
    set: &WaterSettings,
) -> WaterMaterial {
    let blank = Image::new_fill(
        Extent3d::default(),
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    ExtendedMaterial {
        base: StandardMaterial {
            // Drawn after everything solid, with what was drawn
            // before it to look through.
            specular_transmission: 1.0,
            ..default()
        },
        extension: WaterExt {
            water: WaterUniform::new(set),
            noise: images.add(tiling(cellular(512, 5, 1))),
            edge_noise: images.add(tiling(gradient(512, 5, 2))),
            sea_foam: images.add(tiling(sea_foam())),
            foam_near: images.add(blank.clone()),
            foam_far: images.add(blank),
            // Far from any shore until its field is baked.
            shore: images.add(shore_field(
                1,
                1,
                &[[SHORE_REACH, 0.0, 0.0, 1.0]],
            )),
        },
    }
}

/// A square grey image `side` texels across with all its mip levels,
/// from `level`, its texels row by row from 0 to 1, sampled so that
/// it tiles.
fn tiling((side, level): (usize, Vec<f32>)) -> Image {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut data = Vec::new();
    let (mut across, mut level, mut levels) = (side, level, 0);
    loop {
        data.extend(level.iter().map(|&v| byte(v)));
        levels += 1;
        if across == 1 {
            break;
        }
        let half = across / 2;
        level = (0..half * half)
            .map(|i| {
                let (x, y) = (i % half * 2, i / half * 2);
                [(0, 0), (1, 0), (0, 1), (1, 1)]
                    .iter()
                    .map(|(dx, dy)| level[(y + dy) * across + x + dx])
                    .sum::<f32>()
                    * 0.25
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
        TextureFormat::R8Unorm,
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

/// What foam looks like: a grey square that tiles.
fn sea_foam() -> (usize, Vec<f32>) {
    let png = include_bytes!("water/sea_foam.png");
    let foam = image::load_from_memory(png)
        .map(|image| image.to_luma8())
        .unwrap_or_default();
    let side = foam.width().min(foam.height()) as usize;
    let level = (0..side * side)
        .map(|i| {
            let (x, y) = ((i % side) as u32, (i / side) as u32);
            f32::from(foam.get_pixel(x, y).0[0]) / 255.0
        })
        .collect();
    (side, level)
}

/// A number from 0 to 1 for each of two, that follows from a grid
/// point `cells` across, wrapped, and `seed`.
fn corner(x: usize, y: usize, cells: usize, seed: u32) -> (f32, f32) {
    let (x, y) = ((x % cells) as u32, (y % cells) as u32);
    let mut h = x
        .wrapping_mul(0x8da6_b343)
        .wrapping_add(y.wrapping_mul(0xd816_3841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    let g = h.wrapping_mul(0x297a_2d39);
    (
        (h & 0xffff) as f32 / 65535.0,
        (g >> 16) as f32 / 65535.0,
    )
}

/// Five octaves of noise over a square `side` texels across that
/// tiles, the first `cells` across, each twice as fine and half as
/// strong as the last, stretched to run from 0 to 1. `octave` gives
/// one octave's value at a point on a grid so many cells across.
fn fractal(
    side: usize,
    cells: usize,
    octave: impl Fn(f32, f32, usize) -> f32,
) -> (usize, Vec<f32>) {
    let level = (0..side * side)
        .map(|i| {
            let (x, y) = (
                ((i % side) as f32 + 0.5) / side as f32,
                ((i / side) as f32 + 0.5) / side as f32,
            );
            (0..5)
                .map(|o| {
                    let n = cells << o;
                    octave(x * n as f32, y * n as f32, n) * 0.5f32.powi(o)
                })
                .sum::<f32>()
        })
        .collect::<Vec<_>>();
    let (lo, hi) = level
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let span = (hi - lo).max(1e-6);
    (side, level.into_iter().map(|v| (v - lo) / span).collect())
}

/// Cellular noise: at each point, how far the nearest of points
/// scattered one to a cell is.
fn cellular(side: usize, cells: usize, seed: u32) -> (usize, Vec<f32>) {
    fractal(side, cells, |x, y, n| {
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        let mut nearest = f32::MAX;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (gx, gy) = (cx + dx, cy + dy);
                let wrap = |v: i64| v.rem_euclid(n as i64) as usize;
                let (jx, jy) = corner(wrap(gx), wrap(gy), n, seed);
                let (px, py) = (gx as f32 + jx, gy as f32 + jy);
                nearest = nearest.min((px - x).hypot(py - y));
            }
        }
        nearest
    })
}

/// Gradient noise: smooth hills and hollows.
fn gradient(side: usize, cells: usize, seed: u32) -> (usize, Vec<f32>) {
    fractal(side, cells, |x, y, n| {
        let (cx, cy) = (x.floor() as usize, y.floor() as usize);
        let (fx, fy) = (x.fract(), y.fract());
        let slope = |gx: usize, gy: usize, ox: f32, oy: f32| {
            let a = corner(gx, gy, n, seed).0 * std::f32::consts::TAU;
            a.cos() * ox + a.sin() * oy
        };
        let ease = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let (u, v) = (ease(fx), ease(fy));
        let top = slope(cx, cy, fx, fy)
            + (slope(cx + 1, cy, fx - 1.0, fy) - slope(cx, cy, fx, fy)) * u;
        let bottom = slope(cx, cy + 1, fx, fy - 1.0)
            + (slope(cx + 1, cy + 1, fx - 1.0, fy - 1.0)
                - slope(cx, cy + 1, fx, fy - 1.0))
                * u;
        top + (bottom - top) * v
    })
}

/// A surface of water, kept moving while it is seen.
#[derive(Component)]
pub struct Waves;

/// Tells the water the time, and keeps frames coming while water is in
/// view.
pub(crate) fn flow(
    now: Res<Now>,
    set: Res<WaterSettings>,
    wind: Res<WindSettings>,
    waves: Query<&ViewVisibility, With<Waves>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(now.0) == Some(now.0)
        && !set.is_changed()
        && !wind.is_changed()
    {
        return;
    }
    let time = Vec4::new(now.0, set.scale, set.bend, 0.0);
    let blowing = wind.uniform(now.0).flow;
    let way = Vec4::new(blowing.x, blowing.y, 0.0, 0.0);
    for (_, water) in materials.iter_mut() {
        if water.extension.water.time != time
            || water.extension.water.wind != way
        {
            water.extension.water.time = time;
            water.extension.water.wind = way;
        }
    }
    if waves.iter().any(|seen| seen.get()) {
        redraw.write(RequestRedraw);
    }
}
