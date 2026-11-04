//! The types a port takes and the constants a graph's file holds.

use bevy_math::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

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
    /// A list of points with a tangent handle on each side, such as
    /// the points a spline is drawn through.
    Path,
    /// Where something stands, how it is turned, and its scale.
    Transform,
    Field,
    Objects,
    /// Two numbers on a material's surface, such as where it is in
    /// its texture.
    Vector2,
    /// Three numbers on a material's surface, such as a direction.
    Vector3,
    /// Red, green, blue and how opaque, each from 0 to 1.
    Color,
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
            // A material widens or narrows between numbers, vectors
            // and colours, filling or dropping the last axes.
            || (self.on_surface() && from.on_surface())
    }

    /// Whether a material's surface works out values of this type.
    pub fn on_surface(self) -> bool {
        matches!(
            self,
            ValueType::Number
                | ValueType::Vector2
                | ValueType::Vector3
                | ValueType::Color
        )
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
    Color(Vec4),
    Path(Vec<PathPoint>),
}

/// A point of a path, and the offsets from it to the ends of its two
/// tangent handles.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct PathPoint {
    pub at: Vec3,
    /// The handle on the side the path arrives from.
    #[serde(default)]
    pub before: Vec3,
    /// The handle on the side the path leaves by.
    #[serde(default)]
    pub after: Vec3,
}

impl PathPoint {
    pub const fn new(at: Vec3, before: Vec3, after: Vec3) -> Self {
        Self { at, before, after }
    }
}

impl Constant {
    pub fn value_type(&self) -> ValueType {
        match self {
            Constant::Number(_) => ValueType::Number,
            Constant::Integer(_) => ValueType::Integer,
            Constant::Bool(_) => ValueType::Bool,
            Constant::Point(_) => ValueType::Point,
            Constant::Choice(_) => ValueType::Choice,
            Constant::Color(_) => ValueType::Color,
            Constant::Path(_) => ValueType::Path,
        }
    }
}
