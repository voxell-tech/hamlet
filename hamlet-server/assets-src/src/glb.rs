//! Meshes written as glTF binary, `.glb`: each a named mesh on a node
//! of its own, and named empty nodes that mark a spot. Vertex colours
//! are kept, so a mesh built in code looks the same read back.

use bevy::math::Vec3;
use hamlet_engine_sdk::scene::MeshData;
use serde_json::{Value, json};

/// A glTF component type: 32-bit float, unsigned int.
const FLOAT: u32 = 5126;
const INT: u32 = 5125;

/// A buffer view's target: vertices, or indices.
const VERTICES: u32 = 34962;
const INDICES: u32 = 34963;

/// What a `.glb` holds: meshes by name and spots by name.
#[derive(Default)]
pub struct Glb<'a> {
    meshes: Vec<(&'a str, &'a MeshData)>,
    spots: Vec<(&'a str, Vec3)>,
}

impl<'a> Glb<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `mesh`, named `name`.
    pub fn mesh(mut self, name: &'a str, mesh: &'a MeshData) -> Self {
        self.meshes.push((name, mesh));
        self
    }

    /// Adds an empty node named `name` at `at`.
    pub fn spot(mut self, name: &'a str, at: Vec3) -> Self {
        self.spots.push((name, at));
        self
    }

    /// The `.glb`'s bytes.
    pub fn write(&self) -> Vec<u8> {
        let mut bin = Vec::<u8>::new();
        let mut views = Vec::<Value>::new();
        let mut accessors = Vec::<Value>::new();
        let mut meshes = Vec::<Value>::new();
        let mut nodes = Vec::<Value>::new();
        let mut view =
            |bin: &mut Vec<u8>, bytes: &[u8], target: u32| {
                pad(bin, 0);
                views.push(json!({
                    "buffer": 0,
                    "byteOffset": bin.len(),
                    "byteLength": bytes.len(),
                    "target": target,
                }));
                bin.extend_from_slice(bytes);
                views.len() - 1
            };
        for (name, mesh) in &self.meshes {
            let mut attributes = serde_json::Map::new();
            for (key, floats, width, kind) in [
                ("POSITION", &mesh.positions, 3, "VEC3"),
                ("NORMAL", &mesh.normals, 3, "VEC3"),
                ("COLOR_0", &mesh.colors, 4, "VEC4"),
                ("TEXCOORD_0", &mesh.uvs, 2, "VEC2"),
            ] {
                if floats.is_empty() {
                    continue;
                }
                let at = view(&mut bin, &le(floats), VERTICES);
                let mut accessor = json!({
                    "bufferView": at,
                    "componentType": FLOAT,
                    "count": floats.len() / width,
                    "type": kind,
                });
                if key == "POSITION" {
                    let (min, max) = bounds(floats);
                    accessor["min"] = json!(min);
                    accessor["max"] = json!(max);
                }
                accessors.push(accessor);
                attributes
                    .insert(key.into(), json!(accessors.len() - 1));
            }
            let indices = mesh
                .indices
                .iter()
                .flat_map(|i| i.to_le_bytes())
                .collect::<Vec<_>>();
            let at = view(&mut bin, &indices, INDICES);
            accessors.push(json!({
                "bufferView": at,
                "componentType": INT,
                "count": mesh.indices.len(),
                "type": "SCALAR",
            }));
            meshes.push(json!({
                "name": name,
                "primitives": [{
                    "attributes": attributes,
                    "indices": accessors.len() - 1,
                    "material": 0,
                }],
            }));
            nodes.push(json!({
                "name": name,
                "mesh": meshes.len() - 1,
            }));
        }
        for (name, at) in &self.spots {
            nodes.push(json!({
                "name": name,
                "translation": at.to_array(),
            }));
        }
        pad(&mut bin, 0);
        let document = json!({
            "asset": { "version": "2.0", "generator": "hamlet" },
            "scene": 0,
            "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
            "nodes": nodes,
            "meshes": meshes,
            // White and matte: the vertex colours are the colour.
            "materials": [{
                "name": "matte",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.9,
                },
            }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": views,
            "accessors": accessors,
        });
        let mut text =
            serde_json::to_vec(&document).unwrap_or_default();
        pad(&mut text, b' ');
        let total = 12 + 8 + text.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&text);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }
}

/// Fills `bytes` with `with` to a multiple of four, as every chunk
/// and view of a `.glb` is aligned.
fn pad(bytes: &mut Vec<u8>, with: u8) {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(with);
    }
}

/// The floats' bytes, little-endian as glTF has them.
fn le(floats: &[f32]) -> Vec<u8> {
    floats.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// The least and the most of each axis of positions.
fn bounds(floats: &[f32]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for point in floats.as_chunks::<3>().0 {
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    (min, max)
}
