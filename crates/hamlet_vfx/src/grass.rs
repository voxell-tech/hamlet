//! Grass: a meadow of tufts about the eye. Each tuft is one of a few
//! shapes made at random when the grass is made, a clump of curved
//! blades leaning out from its middle, at each of a few details. The
//! GPU does the rest, in `draw`: a pass checks every place a tuft
//! might stand about the eye, keeps those that grow there, are near
//! enough to keep and in sight, and lists each under its shape and
//! the detail its distance calls for; then each list is drawn as many
//! times as it holds. A tuft takes the ground's own colour, darker at
//! its roots and lighter to its tips, and is shaded with the normal
//! of the ground under it, so that tufts crowding one another read as
//! one soft surface. None grows on whatever a mask marks trodden, it
//! shrinks toward the edge of the eye's reach, and it bends in the
//! wind's gusts.

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

use crate::ground::{
    BloomsUniform, PagesUniform, SoilUniform, blank_pages,
};
use crate::leaves::LeafView;
use crate::wind::{WindSettings, WindUniform};
use crate::{Now, VfxSystems};

/// Shapes of tuft. `SHAPES` in `grass_shape.wesl`.
pub const SHAPES: usize = 8;
/// The last of the [`SHAPES`] that are flowers, which grow where the
/// grass flowers. `FLOWERS` in `grass_shape.wesl`.
pub const FLOWERS: usize = 2;
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

/// What the grass is drawn from, as the world last told it.
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

/// The pass that lists the tufts to draw, in the render world's
/// graph. Whatever writes the pages it reads them from runs before
/// it, or a frame's tufts are found where the land was and drawn
/// where it is.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GrassPlaceSystems;

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
    pub shade: Vec4,
    pub sway: Vec4,
    pub glow: Vec4,
    pub thin: Vec4,
    pub reach: Vec4,
    pub rings: Vec4,
    pub mask_at: Vec4,
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
    /// The uniform for `set` at the time `now`, `night` of the way
    /// into the night, with the mask laid at `mask_at`.
    pub fn new(
        set: &GrassSettings,
        now: f32,
        night: f32,
        reach: GrassReach,
        mask_at: Vec4,
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
            shade: Vec4::new(set.root, set.tip, set.sun, set.sheen),
            sway: Vec4::new(set.bend, now, 0.0, 0.0),
            glow: Vec4::new(
                set.glow * night,
                set.pulse.0.max(1e-3),
                set.pulse.1,
                set.halo,
            ),
            mask_at,
        };
        uniform.set_reach(reach);
        uniform
    }

    /// The flowers the ground paints where these are too small to
    /// show.
    pub fn blooms(&self) -> BloomsUniform {
        let glows = self.glow.x > 0.0;
        BloomsUniform {
            grow: Vec4::new(
                self.grow.z,
                self.reach.w,
                self.grow.x,
                0.0,
            ),
            glow: self.glow.truncate().extend(if glows {
                self.sway.y
            } else {
                0.0
            }),
        }
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
/// leaning out further; or, for the last [`FLOWERS`] shapes, a few
/// shorter blades about stems that stand over the grass, each with a
/// flower's head. Only the blades ranked under `detail`'s share of
/// the finest's are made. The stems are ranked lowest, so they are
/// kept longest; the coarsest detail, too far off for heads, has
/// none.
fn tuft(
    built: &mut Built,
    set: &GrassSettings,
    shape: usize,
    detail: usize,
) {
    let seed = shape as u32 * 977 + 13;
    let r = |i: u32, k: u32| hash(seed.wrapping_add(i * 7919), k);
    let finest = set.blades[0].max(1);
    let share =
        set.blades[detail].clamp(1, finest) as f32 / finest as f32;
    let flower = shape >= SHAPES - FLOWERS;
    // A flower has a third as many blades, and shorter.
    let (blades, short) = if flower {
        (finest / 3, 0.65)
    } else {
        (finest, 1.0)
    };

    for i in 0..blades {
        // A flower's stems are ranked under its blades.
        let rank = if flower { 0.2 + 0.8 * r(i, 9) } else { r(i, 9) };
        if rank >= share {
            continue;
        }
        // Thicker toward the middle.
        let out = r(i, 0).powf(0.7);
        let turn = r(i, 1) * TAU;
        let way = Vec3::new(turn.cos(), 0.0, turn.sin());
        let height = set.tall
            * short
            * (0.5 + 0.5 * r(i, 2))
            * (1.0 - 0.4 * out);
        let side = Vec3::new(-way.z, 0.0, way.x);
        let twist = (r(i, 5) - 0.5) * 1.2;
        built.blade(Blade {
            root: way * out * set.spread * 0.35,
            height,
            // How far it leans out, and a little to one side.
            lean: (0.15 + 0.85 * out) * (0.35 + 0.5 * r(i, 3)),
            lean_way: (way + side * (r(i, 4) - 0.5) * 0.8)
                .normalize(),
            across: (side * twist.cos() + way * twist.sin())
                .normalize(),
            wide: set.wide * (0.75 + 0.5 * r(i, 6)),
            rank,
        });
    }

    if !flower {
        return;
    }
    // Stems near the middle, standing over the grass and leaning a
    // little, each with a head facing up and tipped its own way.
    let stems = 3 + (r(0, 20) * 2.0) as u32;
    for k in 0..stems {
        let i = 100 + k;
        let rank = 0.2 * r(i, 9);
        if rank >= share {
            continue;
        }
        let turn = (k as f32 + r(i, 1) * 0.6) / stems as f32 * TAU;
        let way = Vec3::new(turn.cos(), 0.0, turn.sin());
        let side = Vec3::new(-way.z, 0.0, way.x);
        let tip = built.blade(Blade {
            root: way * r(i, 0) * set.spread * 0.12,
            height: set.tall * (1.05 + 0.35 * r(i, 2)),
            lean: 0.12 + 0.2 * r(i, 3),
            lean_way: way,
            across: side,
            wide: set.wide * 0.45,
            rank,
        });
        if detail + 1 < DETAILS {
            let size = 0.075 * (0.8 + 0.4 * r(i, 10));
            // Tipped up to 35 degrees from the sky about an axis of
            // its own.
            let facing = Quat::from_axis_angle(
                Vec3::new(r(i, 11) - 0.5, 0.0, r(i, 12) - 0.5)
                    .normalize_or(Vec3::X),
                r(i, 13) * 0.6,
            );
            let petals = if detail == 0 { 5 } else { 0 };
            built.head(tip, size, facing, turn, petals, rank);
        }
    }
}

/// A blade as [`Built::blade`] lays it.
struct Blade {
    /// Where it rises from, how high, and how far it leans along
    /// `lean_way`.
    root: Vec3,
    height: f32,
    lean: f32,
    lean_way: Vec3,
    /// The way across it, and how wide it is at its root.
    across: Vec3,
    wide: f32,
    /// Its rank.
    rank: f32,
}

impl Built {
    /// Lays `blade` curving up and out, narrowing to its tip, and
    /// gives back where its tip is.
    fn blade(&mut self, blade: Blade) -> Vec3 {
        /// How far up each pair of points across a blade lies; its
        /// tip is one point.
        const RISE: [f32; 4] = [0.0, 0.3, 0.58, 0.82];
        let Blade {
            root,
            height,
            lean,
            lean_way,
            across,
            wide,
            rank,
        } = blade;
        let centre = |t: f32| {
            root + lean_way * (lean * height * t * t)
                + Vec3::Y * height * (t - 0.3 * lean * lean * t * t)
        };
        let first = self.positions.len() as u32;
        let mut push = |t: f32, off: Vec3| {
            self.positions.push(centre(t).to_array());
            self.normals.push([0.0, 1.0, 0.0]);
            self.uvs.push([t, rank]);
            self.colours.push([off.x, off.y, off.z, 1.0]);
        };
        for t in RISE {
            let half = across * wide * 0.5 * (1.0 - t).powf(0.6);
            push(t, -half);
            push(t, half);
        }
        push(1.0, Vec3::ZERO);
        for k in 0..3 {
            let a = first + k * 2;
            self.indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
        }
        self.indices.extend([first + 6, first + 7, first + 8]);
        centre(1.0)
    }

    /// Lays a flower's head at `at`, `size` across, facing up as
    /// `facing` turns it and turned `turn` about itself: with
    /// `petals` petals, each a point out of the head's middle with a
    /// notch between it and the next, or with none, a plain disc. Its
    /// middle, which is its heart, has no offset at all.
    fn head(
        &mut self,
        at: Vec3,
        size: f32,
        facing: Quat,
        turn: f32,
        petals: u32,
        rank: f32,
    ) {
        let rim = if petals > 0 { petals * 2 } else { 5 };
        let first = self.positions.len() as u32;
        let mut point = |out: Vec3| {
            self.positions.push(at.to_array());
            self.normals.push([0.0, 1.0, 0.0]);
            self.uvs.push([1.0, rank]);
            self.colours.push([out.x, out.y, out.z, 0.0]);
        };
        point(Vec3::ZERO);
        for k in 0..rim {
            let a = k as f32 / rim as f32 * TAU + turn;
            // Petals' tips cup up a little; between them, a notch.
            let (reach, rise) = if petals > 0 && k % 2 == 1 {
                (0.45, 0.05)
            } else {
                (1.0, 0.2)
            };
            point(
                facing
                    * Vec3::new(a.cos(), rise, a.sin())
                    * size
                    * reach,
            );
        }
        for k in 0..rim {
            self.indices.extend([
                first,
                first + 1 + k,
                first + 1 + (k + 1) % rim,
            ]);
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

/// Gives the grass the time, the wind, how far into the night it is
/// and its settings.
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
    let current = &grass.grass;
    let next = GrassUniform::new(
        &set,
        now.0,
        view.night,
        current.reach(),
        current.mask_at,
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
