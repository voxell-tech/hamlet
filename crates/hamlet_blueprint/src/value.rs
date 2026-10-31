//! What travels on a wire.

use bevy_math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::field::Field;

/// The type of a port, and of what a wire carries.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum ValueType {
    Number,
    /// A whole number, such as a count.
    Integer,
    Bool,
    Point,
    Choice,
    Curve,
    /// Where something stands, how it is turned, and its scale.
    Transform,
    Field,
    Objects,
}

impl ValueType {
    /// Whether a port of this type takes a wire carrying `from`. A
    /// number or bool port reads a field where each transform is; a
    /// field port takes a number as the same value everywhere.
    pub fn accepts(self, from: ValueType) -> bool {
        self == from
            || matches!(
                (self, from),
                (
                    ValueType::Number | ValueType::Bool,
                    ValueType::Field
                ) | (
                    ValueType::Field | ValueType::Number,
                    ValueType::Number | ValueType::Integer
                ) | (
                    ValueType::Integer,
                    ValueType::Number | ValueType::Field
                )
            )
    }
}

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
    Transform(Box<TransformValue>),
    Field(Field),
    Objects(Vec<Object>),
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
            Value::Transform(_) => ValueType::Transform,
            Value::Field(_) => ValueType::Field,
            Value::Objects(_) => ValueType::Objects,
        }
    }
}

/// A value written in a graph's file.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Constant {
    Number(f32),
    Integer(i32),
    Bool(bool),
    Point(Vec3),
    Choice(String),
}

impl Constant {
    pub fn value_type(&self) -> ValueType {
        match self {
            Constant::Number(_) => ValueType::Number,
            Constant::Integer(_) => ValueType::Integer,
            Constant::Bool(_) => ValueType::Bool,
            Constant::Point(_) => ValueType::Point,
            Constant::Choice(_) => ValueType::Choice,
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

/// Points joined in order by straight edges, open or closed.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Curve {
    pub points: Vec<Vec3>,
    pub closed: bool,
}

impl Curve {
    /// Each edge's two ends, in order.
    pub fn edges(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        let count = self.points.len();
        let edges = if self.closed && count > 2 {
            count
        } else {
            count.saturating_sub(1)
        };
        (0..edges).map(move |i| {
            (self.points[i], self.points[(i + 1) % count])
        })
    }

    pub fn length(&self) -> f32 {
        self.edges().map(|(a, b)| a.distance(b)).sum()
    }

    /// The point `distance` along the curve and the direction of the
    /// edge it is on. A point on a corner is on the edge that starts
    /// there. A closed curve wraps; an open one has nothing past its
    /// end.
    pub fn at(&self, distance: f32) -> Option<(Vec3, Vec3)> {
        let length = self.length();
        if length <= 0.0 {
            return None;
        }
        let mut left = if self.closed {
            distance.rem_euclid(length)
        } else if (0.0..=length).contains(&distance) {
            distance
        } else {
            return None;
        };
        let mut last = None;
        for (a, b) in self.edges() {
            let edge = a.distance(b);
            let direction = (b - a).normalize_or_zero();
            if left < edge {
                return Some((a + direction * left, direction));
            }
            left -= edge;
            last = Some((b, direction));
        }
        last
    }
}
