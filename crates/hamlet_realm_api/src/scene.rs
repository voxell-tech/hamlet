//! What plugins build in the world in 3D: a tree of nodes made of
//! Hamlet's own shapes, materials and lights, or of meshes and
//! shaders the plugin sent, about a place it names. The host of the
//! plugins checks it and keeps it on the plugin; the realm's view
//! builds it.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::math::{Quat, Vec3, Vec4};

pub use crate::overlay::Anchor;

/// The values a plugin's shader is given, at most.
pub const PARAMS: usize = 4;

// `INTERFACE` writes the count out.
const _: () = assert!(PARAMS == 4);

/// What a plugin's shader is written against, shared by the check and
/// the material it becomes: the surface a point is coloured for, and
/// the values the node gives the shader, read with `param`. Each side
/// declares `hamlet_params` itself.
pub const INTERFACE: &str = "
struct Surface {
    uv: vec2<f32>,
    position: vec3<f32>,
    normal: vec3<f32>,
    time: f32,
}

struct HamletParams {
    values: array<vec4<f32>, 4>,
}

fn param(index: u32) -> vec4<f32> {
    return hamlet_params.values[min(index, 3u)];
}
";

/// The function a plugin's shader defines: `fn shade(surface:
/// Surface) -> vec4<f32>`.
pub const SHADE: &str = "shade";

/// A shape of Hamlet's own, one metre across about its middle, or a
/// mesh the plugin sent, by its id.
#[derive(Clone, PartialEq, Debug)]
pub enum Shape {
    Cube,
    Sphere,
    Cylinder,
    Cone,
    Capsule,
    Plane,
    Torus,
    Custom(String),
}

impl Shape {
    /// The radius about its middle that holds one of Hamlet's own
    /// shapes.
    pub fn reach(&self) -> Option<f32> {
        match self {
            Shape::Cube => Some(0.87),
            Shape::Sphere | Shape::Capsule | Shape::Torus => {
                Some(0.5)
            }
            Shape::Cylinder | Shape::Cone | Shape::Plane => {
                Some(0.71)
            }
            Shape::Custom(_) => None,
        }
    }
}

/// How a surface takes the light.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finish {
    /// Lit, and opaque.
    Solid,
    /// Lit, and as see-through as its colour's alpha.
    Clear,
    /// Its own colour whatever the light.
    Unlit,
}

/// What a node's shape is painted with.
#[derive(Clone, PartialEq, Debug)]
pub enum Material {
    Surface {
        finish: Finish,
        colour: [u8; 4],
        /// How strongly it glows in its colour, from 0.
        glow: f32,
        roughness: f32,
        metallic: f32,
    },
    /// A shader the plugin sent, by its id, and the values it is
    /// given.
    Shader { id: String, params: [Vec4; PARAMS] },
}

/// A light of a node.
#[derive(Clone, PartialEq, Debug)]
pub struct Lamp {
    pub colour: [u8; 3],
    pub lumens: f32,
    /// How far it reaches, in metres.
    pub range: f32,
    /// The half angle of a spot light's cone, in radians, which
    /// shines along the node's -Z. None for a point light.
    pub cone: Option<f32>,
}

/// One node of a scene, placed from its parent, or from the scene's
/// place where it has none.
#[derive(Clone, PartialEq, Debug)]
pub struct SceneNode {
    pub id: u32,
    pub parent: Option<u32>,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub shape: Option<Shape>,
    pub material: Material,
    pub lamp: Option<Lamp>,
}

/// A mesh a plugin sent: triangles of its points, with a normal for
/// each point or none.
#[derive(Clone, PartialEq, Debug)]
pub struct SceneMesh {
    pub id: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl SceneMesh {
    /// The radius about its middle that holds it.
    pub fn reach(&self) -> f32 {
        self.positions
            .iter()
            .map(|at| Vec3::from_array(*at).length())
            .fold(0.0, f32::max)
    }
}

/// A shader a plugin sent, checked: the WGSL that defines
/// [`SHADE`].
#[derive(Clone, PartialEq, Debug)]
pub struct SceneShader {
    pub id: String,
    pub source: String,
}

/// On the entity of a plugin: what it builds in the world, as the
/// host of the plugins checked it, with the name the chief knows the
/// plugin by.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct PluginScene {
    pub by: String,
    pub site: Anchor,
    /// Each node after its parent.
    pub nodes: Vec<SceneNode>,
    pub meshes: Vec<SceneMesh>,
    pub shaders: Vec<SceneShader>,
}

/// On the root of a plugin's scene in the world: the plugin's entity.
#[derive(Component, Clone, Copy, Debug)]
#[relationship(relationship_target = Staged)]
pub struct StagedBy(pub Entity);

/// On the entity of a plugin: the roots of its scene in the world,
/// which go with it.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = StagedBy, linked_spawn)]
pub struct Staged(Vec<Entity>);
