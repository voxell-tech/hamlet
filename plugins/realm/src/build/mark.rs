//! Marks of light on the ground: a ring or a rune laid as a decal on
//! whatever ground is under it, its outline a picture made here.

use std::f32::consts::TAU;

use glam::{Quat, Vec2, Vec3};
use hamlet_engine_sdk::scene::gpu::{Texture, decal_material};
use hamlet_engine_sdk::scene::{Decal, Entity, Material};

use crate::build::frame::Frame;
use crate::build::mesh::smoothstep;
use crate::build::upload::Upload;

/// How far over or under its square the ground may lie and still
/// take a mark, in the world's units.
const DEPTH_FADE: f32 = 0.75;

/// The outline a mark draws.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shape {
    /// Number of its sides, none for a ring.
    sides: u32,
    /// The sizes its line lies between, where a shape of size 1
    /// touches the sides of the mark's square.
    inner: f32,
    outer: f32,
    /// Pixels of its picture along each side.
    side: u32,
}

impl Shape {
    /// A thin band, for what is pointed at or chosen.
    pub const BAND: Self = Self::of(0, 0.94, 0.985, 256);
    /// A ring and a square on its corner, for a circle of runes.
    pub const RING: Self = Self::of(0, 0.9, 0.985, 128);
    pub const DIAMOND: Self = Self::of(4, 0.56, 0.64, 128);

    const fn of(
        sides: u32,
        inner: f32,
        outer: f32,
        side: u32,
    ) -> Self {
        Self {
            sides,
            inner,
            outer,
            side,
        }
    }

    /// Its picture, handed over within `upload`: none while the call
    /// has no room left for it.
    pub fn upload(
        self,
        upload: &mut Upload,
    ) -> Result<Option<Texture>, String> {
        upload.texture(self.side, || self.picture())
    }

    /// White, as opaque as the outline covers each pixel, its edge
    /// soft over one pixel.
    fn picture(self) -> Vec<u8> {
        let side = self.side as f32;
        let soft = 1.0 / side;
        (0..self.side * self.side)
            .flat_map(|at| {
                let pixel = Vec2::new(
                    (at % self.side) as f32 + 0.5,
                    (at / self.side) as f32 + 0.5,
                );
                let size = self.size(pixel / side * 2.0 - 1.0);
                let covered = smoothstep(
                    self.inner - soft,
                    self.inner + soft,
                    size,
                ) * (1.0
                    - smoothstep(
                        self.outer - soft,
                        self.outer + soft,
                        size,
                    ));
                [255, 255, 255, (covered * 255.0).round() as u8]
            })
            .collect()
    }

    /// How large a shape of this kind is that passes through `p`, a
    /// point of the square from -1 to 1 each way.
    fn size(self, p: Vec2) -> f32 {
        if self.sides < 3 {
            return p.length();
        }
        let part = TAU / self.sides as f32;
        let around = p.y.atan2(p.x);
        let within =
            around - part * (around / part).floor() - part * 0.5;
        p.length() * within.cos() / (part * 0.5).cos()
    }
}

/// A mark of light at an entity of its own, drawn once it is given
/// its picture.
pub struct Mark {
    entity: Entity,
    look: Decal,
    material: Option<Material>,
}

impl Mark {
    /// A mark of the key `key`, not shown.
    pub fn make(key: u32) -> Result<Self, String> {
        let entity = Entity::make(None, key)?;
        entity.set_visible(false);
        Ok(Self {
            entity,
            look: shine(Vec3::ZERO, 0.0),
            material: None,
        })
    }

    /// Draws it with `picture`, a shape's.
    pub fn draw(&mut self, picture: &Texture) -> Result<(), String> {
        let material = decal_material(self.look, picture)?;
        self.entity.draw_decal(&material);
        self.material = Some(material);
        Ok(())
    }

    /// True once it has its picture.
    pub fn drawn(&self) -> bool {
        self.material.is_some()
    }

    /// Lays it at `at`, `radius` from its middle to the sides of its
    /// square, turned by `turn` about the upright.
    pub fn lay(&self, at: Vec3, radius: f32, turn: f32) {
        self.entity.set_transform(
            Frame::at(at)
                .with_turn(Quat::from_rotation_y(turn))
                .with_size(Vec3::new(radius * 2.0, 1.0, radius * 2.0))
                .scene(),
        );
    }

    /// Has it shine `light`, in linear RGB, over `alpha` of the
    /// ground its outline covers.
    pub fn shine(&mut self, light: Vec3, alpha: f32) {
        self.look = shine(light, alpha);
        if let Some(material) = &self.material {
            material.set_decal(self.look);
        }
    }

    pub fn show(&self, shown: bool) {
        self.entity.set_visible(shown);
    }
}

/// The look of a mark that shines `light` over `alpha` of what it
/// covers, neither lit nor fogged.
fn shine(light: Vec3, alpha: f32) -> Decal {
    Decal {
        base_color: (light.x, light.y, light.z, alpha),
        emissive: (0.0, 0.0, 0.0, 1.0),
        unlit: true,
        fog: false,
        depth_fade: DEPTH_FADE,
    }
}
