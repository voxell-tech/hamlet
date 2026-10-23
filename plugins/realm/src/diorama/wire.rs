//! What the realm's `diorama` service is asked and answers, as both
//! sides read it: the bazaar includes this file as it is, so the two
//! never differ. It depends on serde alone.
//!
//! A town is answered in parts, each a call of its own, so that no
//! answer is near what one call may hand over. The first part also
//! holds the [`Stage`]: the looks the meshes are drawn in, the
//! plinth's size and the light of the realm's day.

use serde::{Deserialize, Serialize};

/// The argument of `diorama` (`realm.diorama_ask`): part `part` of
/// the town of the template `template`, with its mayor and `agents`
/// more.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Ask {
    pub template: String,
    pub agents: u32,
    pub part: u32,
}

/// The result of `diorama` (`realm.diorama_part`): part `part` of
/// `parts`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Part {
    pub part: u32,
    pub parts: u32,
    /// In the first part only.
    pub stage: Option<Stage>,
    pub pieces: Vec<Piece>,
}

/// What the whole town is drawn with.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Stage {
    /// The radius of the plinth's top, of its base and of the ground
    /// the town stands on, about the origin.
    pub radius: f32,
    pub outer: f32,
    pub reach: f32,
    /// The plinth's depth under the ground, and the height of the
    /// tallest thing over it.
    pub depth: f32,
    pub height: f32,
    /// The materials, by [`Piece::look`].
    pub looks: Vec<Look>,
    pub sun: Sun,
    pub fill: Sun,
    /// Light from everywhere: its linear colour and brightness.
    pub ambient: ([f32; 3], f32),
    /// The camera's exposure, in EV100.
    pub exposure: f32,
}

/// A material: linear colours, which a mesh's vertex colours tint.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Look {
    pub base: [f32; 3],
    pub emissive: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    /// Whether what it draws casts shadows.
    pub shadows: bool,
}

/// A directional light: its linear colour, lux and the way it
/// travels.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Sun {
    pub color: [f32; 3],
    pub lux: f32,
    pub toward: [f32; 3],
    pub shadows: bool,
}

/// One mesh in one look, in the town's frame: the middle of the
/// common at the origin and up along +Y.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub look: u8,
    /// x, y and z of each vertex.
    pub positions: Vec<f32>,
    /// x, y and z of each vertex's normal, times 127.
    pub normals: Vec<i8>,
    /// Red, green and blue of each vertex: the square root of the
    /// linear colour, times 255.
    pub colors: Vec<u8>,
    pub indices: Vec<u32>,
}

impl Piece {
    /// A piece of linear `colors`, four a vertex.
    pub fn pack(
        look: u8,
        positions: Vec<f32>,
        normals: &[f32],
        colors: &[f32],
        indices: Vec<u32>,
    ) -> Self {
        let normals = normals
            .iter()
            .map(|n| (n.clamp(-1.0, 1.0) * 127.0).round() as i8)
            .collect();
        let colors = colors
            .chunks_exact(4)
            .flat_map(|c| [c[0], c[1], c[2]])
            .map(|c| (c.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8)
            .collect();
        Self {
            look,
            positions,
            normals,
            colors,
            indices,
        }
    }

    /// Its normals, unit length.
    pub fn normals(&self) -> Vec<f32> {
        self.normals
            .chunks_exact(3)
            .flat_map(|n| {
                let [x, y, z] = [n[0], n[1], n[2]].map(f32::from);
                let length = (x * x + y * y + z * z).sqrt().max(1e-6);
                [x / length, y / length, z / length]
            })
            .collect()
    }

    /// Its linear colours, four a vertex.
    pub fn colors(&self) -> Vec<f32> {
        self.colors
            .chunks_exact(3)
            .flat_map(|c| {
                let [r, g, b] = [c[0], c[1], c[2]]
                    .map(|c| (f32::from(c) / 255.0).powi(2));
                [r, g, b, 1.0]
            })
            .collect()
    }

    /// The bytes the engine counts for it as a mesh: its positions,
    /// normals, colours and indices as floats and words.
    pub fn mesh_bytes(&self) -> usize {
        let vertices = self.positions.len() / 3;
        4 * (vertices * (3 + 3 + 4) + self.indices.len())
    }
}
