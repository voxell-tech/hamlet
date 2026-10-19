//! What plugins draw over the world: marks at agents, hamlets or
//! points, which the realm's view draws each frame where their
//! anchors are seen.

use bevy::ecs::resource::Resource;
use bevy::math::{Vec2, Vec3};

/// What a mark hangs off: the foot of an agent's robot by the agent's
/// name, a hamlet's common by its id, or a point of the world.
#[derive(Clone, PartialEq, Debug)]
pub enum Anchor {
    Agent(String),
    Hamlet(String),
    Point(Vec3),
}

/// A colour of the panels' theme by name, or one of its own.
#[derive(Clone, PartialEq, Debug)]
pub enum Paint {
    Theme(String),
    Rgba([u8; 4]),
}

/// The shape of a mark, in pixels.
#[derive(Clone, PartialEq, Debug)]
pub enum Form {
    /// From its corner.
    Rect { w: f32, h: f32 },
    /// About its middle.
    Circle { r: f32 },
    /// From its start to `end` from where `to` is seen, or from where
    /// the mark's own anchor is where it has none.
    Line {
        to: Option<Anchor>,
        end: Vec2,
        width: f32,
    },
    /// From the top left of the words.
    Text { text: String, size: f32 },
}

/// A shape `offset` pixels from where its anchor is seen.
#[derive(Clone, PartialEq, Debug)]
pub struct Mark {
    pub anchor: Anchor,
    pub offset: Vec2,
    pub form: Form,
    pub paint: Paint,
}

/// The marks one plugin draws, with its id and the name the chief
/// knows it by.
#[derive(Clone, PartialEq, Debug)]
pub struct Overlay {
    pub plugin: String,
    pub by: String,
    pub marks: Vec<Mark>,
}

/// Every plugin's overlay, in the order they were first drawn. The
/// host of the plugins keeps it.
#[derive(Resource, Clone, PartialEq, Default, Debug)]
pub struct Overlays(pub Vec<Overlay>);
