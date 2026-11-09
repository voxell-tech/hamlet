//! The library's meshes: added from their files and changed in the
//! ASSETS panel's details. A chosen `.glb` is copied into the library
//! and added to its catalogue with a palette colour for each of its
//! materials, and the library keeps the file it came from: when that
//! file is saved again, the copy is replaced and the mesh takes its
//! new size, slots and sockets, each slot it had keeping its colour.

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
use bevy::time::Time;
use bevy::ui::percent;
use bevy_fynix::views::{FrameProps, column, dropdown};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_desktop::file::{ChooseFiles, FilesChosen};
use hamlet_graph::{Asset, Paint, Shape};
use hamlet_graph_editor::library::picker::picture;
use hamlet_graph_editor::library::{
    Kind, Library, mesh_file, mesh_path, write_atomic,
};
use hamlet_realm::objects::{COLOURS, Models, read};
use hamlet_ui::styles::{ICON, labelled, note};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, with_cx};

use super::thumbnails;

/// The side of a mesh's picture in the ASSETS panel's details.
const PICTURE: f32 = 128.0;

/// How often the files the meshes were added from are looked at, for
/// one saved again.
const FOLLOW: Duration = Duration::from_secs(2);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Models>()
        .init_resource::<Importer>()
        .init_resource::<Followed>()
        .add_plugins(thumbnails::plugin)
        .add_observer(chosen)
        .add_systems(Update, follow);
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
        Paint::Shade(colour, shade) => {
            format!("{} at {shade}", colour.replace('_', " "))
        }
        Paint::Own => "its own".to_owned(),
    }
}

/// The colour of the slot `slot` of the asset `name`: one of the
/// palette's, or its own.
fn colour(name: String, asset: Asset, slot: String) -> AnyView {
    let at = match asset.slots.get(&slot) {
        Some(Paint::Palette(colour) | Paint::Shade(colour, _)) => {
            COLOURS
                .iter()
                .position(|known| known == colour)
                .unwrap_or(0)
        }
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
    world.resource_mut::<Models>().insert(path, model);
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
    use hamlet_testing::glb::{TRIANGLE, corners, glb, triangle};

    use super::*;

    #[test]
    fn a_mesh_follows_its_file_and_keeps_the_colours_chosen_for_it() {
        let folder = Scratch::new("assets-follow");
        let file = folder.join("barrel.glb");
        fs::write(&file, triangle()).unwrap();
        let mut world = World::new();
        world.insert_resource(Library::in_folder(
            folder.join("library"),
        ));
        world.init_resource::<Models>();
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
