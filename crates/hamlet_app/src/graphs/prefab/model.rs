//! A `.glb` file read as a tree of named objects, each with its own
//! meshes and transform, as Blender's outliner shows them: what a
//! prefab is made of. The catalogue reads the same files flattened
//! into one model (`assets.rs`).

use anyhow::Context;
use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, Image, ImageSampler, ImageType,
};
use bevy::math::{Quat, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::transform::components::Transform;

/// The objects of a `.glb` file's scene, each parent before its
/// children.
pub(super) struct Model {
    pub objects: Vec<Object>,
    /// The file's pictures, by their index in it; none for one that
    /// could not be read.
    pub images: Vec<Option<Image>>,
}

/// One node of the scene.
pub(super) struct Object {
    /// Its node names from the scene's root, joined by `/`.
    pub path: String,
    /// Its own name.
    pub name: String,
    /// Its parent's place in [`Model::objects`].
    pub parent: Option<usize>,
    /// How many objects are above it.
    pub depth: usize,
    /// Its place, turn and scale in its parent's space.
    pub local: Transform,
    pub pieces: Vec<Piece>,
}

/// One primitive of an object's mesh, and the material the file gives
/// it.
pub(super) struct Piece {
    pub mesh: Mesh,
    pub own: Own,
}

/// What the file's material sets of a piece.
#[derive(Clone, Copy, Debug)]
pub(super) struct Own {
    pub base: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    /// The picture its base colour reads, by its index in the file.
    pub texture: Option<usize>,
}

impl Model {
    /// The path of every object, in order.
    pub fn paths(&self) -> Vec<String> {
        self.objects
            .iter()
            .map(|object| object.path.clone())
            .collect()
    }

    /// The object at `path`'s place in [`Model::objects`].
    pub fn find(&self, path: &str) -> Option<usize> {
        self.objects.iter().position(|object| object.path == path)
    }
}

/// Reads the scene of the `.glb` file `bytes`.
pub(super) fn read(bytes: &[u8]) -> anyhow::Result<Model> {
    let gltf = gltf::Gltf::from_slice(bytes)
        .context("the file is not glTF")?;
    let blob = gltf.blob.as_deref();
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .context("the file has no scene")?;
    let mut objects = Vec::new();
    let roots: Vec<gltf::Node> = scene.nodes().collect();
    add(&roots, None, blob, &mut objects);
    anyhow::ensure!(!objects.is_empty(), "the file's scene is empty");
    let images =
        gltf.images().map(|image| picture(&image, blob)).collect();
    Ok(Model { objects, images })
}

/// Adds `nodes`, children of the object at `parent`, and those below
/// them to `objects`. A name used twice among them takes its count
/// after the first, `Window#2`.
fn add(
    nodes: &[gltf::Node],
    parent: Option<usize>,
    blob: Option<&[u8]>,
    objects: &mut Vec<Object>,
) {
    let mut seen: Vec<String> = Vec::new();
    for node in nodes {
        let name = node.name().map_or_else(
            || format!("node {}", node.index()),
            str::to_owned,
        );
        let count =
            seen.iter().filter(|other| **other == name).count();
        seen.push(name.clone());
        let own = if count == 0 {
            name.clone()
        } else {
            format!("{name}#{}", count + 1)
        };
        let (path, depth) = match parent {
            Some(at) => (
                format!("{}/{own}", objects[at].path),
                objects[at].depth + 1,
            ),
            None => (own, 0),
        };
        let (translation, rotation, scale) =
            node.transform().decomposed();
        let at = objects.len();
        objects.push(Object {
            path,
            name,
            parent,
            depth,
            local: Transform {
                translation: Vec3::from(translation),
                rotation: Quat::from_array(rotation),
                scale: Vec3::from(scale),
            },
            pieces: pieces(node, blob),
        });
        let children: Vec<gltf::Node> = node.children().collect();
        add(&children, Some(at), blob, objects);
    }
}

/// The triangle primitives of `node`'s mesh, in its own space.
fn pieces(node: &gltf::Node, blob: Option<&[u8]>) -> Vec<Piece> {
    let mut pieces = Vec::new();
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
        let positions: Vec<[f32; 3]> = positions.collect();
        let indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..positions.len() as u32).collect(),
        };
        // The reader checks neither indices nor lengths.
        if indices.iter().any(|&i| i as usize >= positions.len()) {
            continue;
        }
        let normals: Option<Vec<[f32; 3]>> = reader
            .read_normals()
            .map(Iterator::collect)
            .filter(|normals: &Vec<[f32; 3]>| {
                normals.len() == positions.len()
            });
        let uvs: Option<Vec<[f32; 2]>> = reader
            .read_tex_coords(0)
            .map(|uvs| uvs.into_f32().collect())
            .filter(|uvs: &Vec<[f32; 2]>| {
                uvs.len() == positions.len()
            });
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_indices(Indices::U32(indices));
        match normals {
            Some(normals) => {
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_NORMAL,
                    normals,
                );
            }
            None => mesh.compute_smooth_normals(),
        }
        if let Some(uvs) = uvs {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        }
        let material = primitive.material();
        let pbr = material.pbr_metallic_roughness();
        pieces.push(Piece {
            mesh,
            own: Own {
                base: pbr.base_color_factor(),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                emissive: material.emissive_factor(),
                texture: pbr
                    .base_color_texture()
                    .map(|info| info.texture().source().index()),
            },
        });
    }
    pieces
}

/// The picture `image` holds in the file's own buffer, read; nothing
/// for one kept in another file or of a kind that does not read.
fn picture(
    image: &gltf::Image,
    blob: Option<&[u8]>,
) -> Option<Image> {
    let gltf::image::Source::View { view, mime_type } =
        image.source()
    else {
        return None;
    };
    if !matches!(view.buffer().source(), gltf::buffer::Source::Bin) {
        return None;
    }
    let bytes =
        blob?.get(view.offset()..view.offset() + view.length())?;
    Image::from_buffer(
        bytes,
        ImageType::MimeType(mime_type),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.glb` of two empty nodes named `Window` under one named
    /// `Tower`.
    fn tower() -> Vec<u8> {
        let json = br#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"Tower","children":[1,2],"translation":[1,0,0]},{"name":"Window"},{"name":"Window","scale":[2,2,2]}]}"#;
        let mut json = json.to_vec();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let length = 12 + 8 + json.len() as u32;
        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&length.to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json);
        glb
    }

    #[test]
    fn objects_are_named_by_their_path_and_a_twin_by_its_count() {
        let model = read(&tower()).unwrap();
        assert_eq!(
            model.paths(),
            ["Tower", "Tower/Window", "Tower/Window#2"]
        );
        assert_eq!(model.objects[2].parent, Some(0));
        assert_eq!(model.objects[2].depth, 1);
        assert_eq!(model.objects[2].local.scale, Vec3::splat(2.0));
        assert_eq!(model.objects[0].local.translation, Vec3::X);
        assert_eq!(model.find("Tower/Window#2"), Some(2));
    }
}
