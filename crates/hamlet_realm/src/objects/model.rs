//! Meshes read from `.glb` files, to draw an asset whose shape is a
//! file.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Context;
use bevy::ecs::resource::Resource;
use bevy::math::{Mat4, Quat, Vec3};
use hamlet_graph::{Catalogue, Shape, Socket};

use crate::meshes::Meshing;

/// A mesh as read from its file: its parts, each in one material
/// slot, and the corners of the box around them.
pub struct Model {
    parts: Vec<Part>,
    low: Vec3,
    high: Vec3,
    pub sockets: BTreeMap<String, Socket>,
}

struct Part {
    slot: String,
    /// The colour the file gives the material.
    own: Vec3,
    /// The colour the file gives each corner, linear, which the
    /// material's own colour is multiplied by, as glTF has it.
    colours: Option<Vec<Vec3>>,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
}

impl Model {
    /// The sides of the box around it.
    pub fn extent(&self) -> Vec3 {
        self.high - self.low
    }

    /// The size of the box around it, as it was made, never flat.
    pub fn size(&self) -> Vec3 {
        self.extent().max(Vec3::splat(1e-4))
    }

    /// The names of its material slots, each once.
    pub fn slots(&self) -> Vec<String> {
        let mut slots: Vec<String> =
            self.parts.iter().map(|part| part.slot.clone()).collect();
        slots.sort();
        slots.dedup();
        slots
    }

    /// Adds the model to `meshing`, its box fitted to a box of `size`
    /// at `middle`, turned by `rotation`, each slot in the colour
    /// `paint` gives it from the file's own. The file's own is asked
    /// for at each corner, with the corner's colour in it, so a slot
    /// painted as the file has it keeps the colours baked into its
    /// corners. A slot `paint` gives none is drawn into `apart` in
    /// white, for one that lights on its own, as panes do.
    pub fn draw(
        &self,
        meshing: &mut Meshing,
        mut apart: Option<&mut Meshing>,
        middle: Vec3,
        size: Vec3,
        rotation: Quat,
        paint: impl Fn(&str, Vec3) -> Option<Vec3>,
    ) {
        let centre = (self.low + self.high) * 0.5;
        let scale =
            size / (self.high - self.low).max(Vec3::splat(1e-4));
        for part in &self.parts {
            let (into, colour) = match paint(&part.slot, part.own) {
                Some(colour) => (&mut *meshing, colour),
                None => match apart.as_deref_mut() {
                    Some(apart) => (apart, Vec3::ONE),
                    None => continue,
                },
            };
            let first = part
                .positions
                .iter()
                .zip(&part.normals)
                .enumerate()
                .fold(None, |first, (k, (p, n))| {
                    let at =
                        middle + rotation * ((*p - centre) * scale);
                    let normal =
                        (rotation * (*n / scale)).normalize_or_zero();
                    let colour = match &part.colours {
                        Some(colours) => {
                            paint(&part.slot, part.own * colours[k])
                                .unwrap_or(colour)
                        }
                        None => colour,
                    };
                    let index = into.v(at, normal, colour);
                    first.or(Some(index))
                });
            let Some(first) = first else {
                continue;
            };
            for [a, b, c] in part.indices.as_chunks::<3>().0 {
                into.tri(first + a, first + b, first + c);
            }
        }
    }
}

/// Reads the triangles of every mesh in a `.glb` file's scene, placed
/// as its nodes place them.
pub fn read(bytes: &[u8]) -> anyhow::Result<Model> {
    let gltf = gltf::Gltf::from_slice(bytes)
        .context("the file is not glTF")?;
    let blob = gltf.blob.as_deref();
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .context("the file has no scene")?;
    let (mut parts, mut marks) = (Vec::new(), Vec::new());
    for node in scene.nodes() {
        walk(&node, Mat4::IDENTITY, blob, &mut parts, &mut marks);
    }
    let (mut low, mut high) =
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in parts.iter().flat_map(|part| &part.positions) {
        low = low.min(*p);
        high = high.max(*p);
    }
    anyhow::ensure!(low.x <= high.x, "the file has no triangles");
    let (centre, extent) = ((low + high) * 0.5, high - low);
    let sockets = marks
        .into_iter()
        .map(|(name, transform)| {
            let (_, turn, at) =
                transform.to_scale_rotation_translation();
            // On an axis the asset is flat along, the socket is on
            // it.
            let at = Vec3::select(
                extent.cmpgt(Vec3::splat(1e-4)),
                (at - centre) / extent.max(Vec3::splat(1e-4)),
                Vec3::ZERO,
            );
            (name, Socket { at, turn })
        })
        .collect();
    Ok(Model {
        parts,
        low,
        high,
        sockets,
    })
}

/// Reads the triangles of `node` and those below it into `parts`. A
/// node named `socket_<name>` marks a socket, kept in `marks`, and
/// its own mesh is not drawn.
fn walk(
    node: &gltf::Node,
    above: Mat4,
    blob: Option<&[u8]>,
    parts: &mut Vec<Part>,
    marks: &mut Vec<(String, Mat4)>,
) {
    let transform =
        above * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(name) =
        node.name().and_then(|name| name.strip_prefix("socket_"))
    {
        marks.push((name.to_owned(), transform));
        for child in node.children() {
            walk(&child, transform, blob, parts, marks);
        }
        return;
    }
    let turn = transform.inverse().transpose();
    for primitive in
        node.mesh().iter().flat_map(gltf::Mesh::primitives)
    {
        if primitive.mode() != gltf::mesh::Mode::Triangles {
            continue;
        }
        let reader =
            primitive.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => blob,
                gltf::buffer::Source::Uri(_) => None,
            });
        let Some(positions) = reader.read_positions() else {
            continue;
        };
        let positions: Vec<Vec3> = positions
            .map(|p| transform.transform_point3(Vec3::from(p)))
            .collect();
        let indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..positions.len() as u32).collect(),
        };
        // The file's indices and normals are not checked by the glTF
        // reader, and `smooth` and `Model::draw` index with them.
        if indices.iter().any(|&i| i as usize >= positions.len()) {
            continue;
        }
        let normals = match reader.read_normals() {
            Some(normals) => normals
                .map(|n| {
                    turn.transform_vector3(Vec3::from(n))
                        .normalize_or_zero()
                })
                .collect::<Vec<_>>(),
            None => smooth(&positions, &indices),
        };
        if normals.len() != positions.len() {
            continue;
        }
        let colours = reader
            .read_colors(0)
            .map(|colours| {
                colours.into_rgb_f32().map(Vec3::from).collect::<Vec<_>>()
            })
            .filter(|colours| colours.len() == positions.len());
        let material = primitive.material();
        let slot = match (material.name(), material.index()) {
            (Some(name), _) => name.to_owned(),
            (None, Some(index)) => format!("material {index}"),
            (None, None) => hamlet_graph::BODY.to_owned(),
        };
        let [r, g, b, _] =
            material.pbr_metallic_roughness().base_color_factor();
        parts.push(Part {
            slot,
            own: Vec3::new(r, g, b),
            colours,
            positions,
            normals,
            indices,
        });
    }
    for child in node.children() {
        walk(&child, transform, blob, parts, marks);
    }
}

/// Normals for a mesh whose file gives none: each corner's, the sum
/// of its faces'.
fn smooth(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for corners in indices.as_chunks::<3>().0 {
        let [a, b, c] = corners.map(|i| i as usize);
        let face = (positions[b] - positions[a])
            .cross(positions[c] - positions[a]);
        for i in [a, b, c] {
            normals[i] += face;
        }
    }
    normals.into_iter().map(Vec3::normalize_or_zero).collect()
}

/// Each mesh file read, by its path in the folder it is read from,
/// or why it could not be.
#[derive(Resource, Default)]
pub struct Models(BTreeMap<String, anyhow::Result<Model>>);

impl Models {
    /// The model in the file at `path` in `folder`, read the first
    /// time it is asked for.
    pub fn get(
        &mut self,
        folder: &Path,
        path: &str,
    ) -> Result<&Model, &anyhow::Error> {
        self.0
            .entry(path.to_owned())
            .or_insert_with(|| {
                fs::read(folder.join(path))
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| read(&bytes))
                    .with_context(|| path.to_owned())
            })
            .as_ref()
    }

    /// Keeps `model` as the one in the file at `path`.
    pub fn insert(&mut self, path: String, model: Model) {
        self.0.insert(path, Ok(model));
    }

    /// Sets the size of each mesh in `catalogue` to its file's in
    /// `folder`, for one added before assets kept their size. One
    /// whose file cannot be read is left as it is.
    pub fn measure(
        &mut self,
        catalogue: &mut Catalogue,
        folder: &Path,
    ) {
        for asset in catalogue.0.values_mut() {
            if let Shape::Mesh(path) = &asset.shape
                && let Ok(model) = self.get(folder, path)
            {
                asset.size = model.size();
            }
        }
    }

    /// Forgets every file read, so each is read again.
    pub fn forget(&mut self) {
        self.0.clear();
    }

    /// Forgets the file at `path`, so it is read again.
    pub fn forget_file(&mut self, path: &str) {
        self.0.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::glb::{corners, glb, triangle};

    use super::*;

    #[test]
    fn a_glb_reads_as_its_triangles_and_sockets_where_its_nodes_put_them()
     {
        let model = read(&triangle()).expect("the file reads");
        assert_eq!(model.slots(), ["roof_tiles"]);
        assert_eq!(
            (model.low, model.high),
            (Vec3::new(0.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 0.0))
        );
        assert_eq!(model.parts[0].normals, [Vec3::Z; 3]);
        // Flat along z, the socket sits on the triangle.
        assert_eq!(
            model.sockets["lamp"].at,
            Vec3::new(0.5, 0.5, 0.0)
        );
    }

    #[test]
    fn a_slot_painted_as_the_file_has_it_keeps_its_corners_colours() {
        let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0,"COLOR_0":1},"material":0}]}],"materials":[{"name":"tiles","pbrMetallicRoughness":{"baseColorFactor":[0.5,1,1,1]}}],"buffers":[{"byteLength":72}],"bufferViews":[{"buffer":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[2,1,0]},{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3"}]}"#;
        let mut bin = corners();
        bin.extend(
            [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
                .iter()
                .flat_map(|f| f.to_le_bytes()),
        );
        let model = read(&glb(json, &bin)).expect("the file reads");
        let asked = std::cell::RefCell::new(Vec::new());
        model.draw(
            &mut Meshing::new(),
            None,
            Vec3::ZERO,
            model.size(),
            Quat::IDENTITY,
            |_, own| {
                asked.borrow_mut().push(own);
                Some(own)
            },
        );
        // The material's own colour times each corner's.
        assert_eq!(
            asked.borrow()[1..],
            [
                Vec3::new(0.5, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0)
            ]
        );
    }

    #[test]
    fn a_primitive_indexing_past_its_positions_is_left_out() {
        let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}],"buffers":[{"byteLength":44}],"bufferViews":[{"buffer":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[2,1,0]},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}]}"#;
        let mut bin = corners();
        bin.extend(
            [0u16, 1, 5, 0].iter().flat_map(|i| i.to_le_bytes()),
        );
        let error =
            read(&glb(json, &bin)).err().expect("it is refused");
        assert_eq!(error.to_string(), "the file has no triangles");
    }
}
