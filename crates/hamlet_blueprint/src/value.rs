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
    Transforms,
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
    Transforms(Vec<Transform>),
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
            Value::Transforms(_) => ValueType::Transforms,
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

/// An id of a [`Transform`], made from what made it, so it holds
/// while others are added or removed around it. Its random values are
/// seeded from it.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default,
)]
pub struct TransformId(pub u64);

impl TransformId {
    /// The parent of transforms made from nothing.
    pub const ROOT: Self = Self(0);
}

/// A box in a frame, with tags and what made it.
#[derive(Clone, PartialEq, Debug)]
pub struct Transform {
    pub id: TransformId,
    /// Where it is, how it is turned, and its scale, which is the
    /// size of what stands on it.
    pub frame: Frame,
    /// Its place among those made with it, and how many they were.
    pub index: u32,
    pub count: u32,
    /// How far along the curve it was made on, from 0 to 1.
    pub along: f32,
    pub tags: Vec<String>,
}

impl Transform {
    pub fn new(id: TransformId, frame: Frame) -> Self {
        Self {
            id,
            frame,
            index: 0,
            count: 1,
            along: 0.0,
            tags: Vec::new(),
        }
    }
}

/// A named asset put down by a graph.
#[derive(Clone, PartialEq, Debug)]
pub struct Object {
    pub asset: String,
    /// Where it stands, turned, and scaled to its size.
    pub frame: Frame,
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
