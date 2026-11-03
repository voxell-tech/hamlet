//! Grass: a meadow of tufts about the eye. Each tuft is one of a few
//! shapes made at random when the grass is made, a clump of curved
//! blades leaning out from its middle, at each of a few details. The
//! GPU does the rest, in `draw`: a pass looks at every place a tuft
//! might stand about the eye, keeps those that grow there, are near
//! enough to keep and in sight, and lists each under its shape and
//! the detail its distance calls for; then each list is drawn as many
//! times as it holds. A tuft takes the ground's own colour, darker at
//! its roots and lighter to its tips, and is lit as the ground under
//! it is, so that tufts crowding one another read as one soft
//! surface. None grows on whatever a mask marks trodden, it shrinks
//! toward the edge of the eye's reach, and it bends in the wind's
//! gusts.

mod draw;

use std::f32::consts::TAU;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::extract_resource::{
    ExtractResource, ExtractResourcePlugin,
};
use bevy::render::render_resource::{
    Extent3d, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::load_shader_library;

use crate::ground::{PagesUniform, SoilUniform, blank_pages};
use crate::leaves::LeafView;
use crate::wind::{WindSettings, WindUniform};
use crate::{Now, VfxSystems};

/// Shapes of tuft there are, and how many of the first carry flowers'
/// heads, which show where a tuft flowers. `SHAPES` in
/// `grass_shape.wesl`.
pub const SHAPES: usize = 8;
pub const IN_FLOWER: usize = 4;
/// Details each tuft is made in, the finest first. `DETAILS` in
/// `grass_shape.wesl`.
pub const DETAILS: usize = 3;

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "grass_shape.wesl");
    embedded_asset!(app, "grass.wesl");
    embedded_asset!(app, "grass_place.wesl");
    app.add_plugins((
        ExtractResourcePlugin::<Grass>::default(),
        ExtractResourcePlugin::<GrassTufts>::default(),
        draw::plugin,
    ))
    .init_resource::<GrassSettings>()
    .add_systems(Update, (shape, sway).in_set(VfxSystems));
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
    /// Blades in a tuft at each detail, the finest first, and the
    /// share of the places a tuft might grow that keep one at each.
    pub blades: [u32; 3],
    pub kept: [f32; 3],
    /// How far apart the tufts grow, in metres, how far where the
    /// quality lays them coarser, and the share of the places one
    /// might grow that are left bare.
    pub spacing: f32,
    pub coarse_spacing: f32,
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
    /// How brightly the grass in and about a patch of flowers glows
    /// in their colour, against the flowers themselves.
    pub halo: f32,
    /// How far from the eye each detail reaches, the finest first,
    /// where the eye stands low. The grass shortens to nothing over
    /// the last half of the last, so the furthest blades, too small
    /// to draw steadily, lie low.
    pub rings: [f32; 3],
}

/// What the grass is drawn from, as the world last told it: the
/// GPU's to read each frame, as `grass_shape.wesl` binds it.
#[derive(Resource, ExtractResource, Clone)]
#[extract_app(RenderApp)]
pub struct Grass {
    pub grass: GrassUniform,
    pub wind: WindUniform,
    pub soil: SoilUniform,
    /// The land's pages, as the ground has them.
    pub pages: Handle<Image>,
    pub pages_at: PagesUniform,
    /// How trodden the ground is about the eye, from 0 where grass
    /// grows to 1 where none does, laid as `mask_at` in
    /// [`GrassUniform`] has it.
    pub mask: Handle<Image>,
    /// The planes the eye sees between, each as its normal and its
    /// distance from the origin, the inside where the sum is over
    /// nothing; a tuft wholly outside one is not drawn.
    pub sight: [Vec4; 6],
    /// Whether it is drawn at all.
    pub shown: bool,
}

impl Grass {
    /// The grass for `set`, painted with `soil`, its pages, mask and
    /// sight blank until the world gives them.
    pub fn new(
        images: &mut Assets<Image>,
        set: &GrassSettings,
        soil: SoilUniform,
    ) -> Self {
        Self {
            grass: GrassUniform::new(
                set,
                0.0,
                0.0,
                GrassReach::default(),
                Vec4::ZERO,
                [Vec4::ONE; 5],
            ),
            wind: WindUniform::default(),
            soil,
            pages: images.add(blank_pages()),
            pages_at: PagesUniform::default(),
            mask: images.add(mask(1)),
            sight: [Vec4::ZERO; 6],
            shown: true,
        }
    }
}

/// The tufts each list of them is drawn as: every shape at every
/// detail, the finest first, [`SHAPES`] to a detail.
#[derive(Resource, ExtractResource, Clone)]
#[extract_app(RenderApp)]
pub struct GrassTufts(pub Vec<Mesh>);

/// Mirrors `Grass` in `grass_shape.wesl`; the fields are explained
/// there.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct GrassUniform {
    pub grow: Vec4,
    pub size: Vec4,
    pub look: Vec4,
    pub sway: Vec4,
    pub glow: Vec4,
    pub thin: Vec4,
    pub reach: Vec4,
    pub rings: Vec4,
    pub mask_at: Vec4,
    pub flowers: [Vec4; 5],
}

/// Where the grass is drawn about the eye, as the world lays it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GrassReach {
    /// The eye.
    pub eye: Vec3,
    /// How high the eye stands over the ground under it.
    pub low: f32,
    /// How far from the eye each detail reaches.
    pub rings: [f32; DETAILS],
    /// The share of the tufts the settings keep past the finest
    /// detail that are drawn.
    pub kept: f32,
    /// How far apart the tufts are laid, in place of `spacing`.
    pub spacing: f32,
}

impl GrassUniform {
    /// The uniform for `set`, at the time `now`, `night` of the way
    /// into the night, drawn as far as `reach` has it, with the mask
    /// laid at `mask_at` and the flowers in `flowers`.
    pub fn new(
        set: &GrassSettings,
        now: f32,
        night: f32,
        reach: GrassReach,
        mask_at: Vec4,
        flowers: [Vec4; 5],
    ) -> Self {
        let finest = set.blades[0].max(1) as f32;
        let share = |d: usize| set.blades[d] as f32 / finest;
        let mut uniform = Self {
            thin: Vec4::new(
                set.kept[1],
                set.kept[2],
                share(1),
                share(2),
            ),
            reach: Vec4::ZERO,
            rings: Vec4::ZERO,
            grow: Vec4::new(set.spacing, set.bare, set.flowers, 0.0),
            size: Vec4::new(
                set.across.0,
                set.across.1,
                set.up.0,
                set.up.1,
            ),
            look: Vec4::new(set.root, set.tip, set.sun, set.sheen),
            sway: Vec4::new(set.bend, now, 0.0, 0.0),
            glow: Vec4::new(
                set.glow * night,
                1.0 / set.pulse.0.max(1e-3),
                set.pulse.1,
                set.halo,
            ),
            mask_at,
            flowers,
        };
        uniform.set_reach(reach);
        uniform
    }

    /// Draws the grass as far as `reach` has it.
    pub fn set_reach(&mut self, reach: GrassReach) {
        self.reach = reach.eye.extend(reach.rings[2]);
        self.rings = Vec4::new(
            reach.rings[0],
            reach.rings[1],
            reach.low,
            reach.kept,
        );
        self.grow.x = reach.spacing;
    }

    /// Where the grass is drawn, as this has it.
    pub fn reach(&self) -> GrassReach {
        GrassReach {
            eye: self.reach.xyz(),
            low: self.rings.z,
            rings: [self.rings.x, self.rings.y, self.reach.w],
            kept: self.rings.w,
            spacing: self.grow.x,
        }
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

/// Every tuft the lists are drawn as, as [`GrassTufts`] holds them,
/// each standing on the origin for the shader to place and thin.
/// Each blade has a rank, from 0 to 1; the shader keeps fewer of
/// them the further off they are, the lowest ranked longest. A
/// coarser detail holds only those the shader would keep that far
/// off, so that a tuft drawn coarser shows nothing new.
///
/// A point's UV is how far up its blade it is, from 0 to 1, and its
/// blade's rank, which its tone follows. A blade's points lie on its
/// middle line, their colours how far across it each lies, with alpha
/// 1; a flower's head's points all lie at its middle, their colours
/// how far out of it each lies, with alpha 0. So the shader can widen
/// blades and heads alike.
pub fn tufts(set: &GrassSettings) -> Vec<Mesh> {
    (0..DETAILS)
        .flat_map(|detail| {
            (0..SHAPES).map(move |shape| {
                let mut built = Built::default();
                tuft(&mut built, set, shape, detail);
                Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::RENDER_WORLD,
                )
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    built.positions,
                )
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_NORMAL,
                    built.normals,
                )
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_UV_0,
                    built.uvs,
                )
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_COLOR,
                    built.colours,
                )
                .with_inserted_indices(Indices::U32(built.indices))
            })
        })
        .collect()
}

/// A tuft's points as they are built.
#[derive(Default)]
struct Built {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colours: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// Tuft `shape` of [`SHAPES`] at `detail`, made at random from its
/// number alone: a clump of curved blades, the outer ones shorter and
/// leaning out further, and the first [`IN_FLOWER`] shapes with
/// flowers' heads on their tallest blades. Only the blades ranked
/// under `detail`'s share of the finest's are made.
fn tuft(
    built: &mut Built,
    set: &GrassSettings,
    shape: usize,
    detail: usize,
) {
    /// How far up each pair of points across a blade lies; its tip
    /// is one point.
    const RISE: [f32; 4] = [0.0, 0.3, 0.58, 0.82];
    let seed = shape as u32 * 977 + 13;
    let r = |i: u32, k: u32| hash(seed.wrapping_add(i * 7919), k);
    let finest = set.blades[0].max(1);
    let share =
        set.blades[detail].clamp(1, finest) as f32 / finest as f32;
    let flowering = shape < IN_FLOWER;

    let Built {
        positions,
        normals,
        uvs,
        colours,
        indices,
    } = built;
    for i in 0..finest {
        let own = r(i, 9);
        if own >= share {
            continue;
        }
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
        let wide = set.wide * (0.75 + 0.5 * r(i, 6));
        let centre = |t: f32| {
            root + lean_way * (lean * height * t * t)
                + Vec3::Y * height * (t - 0.3 * lean * lean * t * t)
        };
        let first = positions.len() as u32;
        let mut push = |t: f32, off: Vec3| {
            positions.push(centre(t).to_array());
            normals.push([0.0, 1.0, 0.0]);
            uvs.push([t, own]);
            colours.push([off.x, off.y, off.z, 1.0]);
        };
        for t in RISE {
            let half = across * wide * 0.5 * (1.0 - t).powf(0.6);
            push(t, -half);
            push(t, half);
        }
        push(1.0, Vec3::ZERO);
        for k in 0..3 {
            let a = first + k * 2;
            indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
        }
        indices.extend([first + 6, first + 7, first + 8]);

        // The tallest blades of a tuft in flower carry a head: a few
        // petals about the tip, facing up. The coarsest detail, too
        // far off for heads, has none.
        if flowering
            && detail + 1 < DETAILS
            && i % 6 == 0
            && out < 0.6
        {
            let head = centre(1.0);
            let size = 0.035 * (0.8 + 0.4 * r(i, 10));
            // Each head faces its own way, tipped up to 70 degrees
            // from the sky about an axis of its own.
            let tip = Quat::from_axis_angle(
                Vec3::new(r(i, 11) - 0.5, 0.0, r(i, 12) - 0.5)
                    .normalize_or(Vec3::X),
                r(i, 13) * 1.2,
            );
            let first = positions.len() as u32;
            let mut petal = |out: Vec3| {
                positions.push(head.to_array());
                normals.push([0.0, 1.0, 0.0]);
                uvs.push([1.0, own]);
                colours.push([out.x, out.y, out.z, 0.0]);
            };
            petal(Vec3::ZERO);
            for k in 0..5 {
                let a = k as f32 / 5.0 * TAU + turn;
                petal(tip * Vec3::new(a.cos(), 0.15, a.sin()) * size);
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
}

/// Makes the tufts anew when the settings change.
fn shape(
    mut commands: Commands,
    set: Res<GrassSettings>,
    made: Option<Res<GrassTufts>>,
) {
    if made.is_none() || set.is_changed() {
        commands.insert_resource(GrassTufts(tufts(&set)));
    }
}

/// Tells the grass the time, the wind, how far into the night it is,
/// as the leaves are told, and its settings.
fn sway(
    now: Res<Now>,
    set: Res<GrassSettings>,
    wind: Res<WindSettings>,
    view: Res<LeafView>,
    grass: Option<ResMut<Grass>>,
) {
    let Some(mut grass) = grass else {
        return;
    };
    let was = &grass.grass;
    let next = GrassUniform::new(
        &set,
        now.0,
        view.night,
        was.reach(),
        was.mask_at,
        was.flowers,
    );
    grass.grass = next;
    grass.wind = wind.uniform(now.0);
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
