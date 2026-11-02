//! Grass: a thick field of blades about the eye, each grown on the
//! GPU from the land's pages where it stands, so the CPU lays only
//! square patches of it. A blade takes the ground's own colour at
//! its root, lighter to its tip, and is lit as the ground under it
//! is, so the field reads as one soft surface. It thins under the
//! woods, on steep and high ground and at the water, keeps off
//! whatever a mask marks trodden, bends in the wind's gusts, and
//! some in patches flower.

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
use crate::wind::{WindSettings, WindUniform};
use crate::{Now, VfxSystems};

pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassExt>;

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "grass_shape.wesl");
    embedded_asset!(app, "grass.wesl");
    app.add_plugins(MaterialPlugin::<GrassMaterial>::default())
        .init_resource::<GrassSettings>()
        .add_systems(Update, sway.in_set(VfxSystems));
}

/// How the grass grows, takes the light and moves, and how far
/// about the eye it is drawn.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrassSettings {
    /// The shortest and the tallest blade, in metres.
    pub tall: (f32, f32),
    /// How wide a blade is at its root, in metres, where the blades
    /// are thickest.
    pub wide: f32,
    /// How far a blade leans of its own, and how far more in the
    /// full wind, as shares of its height.
    pub lean: f32,
    pub bend: f32,
    /// What the ground's colour is multiplied by at a blade's root
    /// and at its tip, and the share of the way its tip turns to the
    /// sunlit grass's colour.
    pub root: f32,
    pub tip: f32,
    pub sun: f32,
    /// How bright the tips glow with the sun behind them.
    pub glow: f32,
    /// The share of blades that flower where flowers grow.
    pub flowers: f32,
    /// How far apart the clumps blades grow in are, in metres, the
    /// share of the way each blade's root is drawn to its clump's
    /// middle, and how far it splays out from there, as a share of
    /// its height.
    pub clump: f32,
    pub gather: f32,
    pub splay: f32,
    /// The side of a patch, in metres.
    pub patch: f32,
    /// How far from the eye each ring of patches reaches, nearest
    /// first, where the eye stands low, and how many blades a patch
    /// of each has. The last ring's blades shorten to nothing toward
    /// its edge.
    pub rings: [f32; 3],
    pub blades: [u32; 3],
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
    pub shape: Vec4,
    pub look: Vec4,
    pub sway: Vec4,
    pub clumps: Vec4,
    pub reach: Vec4,
    pub mask_at: Vec4,
    pub flowers: [Vec4; 5],
}

impl GrassUniform {
    /// The uniform for `set`, at the time `now`, drawn as far as
    /// `reach` has it, with the mask laid at `mask_at` and the
    /// flowers in `flowers`.
    pub fn new(
        set: &GrassSettings,
        now: f32,
        reach: Vec4,
        mask_at: Vec4,
        flowers: [Vec4; 5],
    ) -> Self {
        Self {
            shape: Vec4::new(
                set.tall.0, set.tall.1, set.wide, set.lean,
            ),
            look: Vec4::new(set.root, set.tip, set.sun, set.glow),
            sway: Vec4::new(set.bend, now, set.flowers, 0.0),
            clumps: Vec4::new(set.clump, set.gather, set.splay, 0.0),
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

    // The blades are thin enough that the prepass and the main pass,
    // each placing them in its own shader, can disagree on a pixel's
    // depth by a hair; the main pass then draws nothing there. So the
    // grass is left out of the prepass, and casts no shadow.
    fn enable_prepass() -> bool {
        false
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
            perceptual_roughness: 0.85,
            reflectance: 0.2,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: GrassExt {
            grass: GrassUniform::new(
                set,
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

/// A patch `side` across of `count` blades, spread evenly over it,
/// its corner at the origin. A blade is laid flat: x and z are its
/// root, y how far up it a point is, from 0 to 1; the UV's x is the
/// side of the blade, from -1 to 1, and its colour four numbers of
/// its own, from 0 to 1, that the shader grows it by.
pub fn patch(count: u32, side: f32) -> Mesh {
    /// How far up each pair of points across a blade lies; its tip
    /// is one point.
    const RISE: [f32; 3] = [0.0, 0.35, 0.7];
    let across = (count as f32).sqrt().ceil() as u32;
    let cell = side / across as f32;
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut colours = Vec::new();
    let mut indices = Vec::new();
    for i in 0..across * across {
        let r = |k: u32| hash(i, k);
        let root =
            (Vec2::new((i % across) as f32, (i / across) as f32)
                + Vec2::new(r(0), r(1)))
                * cell;
        let own = [r(2), r(3), r(4), r(5)];
        let first = positions.len() as u32;
        let mut push = |y: f32, x: f32| {
            positions.push([root.x, y, root.y]);
            uvs.push([x, 0.0]);
            colours.push(own);
        };
        for y in RISE {
            push(y, -1.0);
            push(y, 1.0);
        }
        push(1.0, 0.0);
        for k in 0..2 {
            let a = first + k * 2;
            indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
        }
        indices.extend([first + 4, first + 5, first + 6]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
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

/// Tells the grass the time, the wind and its settings.
fn sway(
    now: Res<Now>,
    set: Res<GrassSettings>,
    wind: Res<WindSettings>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let blowing = wind.uniform(now.0);
    for (_, grass) in materials.iter_mut() {
        let was = &grass.extension.grass;
        let next = GrassUniform::new(
            &set,
            now.0,
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
fn hash(i: u32, k: u32) -> f32 {
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
