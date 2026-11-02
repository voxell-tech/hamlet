//! Grass: a meadow of tufts about the eye. Each tuft is one of a few
//! shapes made at random when the grass is made, a clump of curved
//! blades leaning out from its middle, and each is placed, scaled and
//! turned by the world. On the GPU a tuft is set on the land under
//! each of its points, takes the ground's own colour, darker at its
//! roots and lighter to its tips, and is lit as the ground under it
//! is, so that tufts crowding one another read as one soft surface.
//! It shrinks away on whatever a mask marks trodden and toward the
//! edge of the eye's reach, and bends in the wind's gusts.

use std::f32::consts::TAU;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension,
    TextureFormat,
};
use bevy::shader::{ShaderRef, load_shader_library};

use crate::ground::{PagesUniform, SoilUniform, blank_pages};
use crate::leaves::LeafView;
use crate::wind::{WindSettings, WindUniform};
use crate::{Now, VfxSystems};

pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassExt>;

/// Shapes of tuft there are. The first [`SHAPES`] are grass alone;
/// the rest are the first few of those again, in flower.
pub const SHAPES: usize = 8;
pub const IN_FLOWER: usize = 4;
pub const TUFTS: usize = SHAPES + IN_FLOWER;
/// Details each tuft is made in, the finest first.
pub const DETAILS: usize = 3;

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "grass_shape.wesl");
    embedded_asset!(app, "grass.wesl");
    embedded_asset!(app, "grass_prepass.wesl");
    app.add_plugins(MaterialPlugin::<GrassMaterial>::default())
        .init_resource::<GrassSettings>()
        .add_systems(Update, sway.in_set(VfxSystems));
}

/// How the grass grows, takes the light and moves, and how far
/// about the eye it is drawn.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrassSettings {
    /// How tall the tallest blade of a tuft is, and how far out from
    /// its middle the blades rise, in metres.
    pub tall: f32,
    pub spread: f32,
    /// How wide a blade is at its root, in metres.
    pub wide: f32,
    /// Blades in a tuft at each detail, the finest first.
    pub blades: [u32; 3],
    /// How far apart the tufts grow, in metres, and the share of the
    /// places one might grow that are left bare.
    pub spacing: f32,
    pub bare: f32,
    /// The least and the most a tuft is scaled across and up.
    pub across: (f32, f32),
    pub up: (f32, f32),
    /// How far the full wind bends a tuft's tips, as a share of its
    /// height.
    pub bend: f32,
    /// What the ground's colour is multiplied by at a blade's root
    /// and at its tip, and the share of the way its tip turns to the
    /// sunlit grass's colour.
    pub root: f32,
    pub tip: f32,
    pub sun: f32,
    /// How bright the grass's sheen is, seen low against the sun.
    pub sheen: f32,
    /// The share of tufts in flower where flowers grow.
    pub flowers: f32,
    /// How brightly the flowers glow at the height of the night, and
    /// the breadth in metres of the patches their glow swells and
    /// fades in and how fast those drift, in metres a second, on a
    /// noise of their own, not the wind's.
    pub glow: f32,
    pub pulse: (f32, f32),
    /// The side of a square of tufts laid together, in metres.
    pub cell: f32,
    /// How far from the eye each detail reaches, the finest first,
    /// where the eye stands low. The grass shortens to nothing over
    /// the last third of the last.
    pub rings: [f32; 3],
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct GrassExt {
    #[uniform(100)]
    pub grass: GrassUniform,
    #[uniform(101)]
    pub wind: WindUniform,
    #[uniform(103)]
    pub soil: SoilUniform,
    /// The land's pages, as the ground has them.
    #[texture(104, dimension = "2d_array", sample_type = "u_int")]
    pub pages: Handle<Image>,
    #[uniform(105)]
    pub pages_at: PagesUniform,
    /// How trodden the ground is about the eye, from 0 where grass
    /// grows to 1 where none does, laid as `mask_at` in
    /// [`GrassUniform`] has it.
    #[texture(106)]
    #[sampler(107)]
    pub mask: Handle<Image>,
}

/// Mirrors `Grass` in `grass_shape.wesl`; the fields are explained
/// there.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct GrassUniform {
    pub look: Vec4,
    pub sway: Vec4,
    pub glow: Vec4,
    pub reach: Vec4,
    pub mask_at: Vec4,
    pub flowers: [Vec4; 5],
}

impl GrassUniform {
    /// The uniform for `set`, at the time `now`, `night` of the way
    /// into the night, drawn as far as `reach` has it, with the mask
    /// laid at `mask_at` and the flowers in `flowers`.
    pub fn new(
        set: &GrassSettings,
        now: f32,
        night: f32,
        reach: Vec4,
        mask_at: Vec4,
        flowers: [Vec4; 5],
    ) -> Self {
        Self {
            look: Vec4::new(set.root, set.tip, set.sun, set.sheen),
            sway: Vec4::new(set.bend, now, 0.0, 0.0),
            glow: Vec4::new(
                set.glow * night,
                1.0 / set.pulse.0.max(1e-3),
                set.pulse.1,
                0.0,
            ),
            reach,
            mask_at,
            flowers,
        }
    }
}

impl MaterialExtension for GrassExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/grass.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/grass.wesl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://hamlet_vfx/grass_prepass.wesl".into()
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// The grass's material, painted with `soil`, its pages and mask
/// blank until the world gives them.
pub fn material(
    images: &mut Assets<Image>,
    set: &GrassSettings,
    soil: SoilUniform,
) -> GrassMaterial {
    ExtendedMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.9,
            reflectance: 0.15,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: GrassExt {
            grass: GrassUniform::new(
                set,
                0.0,
                0.0,
                Vec4::ZERO,
                Vec4::ZERO,
                [Vec4::ONE; 5],
            ),
            wind: WindUniform::default(),
            soil,
            pages: images.add(blank_pages()),
            pages_at: PagesUniform::default(),
            mask: images.add(mask(1)),
        },
    }
}

/// A mask `side` texels across with nothing trodden, for the world
/// to fill. Each texel is one byte, from 0 to 255.
pub fn mask(side: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0],
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler =
        ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

/// Tuft `shape` of [`TUFTS`] at `detail` of [`DETAILS`], standing on
/// the origin, made at random from its number alone: a clump of
/// curved blades, the outer ones shorter and leaning out further.
/// Each point's UV is how far up its blade it is, from 0 to 1, and a
/// number of its blade's own; its colour is its blade's tone, which
/// the shader multiplies the ground's colour by, with alpha 1, or a
/// flower's head, with alpha 0.
pub fn tuft(
    set: &GrassSettings,
    shape: usize,
    detail: usize,
) -> Mesh {
    /// How far up each pair of points across a blade lies; its tip
    /// is one point.
    const RISE: [f32; 4] = [0.0, 0.3, 0.58, 0.82];
    let seed = (shape % SHAPES) as u32 * 977 + 13;
    let r = |i: u32, k: u32| hash(seed.wrapping_add(i * 7919), k);
    let finest = set.blades[0].max(1);
    let count = set.blades[detail].clamp(1, finest);
    // Fewer blades are drawn wider, to cover as much.
    let wider = (finest as f32 / count as f32).sqrt();
    let stride = finest / count;
    let flowering = shape >= SHAPES;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colours = Vec::new();
    let mut indices = Vec::new();
    for n in 0..count {
        let i = n * stride;
        // Thicker toward the middle.
        let out = r(i, 0).powf(0.7);
        let turn = r(i, 1) * TAU;
        let way = Vec3::new(turn.cos(), 0.0, turn.sin());
        let root = way * out * set.spread * 0.35;
        let height =
            set.tall * (0.5 + 0.5 * r(i, 2)) * (1.0 - 0.4 * out);
        // How far it leans out, and a little to one side.
        let lean = (0.15 + 0.85 * out) * (0.35 + 0.5 * r(i, 3));
        let side = Vec3::new(-way.z, 0.0, way.x);
        let lean_way =
            (way + side * (r(i, 4) - 0.5) * 0.8).normalize();
        let twist = (r(i, 5) - 0.5) * 1.2;
        let across =
            (side * twist.cos() + way * twist.sin()).normalize();
        let wide = set.wide * wider * (0.75 + 0.5 * r(i, 6));
        // Each blade a little lighter or darker, greener or yellower.
        let warm = r(i, 7) - 0.5;
        let tone = (0.88 + 0.24 * r(i, 8))
            * Vec3::new(1.0 + 0.12 * warm, 1.0, 1.0 - 0.25 * warm);
        let own = r(i, 9);
        let centre = |t: f32| {
            root + lean_way * (lean * height * t * t)
                + Vec3::Y * height * (t - 0.3 * lean * lean * t * t)
        };
        let first = positions.len() as u32;
        let mut push = |p: Vec3, t: f32, c: [f32; 4]| {
            let ahead = (centre((t + 0.05).min(1.0))
                - centre((t - 0.05).max(0.0)))
            .normalize_or(Vec3::Y);
            let face = across.cross(ahead).normalize_or(Vec3::Y);
            positions.push(p.to_array());
            normals.push(
                (if face.y < 0.0 { -face } else { face }).to_array(),
            );
            uvs.push([t, own]);
            colours.push(c);
        };
        let grass = [tone.x, tone.y, tone.z, 1.0];
        for t in RISE {
            let half = across * wide * 0.5 * (1.0 - t).powf(0.6);
            push(centre(t) - half, t, grass);
            push(centre(t) + half, t, grass);
        }
        push(centre(1.0), 1.0, grass);
        for k in 0..3 {
            let a = first + k * 2;
            indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
        }
        indices.extend([first + 6, first + 7, first + 8]);

        // The tallest blades of a tuft in flower carry a head: a few
        // petals about the tip, facing up.
        if flowering && n % 6 == 0 && out < 0.6 {
            let head = centre(1.0);
            let size = 0.035 * wider.sqrt() * (0.8 + 0.4 * r(i, 10));
            let first = positions.len() as u32;
            let flower = [1.0, 1.0, 1.0, 0.0];
            let mut petal = |p: Vec3| {
                positions.push(p.to_array());
                normals.push([0.0, 1.0, 0.0]);
                uvs.push([1.0, own]);
                colours.push(flower);
            };
            petal(head);
            for k in 0..5 {
                let a = k as f32 / 5.0 * TAU + turn;
                petal(
                    head + Vec3::new(a.cos(), 0.15, a.sin()) * size,
                );
            }
            for k in 0..5 {
                indices.extend([
                    first,
                    first + 1 + k,
                    first + 1 + (k + 1) % 5,
                ]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
    .with_inserted_indices(Indices::U32(indices))
}

/// Tells the grass the time, the wind, how far into the night it is,
/// as the leaves are told, and its settings.
fn sway(
    now: Res<Now>,
    set: Res<GrassSettings>,
    wind: Res<WindSettings>,
    view: Res<LeafView>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let blowing = wind.uniform(now.0);
    for (_, grass) in materials.iter_mut() {
        let was = &grass.extension.grass;
        let next = GrassUniform::new(
            &set,
            now.0,
            view.night,
            was.reach,
            was.mask_at,
            was.flowers,
        );
        if *was != next {
            grass.extension.grass = next;
        }
        if grass.extension.wind != blowing {
            grass.extension.wind = blowing.clone();
        }
    }
}

/// A number from 0 to 1 that follows from `i` and `k` alone.
pub fn hash(i: u32, k: u32) -> f32 {
    let mut h = i
        .wrapping_mul(0x8da6_b343)
        .wrapping_add(k.wrapping_mul(0xd816_3841));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h & 0xff_ffff) as f32 / 16_777_216.0
}
