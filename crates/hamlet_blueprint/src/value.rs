//! What travels on a wire.

use bevy_math::{Quat, Vec3, Vec4};
use hamlet_graph::{Constant, Initial, PathPoint, ValueType};

use crate::curve::Curve;
use crate::field::Field;

/// A value on a wire.
#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    Number(f32),
    Integer(i32),
    Bool(bool),
    Point(Vec3),
    /// A point whose axes are fields, so it is its own at each
    /// transform it is read at.
    PointField(Box<[Field; 3]>),
    Choice(String),
    Curve(Curve),
    Path(Vec<PathPoint>),
    Transform(Box<TransformValue>),
    Field(Field),
    Objects(Vec<Object>),
    Color(Vec4),
}

impl Value {
    pub fn value_type(&self) -> ValueType {
        match self {
            Value::Number(_) => ValueType::Number,
            Value::Integer(_) => ValueType::Integer,
            Value::Bool(_) => ValueType::Bool,
            Value::Point(_) | Value::PointField(_) => {
                ValueType::Point
            }
            Value::Choice(_) => ValueType::Choice,
            Value::Curve(_) => ValueType::Curve,
            Value::Path(_) => ValueType::Path,
            Value::Transform(_) => ValueType::Transform,
            Value::Field(_) => ValueType::Field,
            Value::Objects(_) => ValueType::Objects,
            Value::Color(_) => ValueType::Color,
        }
    }
}

impl From<Constant> for Value {
    fn from(constant: Constant) -> Self {
        match constant {
            Constant::Number(number) => Value::Number(number),
            Constant::Integer(whole) => Value::Integer(whole),
            Constant::Bool(flag) => Value::Bool(flag),
            Constant::Point(point) => Value::Point(point),
            Constant::Choice(choice) => Value::Choice(choice),
            Constant::Color(color) => Value::Color(color),
            Constant::Path(points) => Value::Path(points),
        }
    }
}

/// Position, turn and scale.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Frame {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Frame {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn at(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    /// Where `local`, given in this frame, is in the frame above.
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.translation + self.rotation * (self.scale * local)
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// An id of an [`Object`], made from what made it, so it holds while
/// others are added or removed around it. Its random values are
/// seeded from it.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default,
)]
pub struct ObjectId(pub u64);

impl ObjectId {
    /// The parent of objects made from nothing.
    pub const ROOT: Self = Self(0);
}

/// An asset put down by a graph, and what made it: its place among
/// the copies made with it, which fields read.
#[derive(Clone, PartialEq, Debug)]
pub struct Object {
    pub asset: String,
    pub id: ObjectId,
    /// Where it stands, turned, and scaled to its size.
    pub frame: Frame,
    /// Its place among the copies made with it, and how many they
    /// were.
    pub index: u32,
    pub count: u32,
    /// How far along the curve it was copied along, from 0 to 1.
    pub along: f32,
    pub tags: Vec<String>,
}

impl Object {
    pub fn new(asset: String, id: ObjectId, frame: Frame) -> Self {
        Self {
            asset,
            id,
            frame,
            index: 0,
            count: 1,
            along: 0.0,
            tags: Vec::new(),
        }
    }

    /// A copy of it moved by `at`, as one with the others `at`
    /// moves: `at`, then the object in `at`'s frame.
    pub fn copied(&self, at: &Frame, id: ObjectId) -> Self {
        let mut copy = self.clone();
        copy.id = id;
        copy.frame = Frame {
            translation: at.point(self.frame.translation),
            rotation: at.rotation * self.frame.rotation,
            scale: at.scale * self.frame.scale,
        };
        copy
    }
}

const ZERO: Field = Field::Constant(0.0);
const ONE: Field = Field::Constant(1.0);

/// A transform whose translation, rotation and scale may each be read
/// from fields, so it is its own at each object it moves.
#[derive(Clone, PartialEq, Debug)]
pub struct TransformValue {
    pub translation: [Field; 3],
    /// Degrees about x, y and z, turned about y first.
    pub rotation: [Field; 3],
    pub scale: [Field; 3],
}

impl TransformValue {
    /// The transform that moves nothing.
    pub const IDENTITY: Self = Self {
        translation: [ZERO, ZERO, ZERO],
        rotation: [ZERO, ZERO, ZERO],
        scale: [ONE, ONE, ONE],
    };

    /// The transform read at `at`.
    pub fn at(&self, at: &Object) -> Frame {
        let axes = |fields: &[Field; 3]| {
            Vec3::from(
                fields.each_ref().map(|field| field.sample(at)),
            )
        };
        let turn = axes(&self.rotation);
        Frame {
            translation: axes(&self.translation),
            rotation: crate::turn::euler(turn.y, turn.x, turn.z),
            scale: axes(&self.scale),
        }
    }
}

impl From<Initial> for Value {
    fn from(initial: Initial) -> Self {
        match initial {
            Initial::Number(number) => Value::Number(number),
            Initial::Integer(whole) => Value::Integer(whole),
            Initial::Bool(flag) => Value::Bool(flag),
            Initial::Point(point) => Value::Point(point),
            Initial::Choice(choice) => {
                Value::Choice(choice.to_owned())
            }
            Initial::Color(color) => Value::Color(color),
            Initial::Path(points) => Value::Path(points.to_vec()),
        }
    }
}
