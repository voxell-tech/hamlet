//! Where the open asset is kept. The asset is a file of the library,
//! written by Save. The window's folder remembers which asset was
//! open and the changes not yet saved, at `<FOLDER>/open.ron`,
//! written a moment after each change and read again at launch.
//! Graph files elsewhere are reached by Import and Export, and the
//! files of the assets a graph adds travel beside them.

use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use bevy::app::{App, Update};
use bevy::ecs::change_detection::Mut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::text::{EditableText, TextEdit};
use bevy::time::Time;
use hamlet_client::ViewFolder;
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_graph::{Book, Shape, Sheet};
use serde::{Deserialize, Serialize};

use super::setup::{EditorKey, Setup};
use super::{Editor, canvas};
use crate::library::{
    Item, Kind, Library, Source, mesh_path, temp_of, write_atomic,
};

/// How long the open asset is left as it is before what is
/// remembered of it is written.
const SETTLE: Duration = Duration::from_secs(1);

pub(super) fn plugin<K: EditorKey>(app: &mut App) {
    app.init_resource::<Files<K>>()
        .add_observer(chosen::<K>)
        .add_systems(
            Update,
            (restore::<K>, keep::<K>, focus_naming::<K>),
        );
}

/// What a file chosen in a dialog is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialog {
    Import,
    Export,
}

/// The name being asked for, and a note on the last thing done with
/// a file.
#[derive(Resource)]
pub struct Files<K: EditorKey> {
    /// The last thing done with a file, or its error.
    pub note: String,
    /// The name Save as is asking for, while it asks.
    pub naming: Option<String>,
    /// The revision last written to the window's folder.
    kept: u64,
    /// A revision not yet written, and when it was first seen.
    seen: Option<(u64, Duration)>,
    /// Whether what was remembered has been read at launch.
    restored: bool,
    /// The entity the dialogs answer to.
    asker: Option<Entity>,
    /// What the dialog open now is for.
    asking: Dialog,
    key: PhantomData<K>,
}

impl<K: EditorKey> Default for Files<K> {
    fn default() -> Self {
        Self {
            note: String::new(),
            naming: None,
            kept: 0,
            seen: None,
            restored: false,
            asker: None,
            asking: Dialog::Import,
            key: PhantomData,
        }
    }
}

impl<K: EditorKey> Files<K> {
    /// Whether `asker` is the entity the import and export dialogs
    /// answer to.
    pub fn is_asker(&self, asker: Entity) -> bool {
        self.asker == Some(asker)
    }
}

/// On the field Save as asks for a name in.
#[derive(Component)]
pub(super) struct NamingField<K: EditorKey>(pub PhantomData<K>);

/// Where an open asset was found.
#[derive(Serialize, Deserialize)]
enum Origin {
    /// A file of the library.
    File,
    /// An example.
    Example,
}

/// What the window's folder remembers of the open asset.
#[derive(Serialize, Deserialize)]
struct Remembered {
    origin: Origin,
    name: String,
    /// The sheet as it was left, when it had unsaved changes.
    unsaved: Option<Sheet>,
}

impl Remembered {
    fn of<K: EditorKey>(editor: &Editor<K>) -> Self {
        Self {
            origin: if editor.is_own() {
                Origin::File
            } else {
                Origin::Example
            },
            name: editor.name.clone(),
            unsaved: editor.is_dirty().then(|| editor.sheet.clone()),
        }
    }
}

/// Where the window, keeping its files in `folder`, remembers the
/// open asset of the editor of `setup`.
fn remembered_at(folder: &Path, setup: &Setup) -> PathBuf {
    folder.join(setup.folder).join("open.ron")
}

fn write_remembered(
    path: &Path,
    remembered: &Remembered,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("making {}", parent.display())
        })?;
    }
    let text = hamlet_graph::to_ron(remembered)
        .context("writing what is open")?;
    write_atomic(path, &text)
}

/// Copies the file of each asset `book` adds from the folder `from`
/// to the folder `to`. Every file is copied beside its place first,
/// and put in place only when all are, so a failure leaves the files
/// in `to` as they were.
fn carry(book: &Book, from: &Path, to: &Path) -> anyhow::Result<()> {
    if from == to {
        return Ok(());
    }
    let mut staged = Vec::new();
    let copied = stage(book, from, to, &mut staged).and_then(|()| {
        staged.iter().try_for_each(|(temp, target)| {
            fs::rename(temp, target).with_context(|| {
                format!("replacing {}", target.display())
            })
        })
    });
    if copied.is_err() {
        for (temp, _) in &staged {
            let _ = fs::remove_file(temp);
        }
    }
    copied
}

/// Copies each asset file of `book` from `from` to a temporary file
/// beside its place in `to`, listing each as the temporary file and
/// its place.
fn stage(
    book: &Book,
    from: &Path,
    to: &Path,
    staged: &mut Vec<(PathBuf, PathBuf)>,
) -> anyhow::Result<()> {
    for asset in book.assets.0.values() {
        let Shape::Mesh(path) = &asset.shape else {
            continue;
        };
        let (source, target) =
            (mesh_path(from, path)?, mesh_path(to, path)?);
        if staged.iter().any(|(_, placed)| *placed == target) {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("making {}", parent.display())
            })?;
        }
        let temp = temp_of(&target);
        staged.push((temp.clone(), target));
        fs::copy(&source, &temp).with_context(|| {
            format!("copying {}", source.display())
        })?;
    }
    Ok(())
}

/// The folder a graph file is in.
fn beside(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new("."))
}

/// Opens `item` in the editor, and gives the names of the graphs
/// split off from its file when it held several.
fn load<K: EditorKey>(
    library: &mut Library,
    editor: &mut Editor<K>,
    item: &Item,
) -> anyhow::Result<Vec<String>> {
    let (sheet, split) = library.read(item)?;
    editor.open(item.name.clone(), item.source.clone(), sheet);
    Ok(split)
}

/// Opens the asset the window remembered, with the changes it had
/// not saved, once, as soon as the window's folder and the library
/// are known.
fn restore<K: EditorKey>(
    folder: Option<Res<ViewFolder>>,
    mut library: ResMut<Library>,
    mut files: ResMut<Files<K>>,
    mut editor: ResMut<Editor<K>>,
) {
    let Some(folder) = folder else {
        return;
    };
    if files.restored || library.root().is_none() {
        return;
    }
    files.restored = true;
    let remembered =
        fs::read_to_string(remembered_at(&folder.0, &editor.setup))
            .ok()
            .and_then(|text| ron::from_str::<Remembered>(&text).ok());
    if let Some(remembered) = remembered {
        let found = library
            .items
            .iter()
            .find(|item| {
                item.kind == editor.setup.kind
                    && item.name == remembered.name
                    && item.is_own()
                        == matches!(remembered.origin, Origin::File)
            })
            .cloned();
        files.note = match restored(
            &mut library,
            &mut editor,
            found,
            remembered,
        ) {
            Ok(note) => note,
            Err(error) => format!("{error:#}"),
        };
    }
    files.kept = editor.revision;
}

/// Opens `found`, else saves the unsaved sheet as a new asset, and
/// gives the note on what was recovered.
fn restored<K: EditorKey>(
    library: &mut Library,
    editor: &mut Editor<K>,
    found: Option<Item>,
    remembered: Remembered,
) -> anyhow::Result<String> {
    match (found, remembered.unsaved) {
        (Some(item), unsaved) => {
            load(library, editor, &item)?;
            Ok(match unsaved {
                Some(sheet) => {
                    editor.recover(sheet);
                    format!("Recovered the changes to {}", item.name)
                }
                None => String::new(),
            })
        }
        (None, Some(sheet)) => {
            let kind = editor.setup.kind;
            let name = library.free_name(kind, &remembered.name);
            let path = library.save_sheet(kind, &name, &sheet)?;
            editor.open(name.clone(), Source::File(path), sheet);
            Ok(format!(
                "{} was gone; kept its changes as {name}",
                remembered.name
            ))
        }
        (None, None) => Ok(String::new()),
    }
}

/// Writes what the window remembers of the open asset once it has
/// stayed the same for a moment.
fn keep<K: EditorKey>(
    time: Res<Time>,
    folder: Option<Res<ViewFolder>>,
    editor: Res<Editor<K>>,
    mut files: ResMut<Files<K>>,
) {
    let Some(folder) = folder else {
        return;
    };
    if !files.restored || editor.revision == files.kept {
        return;
    }
    let now = time.elapsed();
    match files.seen {
        Some((revision, since)) if revision == editor.revision => {
            if now - since < SETTLE {
                return;
            }
        }
        _ => {
            files.seen = Some((editor.revision, now));
            return;
        }
    }
    match write_remembered(
        &remembered_at(&folder.0, &editor.setup),
        &Remembered::of(&editor),
    ) {
        Ok(()) => files.kept = editor.revision,
        Err(error) => {
            // The revision stays unkept; the settle wait starts
            // again, so the write is tried again a moment later.
            files.note =
                format!("Could not keep what is open: {error:#}");
            files.seen = Some((editor.revision, now));
        }
    }
}

/// Opens `item`, which discards the changes the open asset has not
/// saved. The note says which, in [`Files::note`].
pub fn open_item<K: EditorKey>(
    world: &mut World,
    item: &Item,
) -> anyhow::Result<()> {
    let editor = world.resource::<Editor<K>>();
    let discarded = editor.is_dirty().then(|| editor.name.clone());
    let opened =
        world.resource_scope(|world, mut library: Mut<Library>| {
            let mut editor = world.resource_mut::<Editor<K>>();
            load::<K>(&mut library, &mut editor, item)
        });
    let note = match &opened {
        Ok(split) => {
            let mut note = format!("Opened {}", item.name);
            if !split.is_empty() {
                note.push_str(&format!(
                    "; split off {}",
                    split.join(", ")
                ));
            }
            if let Some(name) = discarded {
                note.push_str(&format!(
                    "; discarded the unsaved changes to {name}"
                ));
            }
            note
        }
        Err(error) => format!("{error:#}"),
    };
    let mut files = world.resource_mut::<Files<K>>();
    files.note = note;
    files.naming = None;
    if opened.is_ok() {
        canvas::fit::<K>(world);
    }
    opened.map(|_| ())
}

/// The kind of the graphs the editor `K` edits.
fn kind<K: EditorKey>(world: &World) -> Kind {
    world.resource::<Editor<K>>().setup.kind
}

/// Makes a new empty graph in the library and opens it.
pub fn create<K: EditorKey>(world: &mut World) {
    let kind = kind::<K>(world);
    let wanted = format!("new_{}", kind_word(kind));
    let created =
        world.resource_mut::<Library>().create(kind, &wanted);
    match created {
        // Opening notes its own outcome.
        Ok(item) => {
            let _ = open_item::<K>(world, &item);
        }
        Err(error) => {
            world.resource_mut::<Files<K>>().note =
                format!("{error:#}");
        }
    }
}

/// Writes the open graph to its file in the library. An example has
/// no file of its own to write, so it is saved under a name asked
/// for, as by [`save_as`].
pub fn save<K: EditorKey>(world: &mut World) {
    if world.resource::<Editor<K>>().is_own() {
        let name = world.resource::<Editor<K>>().name.clone();
        write_as::<K>(world, name, "Saved");
    } else {
        save_as::<K>(world);
    }
}

/// Starts asking for the name to save a copy of the open graph
/// under, ready with the nearest free name to the open one's.
pub fn save_as<K: EditorKey>(world: &mut World) {
    let name = world.resource::<Editor<K>>().name.clone();
    let free = world
        .resource::<Library>()
        .free_name(kind::<K>(world), &name);
    world.resource_mut::<Files<K>>().naming = Some(free);
}

/// Saves the open graph as `wanted`, or the nearest free name, and
/// goes on editing the copy.
pub fn save_named<K: EditorKey>(world: &mut World, wanted: &str) {
    world.resource_mut::<Files<K>>().naming = None;
    let name = world
        .resource::<Library>()
        .free_name(kind::<K>(world), wanted);
    write_as::<K>(world, name, "Saved as");
}

/// Writes the open graph to the library as `name`, and makes it the
/// open asset.
fn write_as<K: EditorKey>(
    world: &mut World,
    name: String,
    word: &str,
) {
    let saved = {
        let editor = world.resource::<Editor<K>>();
        let (kind, sheet) = (editor.setup.kind, editor.sheet.clone());
        world
            .resource_mut::<Library>()
            .save_sheet(kind, &name, &sheet)
    };
    let note = match saved {
        Ok(path) => {
            world
                .resource_mut::<Editor<K>>()
                .saved(name.clone(), Source::File(path));
            format!("{word} {name}")
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Files<K>>().note = note;
}

/// Gives the open graph's file the name `wanted`, or the nearest free
/// name, keeping the changes not yet saved.
pub fn rename<K: EditorKey>(world: &mut World, wanted: &str) {
    let item = world.resource::<Editor<K>>().item();
    if wanted.trim() == item.name {
        return;
    }
    let renamed = {
        let mut library = world.resource_mut::<Library>();
        library.rename(&item, wanted).and_then(|name| {
            let path = library
                .path_of(item.kind, &name)
                .context("no library yet")?;
            Ok((name, path))
        })
    };
    let note = match renamed {
        Ok((name, path)) => {
            world
                .resource_mut::<Editor<K>>()
                .renamed(name.clone(), Source::File(path));
            format!("Renamed to {name}")
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Files<K>>().note = note;
}

/// Asks for a graph file to add to the library and open.
pub fn import<K: EditorKey>(world: &mut World) {
    ask::<K>(world, Dialog::Import);
}

/// Asks for a file to write the open graph to, with the files of the
/// assets it adds beside it.
pub fn export<K: EditorKey>(world: &mut World) {
    ask::<K>(world, Dialog::Export);
}

fn ask<K: EditorKey>(world: &mut World, asking: Dialog) {
    let asker = match world.resource::<Files<K>>().asker {
        Some(asker) => asker,
        None => {
            let asker = world.spawn_empty().id();
            world.resource_mut::<Files<K>>().asker = Some(asker);
            asker
        }
    };
    world.resource_mut::<Files<K>>().asking = asking;
    let word = kind_word(kind::<K>(world));
    let (mode, title, name) = match asking {
        Dialog::Import => (
            FileMode::Open,
            format!("Add a {word} from a file"),
            String::new(),
        ),
        Dialog::Export => (
            FileMode::Save,
            format!("Export the {word}"),
            format!("{}.ron", world.resource::<Editor<K>>().name),
        ),
    };
    world.trigger(ChooseFile {
        asker,
        mode,
        title,
        extension: "ron".to_owned(),
        name,
    });
}

fn kind_word(kind: Kind) -> String {
    kind.word().to_lowercase()
}

/// Imports or exports with the file the chief chose.
fn chosen<K: EditorKey>(
    chosen: On<FileChosen>,
    files: Res<Files<K>>,
    mut commands: Commands,
) {
    if files.asker != Some(chosen.asker) {
        return;
    }
    let asking = files.asking;
    let path = match chosen.mode {
        FileMode::Save if chosen.path.extension().is_none() => {
            chosen.path.with_extension("ron")
        }
        _ => chosen.path.clone(),
    };
    commands.queue(move |world: &mut World| {
        let note = match asking {
            Dialog::Import => import_from::<K>(world, &path),
            Dialog::Export => export_to::<K>(world, &path),
        };
        match note {
            Ok(note) => {
                world.resource_mut::<Files<K>>().note = note;
            }
            Err(error) => {
                world.resource_mut::<Files<K>>().note =
                    format!("{error:#}");
            }
        }
    });
}

fn import_from<K: EditorKey>(
    world: &mut World,
    path: &Path,
) -> anyhow::Result<String> {
    let kind = kind::<K>(world);
    let mut library = world.resource_mut::<Library>();
    let name = library.import(kind, path)?;
    let item = library
        .find(kind, &name)
        .context("the added graph is not listed")?;
    open_item::<K>(world, &item)?;
    Ok(format!("Added {name}"))
}

fn export_to<K: EditorKey>(
    world: &mut World,
    path: &Path,
) -> anyhow::Result<String> {
    let editor = world.resource::<Editor<K>>();
    let book = editor
        .book(editor.rules.assets.named_in([&editor.sheet.graph]));
    let library = world
        .resource::<Library>()
        .root()
        .context("no library yet")?;
    carry(&book, library, beside(path))?;
    let text = book.to_ron().context("writing the graph")?;
    write_atomic(path, &text)?;
    Ok(format!(
        "Exported {}",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}

/// How far the field Save as asks in has come to being focused.
#[derive(Default)]
enum Focusing {
    #[default]
    Idle,
    /// Shown, and waiting a moment for the name it was given to be
    /// set in it.
    Waiting,
    Done,
}

/// Focuses the field Save as asks in once, a moment after it shows.
fn focus_naming<K: EditorKey>(
    files: Res<Files<K>>,
    fields: Query<&Children, With<NamingField<K>>>,
    mut texts: Query<&mut EditableText>,
    mut focus: ResMut<InputFocus>,
    mut stage: Local<Focusing>,
) {
    if files.naming.is_none() {
        *stage = Focusing::Idle;
        return;
    }
    match *stage {
        Focusing::Idle => *stage = Focusing::Waiting,
        Focusing::Waiting => {
            let Some(input) = fields
                .iter()
                .next()
                .and_then(|kids| kids.first().copied())
            else {
                return;
            };
            focus.set(input, FocusCause::Pressed);
            if let Ok(mut text) = texts.get_mut(input) {
                text.queue_edit(TextEdit::TextEnd(false));
            }
            *stage = Focusing::Done;
        }
        Focusing::Done => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bevy::math::Vec3;
    use hamlet_graph::Asset;
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_graph_whose_second_file_is_missing_leaves_the_first_kept_file_alone()
     {
        let folder = Scratch::new("graph-carry");
        let (from, to) = (folder.join("from"), folder.join("to"));
        fs::create_dir_all(&from).unwrap();
        fs::create_dir_all(&to).unwrap();
        fs::write(from.join("a.glb"), b"new").unwrap();
        fs::write(to.join("a.glb"), b"old").unwrap();
        let mesh = |file: &str| Asset {
            shape: Shape::Mesh(file.to_owned()),
            size: Vec3::ONE,
            slots: BTreeMap::new(),
            sockets: BTreeMap::new(),
        };
        let mut book = Book::of("main", Sheet::default());
        book.assets.0.insert("a".to_owned(), mesh("a.glb"));
        book.assets.0.insert("b".to_owned(), mesh("b.glb"));

        assert!(carry(&book, &from, &to).is_err());
        assert_eq!(fs::read(to.join("a.glb")).unwrap(), b"old");
        assert!(!to.join(".a.glb.tmp").exists());

        fs::write(from.join("b.glb"), b"b").unwrap();
        carry(&book, &from, &to).unwrap();
        assert_eq!(fs::read(to.join("a.glb")).unwrap(), b"new");
    }
}
