//! What a plugin builds in the world in 3D: a scene of nodes about a
//! site, each with one of Hamlet's shapes or a mesh the plugin sent,
//! a surface or a shader the plugin sent, and a light. The host
//! builds it; nothing here reaches the graphics card but as data and
//! WGSL the host checked. A scene and meshes need the capability
//! `render.scene`, and shaders `render.shader`, both allowed when the
//! chief lets the plugin in.

use crate::bindings::hamlet::plugin::render::{self as host, Scene};
pub use crate::bindings::hamlet::plugin::render::{
    Finish, Lamp, Material, Node, Shaded, Shape, Site, Surface, Vec3,
    Vec4,
};

/// The common of the hamlet of the id `id`.
pub fn hamlet(id: &str) -> Site {
    Site::Hamlet(id.into())
}

/// The robot of the agent `name`.
pub fn agent(name: &str) -> Site {
    Site::Agent(name.into())
}

/// A point of the world, in metres, `y` up.
pub fn point(x: f32, y: f32, z: f32) -> Site {
    Site::Point(Vec3 { x, y, z })
}

/// Replaces what the plugin builds in the world with `nodes` about
/// `site`, each after its parent; none takes it away.
pub fn build(site: Site, nodes: Vec<Node>) -> Result<(), String> {
    host::build(&Scene { site, nodes })
}

/// Sends a mesh, kept as `id`: points, a normal for each or none, and
/// three indices for each triangle.
pub fn mesh(
    id: &str,
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    indices: &[u32],
) -> Result<(), String> {
    let points = |list: &[[f32; 3]]| {
        list.iter()
            .map(|&[x, y, z]| Vec3 { x, y, z })
            .collect::<Vec<_>>()
    };
    host::mesh(id, &points(positions), &points(normals), indices)
}

/// Sends a shader, kept as `id`: WGSL that defines `fn shade(surface:
/// Surface) -> vec4<f32>`. The error says why the host refused it.
pub fn shader(id: &str, source: &str) -> Result<(), String> {
    host::shader(id, source)
}

/// A surface of `finish` in the colour 0xRRGGBBAA, half rough.
pub fn surface(finish: Finish, colour: u32) -> Material {
    Material::Surface(Surface {
        finish,
        colour,
        glow: 0.0,
        roughness: 0.5,
        metallic: 0.0,
    })
}

/// A surface that glows in the colour 0xRRGGBBAA, `glow` strong.
pub fn glowing(colour: u32, glow: f32) -> Material {
    Material::Surface(Surface {
        finish: Finish::Unlit,
        colour,
        glow,
        roughness: 1.0,
        metallic: 0.0,
    })
}

/// The shader the plugin sent as `id`, given `params` (at most four),
/// which it reads with `param(i)`.
pub fn shaded(id: &str, params: &[[f32; 4]]) -> Material {
    Material::Shaded(Shaded {
        shader: id.into(),
        params: params
            .iter()
            .map(|&[x, y, z, w]| Vec4 { x, y, z, w })
            .collect(),
    })
}

/// A point light of the colour 0xRRGGBB.
pub fn point_light(colour: u32, lumens: f32, range: f32) -> Lamp {
    Lamp {
        colour,
        lumens,
        range,
        cone: None,
    }
}

/// A spot light of the colour 0xRRGGBB, shining along the node's -z
/// in a cone of the half angle `cone`, in radians.
pub fn spot_light(
    colour: u32,
    lumens: f32,
    range: f32,
    cone: f32,
) -> Lamp {
    Lamp {
        colour,
        lumens,
        range,
        cone: Some(cone),
    }
}

/// A node of the id `id` at the site, unturned, one metre across,
/// with no shape and no light.
pub fn node(id: u32) -> Node {
    Node {
        id,
        parent: None,
        translation: Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        scale: Vec3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
        shape: None,
        material: surface(Finish::Solid, 0xffffffff),
        lamp: None,
    }
}

impl Node {
    /// This, placed from the node `parent`.
    pub fn under(mut self, parent: u32) -> Self {
        self.parent = Some(parent);
        self
    }

    /// This, `x`, `y`, `z` metres from where its parent is.
    pub fn at(mut self, x: f32, y: f32, z: f32) -> Self {
        self.translation = Vec3 { x, y, z };
        self
    }

    /// This, turned by `angle` radians about the upright.
    pub fn turned(mut self, angle: f32) -> Self {
        let (sin, cos) = (angle * 0.5).sin_cos();
        self.rotation = Vec4 {
            x: 0.0,
            y: sin,
            z: 0.0,
            w: cos,
        };
        self
    }

    /// This, `x`, `y`, `z` times its size.
    pub fn scaled(mut self, x: f32, y: f32, z: f32) -> Self {
        self.scale = Vec3 { x, y, z };
        self
    }

    /// This, of `shape` painted with `material`.
    pub fn shaped(
        mut self,
        shape: Shape,
        material: Material,
    ) -> Self {
        self.shape = Some(shape);
        self.material = material;
        self
    }

    /// This, with `lamp` at it.
    pub fn lit(mut self, lamp: Lamp) -> Self {
        self.lamp = Some(lamp);
        self
    }
}
