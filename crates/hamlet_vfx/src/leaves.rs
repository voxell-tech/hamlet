//! Leaves: cards painted with brush strokes, cut sharp to the
//! stroke's edge, set on a tree's crown and bent by the wind from
//! its foot. Lit, they are warm; out of the sun they turn cool, glow
//! at the crown's edge with the sun behind them and, at night, keep a
//! rim of the night's colour. The colour is in the mesh's
//! vertices, tinted for each tree by its
//! [`MeshTag`](bevy::mesh::MeshTag).

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension,
    TextureFormat,
};
use bevy::shader::{ShaderRef, load_shader_library};

use crate::wind::{WindSettings, WindUniform};
use crate::{Now, VfxSystems};

pub type LeafMaterial = ExtendedMaterial<StandardMaterial, LeafExt>;

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "leaves_sway.wesl");
    embedded_asset!(app, "leaves.wesl");
    embedded_asset!(app, "leaves_prepass.wesl");
    app.add_plugins(MaterialPlugin::<LeafMaterial>::default())
        .init_resource::<LeafSettings>()
        .init_resource::<LeafView>()
        .init_resource::<Leaves>()
        .add_systems(Update, sway.in_set(VfxSystems));
}

/// How leaves move and take the light.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeafSettings {
    /// How far the top of a tree three metres tall leans in the
    /// full wind, in metres, and how fast it rocks there.
    pub bend: f32,
    pub rock: f32,
    /// How far the leaves shiver at the crown's top, in metres, and
    /// how fast.
    pub flutter: f32,
    pub shiver: f32,
    /// The colour, in sRGB, the leaves turn to where the sun does
    /// not reach them, and how far they turn to it, from 0 to 1.
    pub shade: [f32; 3],
    pub cool: f32,
    /// How bright the crown's edge glows with the sun behind it,
    /// against the sun on its face.
    pub glow: f32,
    /// How much more of the sky's light the leaves take than other
    /// things do, so that they are lit mostly by the sky.
    pub sky: f32,
    /// The colour, in sRGB, of the rim the leaves keep at night, and
    /// how bright it is.
    pub rim: [f32; 3],
    pub rim_light: f32,
}

/// What the world tells the leaves of itself.
#[derive(Resource, Clone, Default, PartialEq, Debug)]
pub struct LeafView {
    /// How far into the night it is, from 0 by day to 1.
    pub night: f32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct LeafExt {
    #[uniform(100)]
    pub leaves: LeafUniform,
    #[uniform(101)]
    pub wind: WindUniform,
    #[texture(102)]
    #[sampler(103)]
    pub strokes: Handle<Image>,
    /// Whether the leaves move. Those of many trees in one mesh do
    /// not, having no one foot to bend from.
    pub sways: bool,
}

/// Mirrors `Leaves` in `leaves_sway.wesl`; the fields are explained
/// there.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct LeafUniform {
    pub sway: Vec4,
    pub edge: Vec4,
    pub shade: Vec4,
    pub rim: Vec4,
}

impl LeafUniform {
    fn new(
        set: &LeafSettings,
        view: &LeafView,
        now: f32,
        sways: bool,
    ) -> Self {
        let moves = if sways { 1.0 } else { 0.0 };
        let linear =
            |c: [f32; 3]| LinearRgba::from(Color::srgb_from_array(c));
        let (shade, rim) = (linear(set.shade), linear(set.rim));
        Self {
            sway: Vec4::new(
                now,
                set.bend * moves,
                set.rock,
                set.flutter * moves,
            ),
            edge: Vec4::new(
                set.shiver, set.glow, set.sky, view.night,
            ),
            shade: Vec4::new(
                shade.red,
                shade.green,
                shade.blue,
                set.cool,
            ),
            rim: Vec4::new(
                rim.red,
                rim.green,
                rim.blue,
                set.rim_light,
            ),
        }
    }
}

impl MaterialExtension for LeafExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/leaves.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/leaves.wesl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/leaves_prepass.wesl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/leaves_prepass.wesl".into()
    }
}

/// The two materials every tree's leaves are drawn with.
#[derive(Resource)]
pub struct Leaves {
    /// For a tree drawn alone, standing on its mesh's origin.
    pub sway: Handle<LeafMaterial>,
    /// For many trees in one mesh, and leaves lying on the ground.
    pub still: Handle<LeafMaterial>,
}

impl FromWorld for Leaves {
    fn from_world(world: &mut World) -> Self {
        let set =
            world.get_resource_or_init::<LeafSettings>().clone();
        let strokes =
            world.resource_mut::<Assets<Image>>().add(strokes());
        let mut materials =
            world.resource_mut::<Assets<LeafMaterial>>();
        Self {
            sway: materials.add(material(&set, &strokes, true)),
            still: materials.add(material(&set, &strokes, false)),
        }
    }
}

/// The leaves' material: for trees each drawn alone where `sways`,
/// each standing on its mesh's origin, else for many in one mesh.
fn material(
    set: &LeafSettings,
    strokes: &Handle<Image>,
    sways: bool,
) -> LeafMaterial {
    ExtendedMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.92,
            reflectance: 0.18,
            double_sided: true,
            cull_mode: None,
            // The shaders cut each card to its stroke; a mask is what
            // has the depth and the shadows run them too.
            alpha_mode: AlphaMode::Mask(0.0),
            ..default()
        },
        // The time and the wind are told in `sway`.
        extension: LeafExt {
            leaves: LeafUniform::new(
                set,
                &LeafView::default(),
                0.0,
                sways,
            ),
            wind: WindUniform::default(),
            strokes: strokes.clone(),
            sways,
        },
    }
}

/// Tells the leaves the time, the wind and what the world tells them.
fn sway(
    now: Res<Now>,
    set: Res<LeafSettings>,
    view: Res<LeafView>,
    wind: Res<WindSettings>,
    mut materials: ResMut<Assets<LeafMaterial>>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(now.0) == Some(now.0)
        && !set.is_changed()
        && !view.is_changed()
        && !wind.is_changed()
    {
        return;
    }
    let blowing = wind.uniform(now.0);
    for (_, leaves) in materials.iter_mut() {
        let next = LeafUniform::new(
            &set,
            &view,
            now.0,
            leaves.extension.sways,
        );
        if leaves.extension.leaves != next
            || leaves.extension.wind != blowing
        {
            leaves.extension.leaves = next;
            leaves.extension.wind = blowing.clone();
        }
    }
}

/// The tint of the tree whose tag is `tag`, as `tint` in
/// `leaves.wesl` has it: a little lighter or darker, warmer or
/// cooler, so that no two trees of a mesh are alike. A tag of 0 is
/// no tint.
pub fn tint(tag: u32) -> Vec3 {
    if tag == 0 {
        return Vec3::ONE;
    }
    let a = (tag & 0xff) as f32 / 255.0 - 0.5;
    let b = ((tag >> 8) & 0xff) as f32 / 255.0 - 0.5;
    let light = 1.0 + 0.22 * a;
    light * Vec3::new(1.0 + 0.1 * b, 1.0, 1.0 - 0.12 * b)
}

/// The strokes a leaf card is painted with, each in its own cell of
/// the strokes' texture.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stroke {
    /// A square dab, dry at one end.
    Dab,
    /// A rounder blot.
    Blot,
    /// A long stroke that hangs from its top and frays at its tail.
    Hang,
    /// A few short strokes fanned out from a point at the top.
    Fan,
}

impl Stroke {
    pub const ALL: [Stroke; 4] =
        [Stroke::Dab, Stroke::Blot, Stroke::Hang, Stroke::Fan];

    /// The stroke's cell of the texture, as its UV corners: the top
    /// left, then the bottom right. Down the cell is down the stroke.
    pub fn cell(self) -> (Vec2, Vec2) {
        let (x, y) = match self {
            Stroke::Dab => (0, 0),
            Stroke::Blot => (1, 0),
            Stroke::Hang => (0, 1),
            Stroke::Fan => (1, 1),
        };
        let at = |x: usize, y: usize| {
            Vec2::new(
                (SOLID + x * CELL.0) as f32 / SIDE as f32,
                (y * CELL.1) as f32 / SIDE as f32,
            )
        };
        (at(x, y), at(x + 1, y + 1))
    }

    /// How much of the stroke covers the point `p` of its cell, from
    /// 0 at the top left to 1 at the bottom right, and the tone of
    /// the paint there.
    fn paint(self, p: Vec2) -> (f32, f32) {
        let (x, y) = (p.x, p.y);
        let bristles = noise(Vec2::new(x * 2.5, y * 42.0), 3);
        let grain = noise(p * 18.0, 4);
        let ragged = noise(p * 9.0, 5) - 0.5;
        let cover = match self {
            Stroke::Dab => {
                let d = Vec2::new((x - 0.5) / 0.4, (y - 0.5) / 0.36);
                let r = (d.x.powi(4) + d.y.powi(4)).powf(0.25);
                let body = smooth(1.06, 0.94, r + 0.22 * ragged);
                // Dry at the right end, where the bristles part.
                let dry = smooth(0.6, 0.92, x);
                body * step(dry * 0.85, bristles)
            }
            Stroke::Blot => {
                let d =
                    Vec2::new((x - 0.5) / 0.42, (y - 0.52) / 0.38);
                body_of(d.length() + 0.3 * ragged)
            }
            Stroke::Hang => {
                let wide = 0.42 * (1.0 - 0.45 * y);
                let across = (x - 0.5).abs() / wide;
                let body = smooth(1.05, 0.9, across + 0.2 * ragged)
                    * smooth(0.02, 0.1, y);
                let streaks = noise(Vec2::new(x * 34.0, y * 2.0), 6);
                let fray = smooth(0.5, 0.97, y);
                body * step(fray, streaks * 1.08)
            }
            Stroke::Fan => (0..5)
                .map(|k| {
                    let turn = (k as f32 - 2.0) * 0.32;
                    let way = Vec2::new(turn.sin(), turn.cos());
                    let from = p - Vec2::new(0.5, 0.06);
                    let along = from.dot(way);
                    let off = (from - way * along).length();
                    let long = 0.8 - 0.1 * (k as f32 - 2.0).abs();
                    let wide = 0.09 * (1.0 - 0.5 * along / long);
                    smooth(1.0, 0.85, off / wide + 0.3 * ragged)
                        * smooth(long, long - 0.12, along)
                        * smooth(0.0, 0.05, along)
                })
                .fold(0.0, f32::max),
        };
        let tone = 0.8 + 0.14 * bristles + 0.08 * grain;
        (cover, tone)
    }
}

fn body_of(r: f32) -> f32 {
    smooth(1.04, 0.92, r)
}

/// Pixels across the strokes' texture. `STROKES` in
/// `leaves_sway.wesl`.
const SIDE: usize = 512;
/// Pixels across the solid band down its left side, where whatever
/// is not a card takes its colour, at UV zero.
const SOLID: usize = 64;
/// Pixels across and down each stroke's cell.
const CELL: (usize, usize) = ((SIDE - SOLID) / 2, SIDE / 2);

/// The strokes' texture, with all its mip levels. Red, green and
/// blue are the paint's tone, alpha how much of a stroke covers it.
fn strokes() -> Image {
    let mut level = vec![[1.0f32; 2]; SIDE * SIDE];
    for stroke in Stroke::ALL {
        let (lo, _) = stroke.cell();
        let (x0, y0) = (
            (lo.x * SIDE as f32).round() as usize,
            (lo.y * SIDE as f32).round() as usize,
        );
        // A margin about each stroke, so that the levels far down
        // do not bleed one cell into the next.
        const MARGIN: f32 = 0.04;
        for y in 0..CELL.1 {
            for x in 0..CELL.0 {
                let p = Vec2::new(
                    (x as f32 + 0.5) / CELL.0 as f32,
                    (y as f32 + 0.5) / CELL.1 as f32,
                );
                let inner = (p - MARGIN) / (1.0 - 2.0 * MARGIN);
                let (cover, tone) = if inner.cmplt(Vec2::ZERO).any()
                    || inner.cmpgt(Vec2::ONE).any()
                {
                    (0.0, 0.9)
                } else {
                    stroke.paint(inner)
                };
                level[(y0 + y) * SIDE + x0 + x] = [tone, cover];
            }
        }
    }
    let mut data = Vec::new();
    let (mut across, mut levels) = (SIDE, 0);
    loop {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        for [tone, cover] in &level {
            let t = byte(*tone);
            data.extend([t, t, t, byte(*cover)]);
        }
        levels += 1;
        if across == 1 {
            break;
        }
        // The tone is weighed by how much each texel is covered, so
        // the bare paper about a stroke does not pale its edge.
        let half = across / 2;
        level = (0..half * half)
            .map(|i| {
                let (x, y) = (i % half * 2, i / half * 2);
                let (mut tone, mut cover) = (0.0, 0.0);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let [t, c] = level[(y + dy) * across + x + dx];
                    tone += t * (c + 0.01);
                    cover += c;
                }
                [tone / (cover + 0.04), cover * 0.25]
            })
            .collect();
        across = half;
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: SIDE as u32,
            height: SIDE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler =
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            // Below this the cells run together.
            lod_max_clamp: 4.0,
            anisotropy_clamp: 8,
            ..ImageSamplerDescriptor::linear()
        });
    image
}

fn smooth(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn step(edge: f32, x: f32) -> f32 {
    if x < edge { 0.0 } else { 1.0 }
}

/// Smooth noise from 0 to 1 over `p`, one of many by `seed`.
fn noise(p: Vec2, seed: u32) -> f32 {
    let hash = |x: i32, y: i32| {
        let mut h = (x as u32).wrapping_mul(0x8da6_b343)
            ^ (y as u32).wrapping_mul(0xd816_3841)
            ^ seed.wrapping_mul(0x9e37_79b9);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h & 0xff_ffff) as f32 / 16_777_216.0
    };
    let i = p.floor();
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let (x, y) = (i.x as i32, i.y as i32);
    let low = hash(x, y).lerp(hash(x + 1, y), u.x);
    let high = hash(x, y + 1).lerp(hash(x + 1, y + 1), u.x);
    low.lerp(high, u.y)
}
