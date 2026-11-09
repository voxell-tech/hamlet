//! The library's meshes: read from their files, drawn in the
//! blueprint editor's preview, and changed in the ASSETS panel's
//! details. A chosen `.glb` is copied into the library and added to
//! its catalogue with a palette colour for each of its materials, and
//! the library keeps the file it came from: when that file is saved
//! again, the copy is replaced and the mesh takes its new size, slots
//! and sockets, each slot it had keeping its colour. The preview
//! reads each file once and draws it at its own size times its
//! object's scale.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Context;
use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res};
use bevy::ecs::world::World;
use bevy::math::{Mat4, Quat, Vec3};
use bevy::time::Time;
use bevy::ui::percent;
use bevy_fynix::views::{FrameProps, column, dropdown};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_desktop::file::{ChooseFiles, FilesChosen};
use hamlet_graph::{Asset, Catalogue, Paint, Shape, Socket};
use hamlet_graph_editor::library::picker::picture;
use hamlet_graph_editor::library::{
    Kind, Library, mesh_file, mesh_path, write_atomic,
};
use hamlet_realm::meshes::Meshing;
use hamlet_realm::palette::Palette as Colours;
use hamlet_ui::styles::{ICON, labelled, note};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, with_cx};

use super::thumbnails;

/// The side of a mesh's picture in the ASSETS panel's details.
const PICTURE: f32 = 128.0;

/// How often the files the meshes were added from are looked at, for
/// one saved again.
const FOLLOW: Duration = Duration::from_secs(2);

/// The palette's colours a material slot can take, of those a
/// building is made of.
pub(super) const COLOURS: [&str; 20] = [
    "plaster",
    "plaster_worn",
    "whitewash",
    "ochre",
    "limestone",
    "limestone_shade",
    "rock",
    "rock_dark",
    "timber",
    "timber_dark",
    "log",
    "bark",
    "terracotta",
    "tile_drift",
    "slate_roof",
    "moss",
    "roof",
    "livery",
    "pane",
    "brass",
];

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Meshes>()
        .init_resource::<Importer>()
        .init_resource::<Followed>()
        .add_plugins(thumbnails::plugin)
        .add_observer(chosen)
        .add_systems(Update, follow);
}

/// The palette's colour named `name`, of [`COLOURS`].
pub(super) fn palette_colour(
    colours: &Colours,
    name: &str,
) -> Option<Vec3> {
    Some(match name {
        "plaster" => colours.plaster,
        "plaster_worn" => colours.plaster_worn,
        "whitewash" => colours.whitewash,
        "ochre" => colours.ochre,
        "limestone" => colours.limestone,
        "limestone_shade" => colours.limestone_shade,
        "rock" => colours.rock,
        "rock_dark" => colours.rock_dark,
        "timber" => colours.timber,
        "timber_dark" => colours.timber_dark,
        "log" => colours.log,
        "bark" => colours.bark,
        "terracotta" => colours.terracotta,
        "tile_drift" => colours.tile_drift,
        "slate_roof" => colours.slate_roof,
        "moss" => colours.moss,
        // The first hamlet's roof and the first agent's livery, in
        // place of the ones the realm picks for each.
        "roof" => colours.towns[0].roof,
        "livery" => colours.livery[0],
        "pane" => {
            let pane = colours.pane_lit.to_linear();
            Vec3::new(pane.red, pane.green, pane.blue)
        }
        "brass" => colours.brass,
        _ => return None,
    })
}

/// A mesh as read from its file: its parts, each in one material
/// slot, and the corners of the box around them.
pub(super) struct Model {
    parts: Vec<Part>,
    low: Vec3,
    high: Vec3,
    sockets: BTreeMap<String, Socket>,
}

struct Part {
    slot: String,
    /// The colour the file gives the material.
    own: Vec3,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
}

impl Model {
    /// The sides of the box around it.
    pub(super) fn extent(&self) -> Vec3 {
        self.high - self.low
    }

    /// The size of the box around it, as it was made, never flat.
    fn size(&self) -> Vec3 {
        self.extent().max(Vec3::splat(1e-4))
    }

    /// The names of its material slots, each once.
    fn slots(&self) -> Vec<String> {
        let mut slots: Vec<String> =
            self.parts.iter().map(|part| part.slot.clone()).collect();
        slots.sort();
        slots.dedup();
        slots
    }

    /// Adds the model to `meshing`, its box fitted to a box of `size`
    /// at `middle`, turned by `rotation`, each slot in the colour
    /// `paint` gives it.
    pub(super) fn draw(
        &self,
        meshing: &mut Meshing,
        middle: Vec3,
        size: Vec3,
        rotation: Quat,
        paint: impl Fn(&str, Vec3) -> Vec3,
    ) {
        let centre = (self.low + self.high) * 0.5;
        let scale =
            size / (self.high - self.low).max(Vec3::splat(1e-4));
        for part in &self.parts {
            let colour = paint(&part.slot, part.own);
            let first = part
                .positions
                .iter()
                .zip(&part.normals)
                .fold(None, |first, (p, n)| {
                    let at =
                        middle + rotation * ((*p - centre) * scale);
                    let normal =
                        (rotation * (*n / scale)).normalize_or_zero();
                    let index = meshing.v(at, normal, colour);
                    first.or(Some(index))
                });
            let Some(first) = first else {
                continue;
            };
            for [a, b, c] in part.indices.as_chunks::<3>().0 {
                meshing.tri(first + a, first + b, first + c);
            }
        }
    }
}

/// Reads the triangles of every mesh in a `.glb` file's scene, placed
/// as its nodes place them.
fn read(bytes: &[u8]) -> anyhow::Result<Model> {
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

/// Each mesh file read, by its path beside the book, or why it could
/// not be.
#[derive(Resource, Default)]
pub(super) struct Meshes(BTreeMap<String, anyhow::Result<Model>>);

impl Meshes {
    /// The model in the file at `path` beside the book in `folder`,
    /// read the first time it is asked for.
    pub(super) fn get(
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

    /// Sets the size of each mesh in `catalogue` to its file's in
    /// `folder`, for one added before assets kept their size. One
    /// whose file cannot be read is left as it is.
    pub(super) fn measure(
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
    pub(super) fn forget(&mut self) {
        self.0.clear();
    }

    /// Forgets the file at `path`, so it is read again.
    pub(super) fn forget_file(&mut self, path: &str) {
        self.0.remove(path);
    }
}

/// The details of the mesh `name` in the ASSETS panel, built again
/// whenever the library changes, as when its file is saved again.
pub(crate) fn details(name: String) -> AnyView {
    keyed(
        resource::<Library, _>(|library| library.revision),
        move |_: &u64| mesh(name.clone()),
    )
    .within(column(()).gap(0.0))
    .boxed()
}

/// The mesh `name`'s picture, the file it follows, the colour of each
/// of its material slots and its sockets. A mesh the library added
/// takes a colour for each slot; a built-in one is shown as it is.
fn mesh(name: String) -> AnyView {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let library = cx.world.resource::<Library>();
        let added = library.catalogue.0.get(&name).cloned();
        let asset =
            added.clone().or_else(|| library.catalogue.get(&name));
        let mut rows: Vec<AnyView> = vec![picture(&name, PICTURE)];
        if let Some(from) = library.sources.get(&name) {
            rows.push(labelled(
                "file",
                note(from.display().to_string()).boxed(),
            ));
        }
        if let Some(asset) = asset {
            for (slot, paint) in &asset.slots {
                let shown = match &added {
                    Some(added) => colour(
                        name.clone(),
                        added.clone(),
                        slot.clone(),
                    ),
                    None => note(paint_word(paint)).boxed(),
                };
                rows.push(labelled(&slot.replace('_', " "), shown));
            }
            if !asset.sockets.is_empty() {
                let names: Vec<&str> = asset
                    .sockets
                    .keys()
                    .map(String::as_str)
                    .collect();
                rows.push(labelled(
                    "sockets",
                    note(names.join(", ")).boxed(),
                ));
            }
        }
        column(rows).gap(gap / 2.0)
    })
    .boxed()
}

/// How a slot's colour is named: the palette colour's name, or "its
/// own".
fn paint_word(paint: &Paint) -> String {
    match paint {
        Paint::Palette(colour) => colour.replace('_', " "),
        Paint::Own => "its own".to_owned(),
    }
}

/// The colour of the slot `slot` of the asset `name`: one of the
/// palette's, or its own.
fn colour(name: String, asset: Asset, slot: String) -> AnyView {
    let at = match asset.slots.get(&slot) {
        Some(Paint::Palette(colour)) => COLOURS
            .iter()
            .position(|known| known == colour)
            .unwrap_or(0),
        Some(Paint::Own) | None => COLOURS.len(),
    };
    dropdown(
        COLOURS
            .map(|colour| colour.replace('_', " "))
            .into_iter()
            .chain(["its own".to_owned()]),
        at,
        Glyph::Chevron.image(ICON),
        move |world, chosen| {
            let mut asset = asset.clone();
            let paint =
                COLOURS.get(chosen).map_or(Paint::Own, |colour| {
                    Paint::Palette((*colour).to_owned())
                });
            asset.slots.insert(slot.clone(), paint);
            world
                .resource_mut::<Library>()
                .set_asset(name.clone(), Some(asset));
        },
    )
    .width(percent(100.0))
    .boxed()
}

/// The entity the file dialog for adding an asset answers to.
#[derive(Resource, Default)]
struct Importer(Option<Entity>);

/// Asks for the `.glb` files to add as assets.
pub(crate) fn import(world: &mut World) {
    let asker = match world.resource::<Importer>().0 {
        Some(asker) => asker,
        None => {
            let asker = world.spawn_empty().id();
            world.resource_mut::<Importer>().0 = Some(asker);
            asker
        }
    };
    world.trigger(ChooseFiles {
        asker,
        title: "Add assets".to_owned(),
        extension: "glb".to_owned(),
    });
}

fn chosen(
    chosen: On<FilesChosen>,
    importer: Res<Importer>,
    mut commands: Commands,
) {
    if importer.0 != Some(chosen.asker) {
        return;
    }
    let paths = chosen.paths.clone();
    commands.queue(move |world: &mut World| {
        let (mut added, mut errors) = (Vec::new(), Vec::new());
        for path in &paths {
            match add_mesh(world, path) {
                Ok(name) => added.push(name),
                Err(error) => errors.push(format!("{error:#}")),
            }
        }
        let mut notes = Vec::new();
        if !added.is_empty() {
            notes.push(format!("Added {}", added.join(", ")));
        }
        notes.extend(errors);
        world.resource_mut::<Library>().note = notes.join("; ");
    });
}

/// Copies the `.glb` file at `from` into the library's meshes, with a
/// palette colour guessed for each of its materials, follows the file
/// from then on, and gives the mesh's name. A file added again, or
/// one named as a mesh added before meshes followed their files,
/// brings that mesh up to date; any other takes a name no mesh has.
fn add_mesh(
    world: &mut World,
    from: &Path,
) -> anyhow::Result<String> {
    let from =
        std::path::absolute(from).unwrap_or_else(|_| from.to_owned());
    let library = world.resource::<Library>();
    let stem = from
        .file_stem()
        .map(|stem| stem.to_string_lossy().replace(' ', "_"))
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "mesh".to_owned());
    let again = library
        .sources
        .iter()
        .find(|(_, source)| **source == from)
        .map(|(name, _)| name.clone())
        .or_else(|| {
            let earlier = matches!(
                library.catalogue.0.get(&stem),
                Some(Asset {
                    shape: Shape::Mesh(_),
                    ..
                })
            );
            (earlier && !library.sources.contains_key(&stem))
                .then(|| stem.clone())
        });
    let name =
        again.unwrap_or_else(|| library.free_name(Kind::Mesh, &stem));
    let bytes = fs::read(&from)
        .with_context(|| format!("reading {}", from.display()))?;
    refresh(world, &name, &bytes)?;
    world
        .resource_mut::<Followed>()
        .stamps
        .insert(name.clone(), stamp(&from));
    world
        .resource_mut::<Library>()
        .set_source(&name, Some(from));
    Ok(name)
}

/// Makes the mesh `name` the model in the `.glb` file `bytes`: its
/// file in the library replaced, and its size, slots and sockets the
/// model's. A slot it had keeps its colour, and a new one takes the
/// colour its name suggests. Bytes that do not read change nothing.
fn refresh(
    world: &mut World,
    name: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let model = read(bytes)?;
    let library = world.resource::<Library>();
    let root = library.root().context("no library yet")?.to_owned();
    let old = library.catalogue.0.get(name).cloned();
    let path = match old.as_ref().map(|asset| &asset.shape) {
        Some(Shape::Mesh(path)) => path.clone(),
        _ => mesh_file(name),
    };
    let to = mesh_path(&root, &path)?;
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("making {}", parent.display())
        })?;
    }
    write_atomic(&to, bytes)?;
    let had = old.map(|asset| asset.slots).unwrap_or_default();
    let slots = model
        .slots()
        .into_iter()
        .map(|slot| {
            let paint =
                had.get(&slot).cloned().unwrap_or_else(|| {
                    Paint::Palette(guess(&slot).to_owned())
                });
            (slot, paint)
        })
        .collect();
    let asset = Asset {
        shape: Shape::Mesh(path.clone()),
        size: model.size(),
        slots,
        sockets: model.sockets.clone(),
    };
    world.resource_mut::<Meshes>().0.insert(path, Ok(model));
    world
        .resource_mut::<Library>()
        .set_asset(name.to_owned(), Some(asset));
    Ok(())
}

/// A file's length and the time it was last written, or nothing for
/// one that cannot be looked at.
type Stamp = Option<(u64, SystemTime)>;

fn stamp(path: &Path) -> Stamp {
    let data = fs::metadata(path).ok()?;
    Some((data.len(), data.modified().ok()?))
}

/// The files the meshes follow, as they were when last looked at.
#[derive(Resource, Default)]
struct Followed {
    /// By the mesh's name.
    stamps: BTreeMap<String, Stamp>,
    /// When they were last looked at.
    checked: Duration,
}

/// Brings the meshes whose files have been saved again up to date,
/// every few seconds.
fn follow(world: &mut World) {
    let now = world.resource::<Time>().elapsed();
    let mut followed = world.resource_mut::<Followed>();
    if now.saturating_sub(followed.checked) < FOLLOW {
        return;
    }
    followed.checked = now;
    catch_up(world);
}

/// Brings each mesh whose file has changed since it was last looked
/// at up to date with it, and notes what was done. The first look at
/// a file, as when the window opens, leaves a mesh that already
/// matches it as it is; a file that is gone leaves its mesh as it
/// was.
fn catch_up(world: &mut World) {
    let library = world.resource::<Library>();
    let Some(root) = library.root().map(Path::to_owned) else {
        return;
    };
    let sources: Vec<(String, PathBuf)> = library
        .sources
        .iter()
        .map(|(name, from)| (name.clone(), from.clone()))
        .collect();
    let mut notes = Vec::new();
    for (name, from) in sources {
        let now = stamp(&from);
        let last = world
            .resource_mut::<Followed>()
            .stamps
            .insert(name.clone(), now);
        if now.is_none() || last == Some(now) {
            continue;
        }
        // A file still being written fails to read, and is read again
        // once it is whole, as its stamp changes then.
        let Ok(bytes) = fs::read(&from) else {
            continue;
        };
        let copy = match world
            .resource::<Library>()
            .catalogue
            .0
            .get(&name)
        {
            Some(Asset {
                shape: Shape::Mesh(path),
                ..
            }) => mesh_path(&root, path)
                .ok()
                .and_then(|path| fs::read(path).ok()),
            _ => None,
        };
        if last.is_none() && copy.is_some_and(|copy| copy == bytes) {
            continue;
        }
        notes.push(match refresh(world, &name, &bytes) {
            Ok(()) => format!("Updated {name}"),
            Err(error) => format!("{name}: {error:#}"),
        });
    }
    if !notes.is_empty() {
        world.resource_mut::<Library>().note = notes.join("; ");
    }
}

/// The palette colour a material's name suggests.
fn guess(slot: &str) -> &'static str {
    let slot = slot.to_lowercase();
    let words = [
        ("stone", "limestone"),
        ("rock", "rock"),
        ("wood", "timber"),
        ("timber", "timber"),
        ("plank", "log"),
        ("bark", "bark"),
        ("roof", "terracotta"),
        ("tile", "terracotta"),
        ("slate", "slate_roof"),
        ("leaf", "moss"),
        ("moss", "moss"),
    ];
    words
        .iter()
        .find(|(word, _)| slot.contains(word))
        .map_or("plaster", |(_, colour)| colour)
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    /// The glTF of one triangle in the material `roof_tiles`, with no
    /// normals, lifted by its node, and a socket at its top corner.
    const TRIANGLE: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],"nodes":[{"mesh":0,"translation":[0,1,0]},{"name":"socket_lamp","translation":[2,2,0]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"name":"roof_tiles"}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[2,1,0]}]}"#;

    fn triangle() -> Vec<u8> {
        glb(TRIANGLE, &corners())
    }

    fn corners() -> Vec<u8> {
        [0.0f32, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0, 0.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect()
    }

    fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
        let mut json = json.as_bytes().to_vec();
        json.resize(json.len().next_multiple_of(4), b' ');
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        let length = 12 + 8 + json.len() + 8 + bin.len();
        glb.extend((length as u32).to_le_bytes());
        glb.extend((json.len() as u32).to_le_bytes());
        glb.extend(b"JSON");
        glb.extend(json);
        glb.extend((bin.len() as u32).to_le_bytes());
        glb.extend(b"BIN\0");
        glb.extend(bin);
        glb
    }

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

    #[test]
    fn a_mesh_follows_its_file_and_keeps_the_colours_chosen_for_it() {
        let folder = Scratch::new("assets-follow");
        let file = folder.join("barrel.glb");
        fs::write(&file, triangle()).unwrap();
        let mut world = World::new();
        world.insert_resource(Library::in_folder(
            folder.join("library"),
        ));
        world.init_resource::<Meshes>();
        world.init_resource::<Followed>();
        let name = add_mesh(&mut world, &file).unwrap();
        let mut asset =
            world.resource::<Library>().catalogue.0[&name].clone();
        asset.slots.insert("roof_tiles".to_owned(), Paint::Own);
        world
            .resource_mut::<Library>()
            .set_asset(name.clone(), Some(asset));

        // Saved again twice as wide, the file is longer than it was,
        // so its stamp changes however coarse the clock.
        let wider = TRIANGLE.replace(
            r#""translation":[0,1,0]"#,
            r#""translation":[0,1,0],"scale":[2,1,1]"#,
        );
        fs::write(&file, glb(&wider, &corners())).unwrap();
        catch_up(&mut world);

        let library = world.resource::<Library>();
        let asset = &library.catalogue.0[&name];
        assert_eq!(asset.size.x, 4.0);
        assert_eq!(asset.slots["roof_tiles"], Paint::Own);
        assert_eq!(
            fs::read(folder.join("library").join(mesh_file(&name)))
                .unwrap(),
            fs::read(&file).unwrap()
        );
        // Added again, it is the same mesh, not a second one.
        assert_eq!(add_mesh(&mut world, &file).unwrap(), name);
        assert_eq!(world.resource::<Library>().catalogue.0.len(), 1);
    }
}
