//! A worn mesh, a glTF binary (`.glb`), read inside the plugin: its
//! meshes by the names of their glTF meshes, and its spots by the
//! names of its empty nodes (docs/looks.md, "Meshes").

use glam::{Affine3A, Vec3};
use gltf::buffer::Source;
use gltf::mesh::Mode;

use crate::build::mesh::Mb;

/// The most vertices a worn file's meshes hold together.
const MOST_VERTICES: usize = 200_000;

/// The meshes and spots of a `.glb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glb {
    pub meshes: Vec<(String, Model)>,
    pub spots: Vec<(String, Vec3)>,
}

/// Triangles with a normal and a colour at each vertex, in linear
/// RGB as the world's meshes are coloured.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub pos: Vec<Vec3>,
    pub nor: Vec<Vec3>,
    pub col: Vec<Vec3>,
    pub idx: Vec<u32>,
}

impl Glb {
    /// Reads `bytes`. Every primitive of triangles of a mesh is part
    /// of it, in the mesh's own coordinates: no node's transform is
    /// applied.
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        let gltf = gltf::Gltf::from_slice(bytes)
            .map_err(|why| format!("not a glTF binary: {why}"))?;
        let blob = gltf.blob.as_deref();
        let mut glb = Glb::default();
        let mut vertices = 0;
        for mesh in gltf.document.meshes() {
            let Some(name) = mesh.name() else {
                continue;
            };
            let mut model = Model::default();
            for primitive in mesh.primitives() {
                if primitive.mode() != Mode::Triangles {
                    continue;
                }
                let reader = primitive.reader(|buffer| match buffer
                    .source()
                {
                    Source::Bin => blob,
                    Source::Uri(_) => None,
                });
                let Some(pos) = reader.read_positions() else {
                    continue;
                };
                let pos = pos.map(Vec3::from).collect::<Vec<_>>();
                vertices += pos.len();
                if vertices > MOST_VERTICES {
                    return Err(format!(
                        "more than {MOST_VERTICES} vertices"
                    ));
                }
                let n = pos.len();
                let nor = reader
                    .read_normals()
                    .map(|nor| {
                        nor.map(Vec3::from).collect::<Vec<_>>()
                    })
                    .filter(|nor| nor.len() == n);
                let col = reader
                    .read_colors(0)
                    .map(|col| {
                        col.into_rgb_f32()
                            .map(Vec3::from)
                            .collect::<Vec<_>>()
                    })
                    .filter(|col| col.len() == n)
                    .unwrap_or_else(|| vec![Vec3::ONE; n]);
                let idx = match reader.read_indices() {
                    Some(idx) => idx.into_u32().collect::<Vec<_>>(),
                    None => (0..n as u32).collect(),
                };
                if idx.iter().any(|&i| i as usize >= n) {
                    return Err(format!("`{name}` has a bad index"));
                }
                let nor = nor.unwrap_or_else(|| smooth(&pos, &idx));
                model.add(pos, nor, col, &idx);
            }
            if !model.idx.is_empty() {
                glb.meshes.push((name.to_owned(), model));
            }
        }
        for node in gltf.document.nodes() {
            if node.mesh().is_some() {
                continue;
            }
            if let Some(name) = node.name() {
                let (at, _, _) = node.transform().decomposed();
                glb.spots.push((name.to_owned(), Vec3::from(at)));
            }
        }
        Ok(glb)
    }

    /// The mesh named `name`.
    pub fn mesh(&self, name: &str) -> Option<&Model> {
        self.meshes
            .iter()
            .find(|(named, _)| named == name)
            .map(|(_, model)| model)
    }

    /// The spot named `name`.
    pub fn spot(&self, name: &str) -> Option<Vec3> {
        self.spots
            .iter()
            .find(|(named, _)| named == name)
            .map(|(_, at)| *at)
    }
}

impl Model {
    fn add(
        &mut self,
        pos: Vec<Vec3>,
        nor: Vec<Vec3>,
        col: Vec<Vec3>,
        idx: &[u32],
    ) {
        let base = self.pos.len() as u32;
        self.pos.extend(pos);
        self.nor.extend(nor);
        self.col.extend(col);
        self.idx.extend(idx.iter().map(|i| i + base));
    }

    /// Adds its triangles to `mb`, placed by `xf`.
    pub fn lay(&self, mb: &mut Mb, xf: Affine3A) {
        let outer = mb.xf;
        mb.at(outer * xf);
        let ids = (0..self.pos.len())
            .map(|i| mb.v(self.pos[i], self.nor[i], self.col[i]))
            .collect::<Vec<_>>();
        for tri in self.idx.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| ids[tri[k] as usize]);
            mb.tri(a, b, c);
        }
        mb.at(outer);
    }

    /// The least and most corners of its bounds.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.pos.iter().fold(
            (Vec3::INFINITY, Vec3::NEG_INFINITY),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }

    /// The mean colour of its vertices.
    pub fn tint(&self) -> Vec3 {
        if self.col.is_empty() {
            return Vec3::ONE;
        }
        self.col.iter().sum::<Vec3>() / self.col.len() as f32
    }
}

/// A normal at each vertex: the sum of those of its triangles.
fn smooth(pos: &[Vec3], idx: &[u32]) -> Vec<Vec3> {
    let mut nor = vec![Vec3::ZERO; pos.len()];
    for tri in idx.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| tri[k] as usize);
        let n = (pos[b] - pos[a]).cross(pos[c] - pos[a]);
        for i in [a, b, c] {
            nor[i] += n;
        }
    }
    nor.into_iter().map(|n| n.normalize_or(Vec3::Y)).collect()
}

#[cfg(test)]
pub mod tests;
