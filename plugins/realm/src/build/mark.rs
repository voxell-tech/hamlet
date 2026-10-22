//! Marks of light on the ground: a ring or a rune drawn by the
//! realm's own decal shader (`mark.wgsl`), laid on the ground about
//! it and never on what stands there.

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_sdk::scene::gpu::{
    Buffer, Shader, ShaderKind, set_params, shaded_decal,
};
use hamlet_engine_sdk::scene::{AlphaMode, Entity, Material};

use crate::build::frame::Frame;
use crate::build::upload::Upload;
use crate::folk::keeper::Floor;

/// Points along each side of the heights a mark is laid with.
const SIDE: usize = 48;

/// Farthest over or under the ground a mark still falls: over a path
/// and the flags of a yard, and no higher.
const NEAR: f32 = 0.16;

/// Height of the box of the smallest mark, which a mark's radius
/// adds to, so that it holds the ground under all of it, steps and a
/// slope across a wide mark too.
const TALL: f32 = 2.5;

/// The size the outer edge of a band lies at.
const BAND_OUTER: f32 = 0.985;

/// The shader every mark is drawn by.
pub fn shader() -> Result<Shader, String> {
    Shader::compile(ShaderKind::Decal, include_str!("mark.wgsl"))
}

/// The outline a mark draws.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shape {
    /// Number of its sides, none for a ring.
    sides: u32,
    /// The sizes its line lies between, where a shape of size 1
    /// touches the sides of the mark's square.
    inner: f32,
    outer: f32,
}

impl Shape {
    /// A ring and a square on its corner, for a circle of runes.
    pub const RING: Self = Self::of(0, 0.9, 0.985);
    pub const DIAMOND: Self = Self::of(4, 0.56, 0.64);

    const fn of(sides: u32, inner: f32, outer: f32) -> Self {
        Self {
            sides,
            inner,
            outer,
        }
    }

    /// A ring `width` wide in the world's units, just within the edge
    /// of a mark `radius` from its middle to its sides.
    pub fn band(radius: f32, width: f32) -> Self {
        let inner = (BAND_OUTER - width / radius.max(1e-3)).max(0.0);
        Self::of(0, inner, BAND_OUTER)
    }
}

/// A mark of light at an entity of its own, shown once it has the
/// heights of the ground it is laid on.
pub struct Mark {
    entity: Entity,
    material: Material,
    heights: Buffer,
    params: [f32; 16],
    /// The middle of the ground its heights are of, and how far they
    /// reach to each side.
    ground: Option<(Vec2, f32)>,
    /// Heights of other ground and the params that tell of it, until
    /// they are handed over.
    waiting: Option<(Vec<u8>, [f32; 4])>,
    wanted: bool,
    shown: bool,
}

impl Mark {
    /// A mark of the key `key` and the outline `shape`, drawn by
    /// `shader`, not shown.
    pub fn make(
        key: u32,
        shader: &Shader,
        shape: Shape,
    ) -> Result<Self, String> {
        let entity = Entity::make(None, key)?;
        entity.set_visible(false);
        let heights = Buffer::make((SIDE * SIDE * 4) as u32)?;
        let mut params = [0.0; 16];
        params[12] = SIDE as f32;
        params[8..12].copy_from_slice(&shape_params(shape));
        let material = shaded_decal(
            shader,
            AlphaMode::Add,
            &params,
            None,
            Some(&heights),
        )?;
        entity.draw_decal(&material);
        Ok(Self {
            entity,
            material,
            heights,
            params,
            ground: None,
            waiting: None,
            wanted: false,
            shown: false,
        })
    }

    /// Lays it at `at`, `radius` from its middle to the sides of its
    /// square, turned by `turn` about the upright.
    pub fn lay(&self, at: Vec3, radius: f32, turn: f32) {
        self.entity.set_transform(
            Frame::at(at)
                .with_turn(Quat::from_rotation_y(turn))
                .with_size(Vec3::new(
                    radius * 2.0,
                    TALL + radius,
                    radius * 2.0,
                ))
                .scene(),
        );
    }

    /// Has it draw `shape`.
    pub fn shape(&mut self, shape: Shape) {
        self.set(8, shape_params(shape));
    }

    /// Has it shine `light`, in linear RGB, `strength` strong.
    pub fn shine(&mut self, light: Vec3, strength: f32) {
        self.set(0, light.extend(strength).to_array());
    }

    pub fn show(&mut self, shown: bool) {
        self.wanted = shown;
        self.reveal();
    }

    /// True when the ground it has, or waits for, reaches `reach`
    /// about `middle`.
    pub fn holds(&self, middle: Vec2, reach: f32) -> bool {
        let field = match &self.waiting {
            Some((_, field)) => {
                Some((Vec2::new(field[0], field[1]), field[2]))
            }
            None => self.ground,
        };
        field.is_some_and(|(at, half)| {
            (middle - at).abs().max_element() + reach <= half
        })
    }

    /// Looks up the ground about `at`, `half` to each side, on
    /// `floor`, to be handed over with [`Self::hand_over`]. Until it
    /// is, the mark is not shown.
    pub fn settle(&mut self, at: Vec3, half: f32, floor: &Floor) {
        let middle = at.xz();
        let mut near = floor.near(middle, half);
        let mut bytes = Vec::with_capacity(SIDE * SIDE * 4);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let from =
                    (Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
                        / SIDE as f32
                        - 0.5)
                        * 2.0;
                let height = near.at(middle + from * half) - at.y;
                bytes.extend(height.to_le_bytes());
            }
        }
        self.waiting =
            Some((bytes, [middle.x, middle.y, half, at.y]));
        self.reveal();
    }

    /// Hands over the heights it waits for, if the call has room for
    /// them.
    pub fn hand_over(&mut self, upload: &mut Upload) {
        let Some((bytes, field)) = &self.waiting else {
            return;
        };
        match upload.write(&self.heights, bytes) {
            Ok(true) => {}
            Ok(false) => return,
            Err(why) => {
                hamlet_engine_sdk::warn(&format!("a mark: {why}"));
                return;
            }
        }
        let field = *field;
        self.ground = Some((Vec2::new(field[0], field[1]), field[2]));
        self.waiting = None;
        self.set(4, field);
        self.reveal();
    }

    /// Sets the four params from `at`.
    fn set(&mut self, at: usize, four: [f32; 4]) {
        if self.params[at..at + 4] == four {
            return;
        }
        self.params[at..at + 4].copy_from_slice(&four);
        if let Err(why) = set_params(&self.material, &self.params) {
            hamlet_engine_sdk::warn(&format!("a mark: {why}"));
        }
    }

    /// Shows it while it is wanted and has its ground.
    fn reveal(&mut self) {
        let shown = self.wanted
            && self.waiting.is_none()
            && self.ground.is_some();
        if shown != self.shown {
            self.entity.set_visible(shown);
            self.shown = shown;
        }
    }
}

fn shape_params(shape: Shape) -> [f32; 4] {
    [shape.sides as f32, shape.inner, shape.outer, NEAR]
}
