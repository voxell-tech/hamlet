//! Marks of light on the ground: a ring or a rune drawn as a decal
//! (`mark.wesl`), so that it falls on the ground and never on the
//! things that stand there. The camera that draws it needs a depth
//! prepass.

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, Extent3d, Face,
    RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "mark.wesl");
    app.add_plugins(MaterialPlugin::<MarkMaterial>::default())
        .add_systems(PreStartup, draw);
}

/// The material of a mark.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct MarkMaterial {
    #[uniform(0)]
    mark: MarkUniform,
    #[texture(1, sample_type = "float", filterable = false)]
    heights: Handle<Image>,
}

/// Mirrors `Mark` in `mark.wesl`; the fields are explained there.
#[derive(ShaderType, Debug, Clone)]
struct MarkUniform {
    light: Vec4,
    field: Vec4,
    shape: Vec4,
    near: f32,
}

/// The outline a mark draws. It is worked out in the shader, so it
/// is as sharp close up as from far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Number of its sides. None for a ring.
    pub sides: u32,
    /// The sizes its line lies between, where a shape of size 1
    /// touches the sides of the mark's square.
    pub inner: f32,
    pub outer: f32,
}

impl Shape {
    /// A thin band, for what is pointed at or chosen.
    pub const BAND: Self = Self::of(0, 0.94, 0.985);
    /// A ring, a square on its corner and a six-sided rune, for
    /// circles of runes.
    pub const RING: Self = Self::of(0, 0.9, 0.985);
    pub const DIAMOND: Self = Self::of(4, 0.56, 0.64);
    pub(crate) const HEX: Self = Self::of(6, 0.62, 0.7);

    const fn of(sides: u32, inner: f32, outer: f32) -> Self {
        Self {
            sides,
            inner,
            outer,
        }
    }
}

impl Material for MarkMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_vfx/mark.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // It is light: it adds to the ground and fades by dimming.
        AlphaMode::Add
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The box is drawn wherever it stands, under the ground or
        // over it: the shader finds what it falls on. Only its far
        // sides are drawn, which cover all of it on the screen once,
        // from outside it and from within.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Always);
        }
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

impl MarkMaterial {
    /// Sets the light it shines in, in linear RGB.
    pub fn light(&mut self, light: Vec3) {
        self.mark.light = light.extend(1.0);
    }

    /// The picture of the heights it was laid with.
    pub fn heights(&self) -> &Handle<Image> {
        &self.heights
    }

    /// Lays it on other ground.
    pub fn lay(&mut self, lay: &Lay) {
        self.mark.field =
            Vec4::new(lay.centre.x, lay.centre.y, lay.half, lay.base);
        self.mark.near = lay.near;
        self.heights = lay.heights.clone();
    }
}

/// The ground a mark is laid on: how high it is about `centre`, as
/// far as `half` to every side.
#[derive(Clone, Debug)]
pub struct Lay {
    /// The middle, as x and z.
    pub centre: Vec2,
    pub half: f32,
    /// The height that 0 in `heights` stands for.
    pub base: f32,
    /// A picture of the ground's height over `base`, from
    /// [`heights`].
    pub heights: Handle<Image>,
    /// Distance over or under the ground that still counts as the
    /// ground.
    pub near: f32,
}

/// A picture, `side` points each way, of the heights `at` gives for
/// the points of a square: each is asked for by how far it is from
/// the square's middle, as a share of half its width.
pub fn heights(side: usize, at: impl Fn(Vec2) -> f32) -> Image {
    let mut data = Vec::with_capacity(side * side * 4);
    for y in 0..side {
        for x in 0..side {
            let from = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
                / side as f32
                - 0.5)
                * 2.0;
            data.extend(at(from).to_le_bytes());
        }
    }
    Image::new(
        Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::R32Float,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// The things every mark shares: the box it is drawn through and the
/// heights of level ground.
#[derive(Resource, Clone, Default)]
pub struct Marks {
    /// One unit a side, about its own middle.
    square: Handle<Mesh>,
    level: Handle<Image>,
}

fn draw(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    // A box and not a flat square: from a low angle, ground that
    // lies higher or lower than a square would be seen past its
    // edge, and the mark would be cut off there.
    commands.insert_resource(Marks {
        square: meshes.add(Cuboid::from_length(1.0)),
        level: images.add(heights(1, |_| 0.0)),
    });
}

impl Marks {
    /// True when `heights` is the level ground every mark begins on.
    pub fn is_level(&self, heights: &Handle<Image>) -> bool {
        *heights == self.level
    }

    /// Level ground at the height of `at`, for a mark where nothing
    /// is known of the ground.
    pub fn level(&self, at: Vec3, near: f32) -> Lay {
        Lay {
            centre: at.xz(),
            half: 1.0,
            base: at.y,
            heights: self.level.clone(),
            near,
        }
    }

    /// The material of a mark of `shape` that shines in `light`, on
    /// the ground `lay` tells of.
    pub fn shine(
        &self,
        shape: Shape,
        light: Vec3,
        lay: &Lay,
    ) -> MarkMaterial {
        let mut material = MarkMaterial {
            mark: MarkUniform {
                light: light.extend(1.0),
                field: Vec4::ZERO,
                shape: Vec4::new(
                    shape.sides as f32,
                    shape.inner,
                    shape.outer,
                    0.0,
                ),
                near: 0.0,
            },
            heights: self.level.clone(),
        };
        material.lay(lay);
        material
    }

    /// A mark of `material`. Its transform says where it lies, how
    /// far across it is and how it is turned about the upright.
    pub fn mark(
        &self,
        material: Handle<MarkMaterial>,
    ) -> impl Bundle {
        (
            Mesh3d(self.square.clone()),
            MeshMaterial3d(material),
            NotShadowCaster,
        )
    }
}

/// The transform of a mark at `at`, `radius` across from its
/// middle, turned by `turn`.
pub fn lie(at: Vec3, radius: f32, turn: f32) -> Transform {
    // Tall enough to hold the ground under all of it, steps and a
    // slope across a wide mark too.
    let tall = TALL + radius;
    Transform::from_translation(at)
        .with_rotation(Quat::from_rotation_y(turn))
        .with_scale(Vec3::new(radius * 2.0, tall, radius * 2.0))
}

/// Height of the box of the smallest mark.
const TALL: f32 = 2.5;
