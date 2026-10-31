//! The meshes a book adds as assets. A chosen `.glb` is copied into
//! the `assets/` folder beside the book the window keeps, and added
//! to the book's catalogue with a palette colour for each of its
//! materials. The preview reads each file once and draws it fitted
//! to its placement, as a box fills one.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Mat4, Quat, Vec3};
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    icon_button, row,
};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_blueprint::{Asset, Paint, Shape, Socket};
use hamlet_client::ViewFolder;
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_realm::meshes::Meshing;
use hamlet_realm::palette::Palette as Colours;
use hamlet_ui::styles::{ICON, command, description, note, section};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, with_cx};

use super::files::{self, Files};
use super::{Editor, params};

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
        .add_observer(chosen);
}

/// The palette's colour named `name`, of [`COLOURS`].
pub(super) fn palette_colour(
    c: &Colours,
    name: &str,
) -> Option<Vec3> {
    Some(match name {
        "plaster" => c.plaster,
        "plaster_worn" => c.plaster_worn,
        "whitewash" => c.whitewash,
        "ochre" => c.ochre,
        "limestone" => c.limestone,
        "limestone_shade" => c.limestone_shade,
        "rock" => c.rock,
        "rock_dark" => c.rock_dark,
        "timber" => c.timber,
        "timber_dark" => c.timber_dark,
        "log" => c.log,
        "bark" => c.bark,
        "terracotta" => c.terracotta,
        "tile_drift" => c.tile_drift,
        "slate_roof" => c.slate_roof,
        "moss" => c.moss,
        // The first hamlet's roof and the first agent's livery, in
        // place of the ones the realm picks for each.
        "roof" => c.towns[0].roof,
        "livery" => c.livery[0],
        "pane" => {
            let lit = c.pane_lit.to_linear();
            Vec3::new(lit.red, lit.green, lit.blue)
        }
        "brass" => c.brass,
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
        let normals = match reader.read_normals() {
            Some(normals) => normals
                .map(|n| {
                    turn.transform_vector3(Vec3::from(n))
                        .normalize_or_zero()
                })
                .collect(),
            None => smooth(&positions, &indices),
        };
        let material = primitive.material();
        let slot = match (material.name(), material.index()) {
            (Some(name), _) => name.to_owned(),
            (None, Some(index)) => format!("material {index}"),
            (None, None) => hamlet_blueprint::BODY.to_owned(),
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
pub(super) struct Meshes(BTreeMap<String, Result<Model, String>>);

impl Meshes {
    /// The model in the file at `path` beside the book in `folder`,
    /// read the first time it is asked for.
    pub(super) fn get(
        &mut self,
        folder: &Path,
        path: &str,
    ) -> Result<&Model, &str> {
        self.0
            .entry(path.to_owned())
            .or_insert_with(|| {
                fs::read(folder.join(path))
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| read(&bytes))
                    .map_err(|error| format!("{path}: {error:#}"))
            })
            .as_ref()
            .map_err(String::as_str)
    }

    /// Forgets every file read, so each is read again.
    pub(super) fn forget(&mut self) {
        self.0.clear();
    }
}

/// The panel's part for the assets the book adds: each with a colour
/// for each of its material slots, and a way to take it out; and the
/// button that adds one.
pub(super) fn part() -> Vec<AnyView> {
    let mut parts = vec![
        section("Assets").boxed(),
        description(
            "Built-in boxes, and meshes added from .glb files. Each \
             material takes a palette colour, or keeps its own.",
        )
        .boxed(),
    ];
    parts.push(
        with_cx(|cx| {
            let editor = cx.world.resource::<Editor>();
            let gap = cx.theme().space.gap;
            let cards: Vec<AnyView> = editor
                .rules
                .assets
                .0
                .iter()
                .map(|(name, asset)| {
                    card(name.clone(), asset.clone())
                })
                .collect();
            column(cards).gap(gap)
        })
        .boxed(),
    );
    parts.push(command("Add asset", import).boxed());
    parts
}

/// One asset's name, a way to take it out, and its slots' colours.
fn card(name: String, asset: Asset) -> AnyView {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let remove = name.clone();
        let mut rows: Vec<AnyView> = vec![
            row((
                column((note(name.replace('_', " ")),))
                    .width(percent(100.0))
                    .min_width(px(0.0)),
                button(glyph(Glyph::Cross, ICON))
                    .rules(icon_button)
                    .tooltip(|| {
                        note("Take this asset out of the book")
                    })
                    .on_activate(move |world| {
                        world
                            .resource_mut::<Editor>()
                            .set_asset(remove.clone(), None);
                    }),
            ))
            .gap(gap)
            .align(AlignItems::Center)
            .boxed(),
        ];
        for slot in asset.slots.keys() {
            rows.push(params::labelled(
                &slot.replace('_', " "),
                colour(name.clone(), asset.clone(), slot.clone()),
            ));
        }
        if !asset.sockets.is_empty() {
            let names: Vec<&str> =
                asset.sockets.keys().map(String::as_str).collect();
            rows.push(params::labelled(
                "sockets",
                note(names.join(", ")).boxed(),
            ));
        }
        column(rows)
            .gap(gap / 2.0)
            .padding(UiRect::all(px(gap)))
            .border(1.0)
            .border_color(cx.theme().palette.border)
            .radius(cx.theme().space.round)
    })
    .boxed()
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
                .resource_mut::<Editor>()
                .set_asset(name.clone(), Some(asset));
        },
    )
    .width(percent(100.0))
    .boxed()
}

/// What the dialog that chooses a file to add answers to.
#[derive(Resource, Default)]
struct Importer(Option<Entity>);

/// Asks for a `.glb` file to add as an asset.
pub(super) fn import(world: &mut World) {
    let asker = match world.resource::<Importer>().0 {
        Some(asker) => asker,
        None => {
            let asker = world.spawn_empty().id();
            world.resource_mut::<Importer>().0 = Some(asker);
            asker
        }
    };
    world.trigger(ChooseFile {
        asker,
        mode: FileMode::Open,
        title: "Add an asset".to_owned(),
        extension: "glb".to_owned(),
        name: String::new(),
    });
}

/// Copies the chosen file beside the book and adds it to the
/// catalogue, named for the file.
fn chosen(
    chosen: On<FileChosen>,
    importer: Res<Importer>,
    folder: Option<Res<ViewFolder>>,
    mut meshes: ResMut<Meshes>,
    mut files: ResMut<Files>,
    mut editor: ResMut<Editor>,
) {
    if importer.0 != Some(chosen.asker) {
        return;
    }
    let Some(folder) = folder else {
        return;
    };
    let book = files::book_folder(&folder.0);
    match add(&book, &chosen.path, &editor, &mut meshes) {
        Ok((name, asset)) => {
            files.note = format!("Added {name}");
            editor.set_asset(name, Some(asset));
        }
        Err(error) => files.note = format!("{error:#}"),
    }
}

/// Copies the file at `from` into `book`'s `assets/` folder, and the
/// catalogue entry for it under a name no asset has.
fn add(
    book: &Path,
    from: &Path,
    editor: &Editor,
    meshes: &mut Meshes,
) -> anyhow::Result<(String, Asset)> {
    let stem = from
        .file_stem()
        .map(|stem| stem.to_string_lossy().replace(' ', "_"))
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "asset".to_owned());
    let taken = editor.rules.assets.names();
    let name = std::iter::once(stem.clone())
        .chain((2..).map(|n| format!("{stem}_{n}")))
        .find(|name| {
            !taken.contains(name)
                && !book.join(file_of(name)).exists()
        })
        .unwrap_or_default();
    let path = file_of(&name);
    let to: PathBuf = book.join(&path);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("making {}", parent.display())
        })?;
    }
    fs::copy(from, &to)
        .with_context(|| format!("copying {}", from.display()))?;
    let model = match meshes.get(book, &path) {
        Ok(model) => model,
        Err(problem) => {
            let problem = problem.to_owned();
            meshes.0.remove(&path);
            let _ = fs::remove_file(&to);
            anyhow::bail!(problem);
        }
    };
    let slots = model
        .slots()
        .into_iter()
        .map(|slot| {
            let paint = Paint::Palette(guess(&slot).to_owned());
            (slot, paint)
        })
        .collect();
    Ok((
        name,
        Asset {
            shape: Shape::Mesh(path),
            slots,
            sockets: model.sockets.clone(),
        },
    ))
}

/// Where the asset `name`'s file is kept, beside the book.
fn file_of(name: &str) -> String {
    format!("assets/{name}.glb")
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
    use super::*;

    /// A `.glb` of one triangle in the material `roof_tiles`, with no
    /// normals, lifted by its node, and a socket at its top corner.
    fn triangle() -> Vec<u8> {
        let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],"nodes":[{"mesh":0,"translation":[0,1,0]},{"name":"socket_lamp","translation":[2,2,0]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"name":"roof_tiles"}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[2,1,0]}]}"#;
        let mut json = json.as_bytes().to_vec();
        json.resize(json.len().next_multiple_of(4), b' ');
        let bin: Vec<u8> =
            [0.0f32, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0, 0.0]
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();
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
}
