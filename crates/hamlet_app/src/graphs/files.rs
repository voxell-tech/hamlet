//! Where a book of graphs is kept: in the window's folder, at
//! `<FOLDER>/book.ron`, written a moment after each change and read
//! again at launch; and in any file it is opened from or saved to.
//! The files of the assets the book adds sit beside it, and go with
//! it to a file it is saved to and from one it is opened from.

use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
use hamlet_blueprint::{Book, Shape};
use hamlet_client::ViewFolder;
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};

use super::flavor::Flavor;
use super::{Editor, canvas, example};
use crate::library::Library;

/// How long the book is left as it is before it is written.
const SETTLE: Duration = Duration::from_secs(1);

pub(super) fn plugin<F: Flavor>(app: &mut App) {
    app.init_resource::<Files<F>>()
        .add_observer(chosen::<F>)
        .add_systems(Update, (restore::<F>, keep::<F>));
}

/// What a book is kept in, and the last word on it.
#[derive(Resource)]
pub(super) struct Files<F: Flavor> {
    /// The file the book was last opened from or saved to.
    pub path: Option<PathBuf>,
    /// The last thing done with a file, or what went wrong.
    pub note: String,
    /// The book's revision last written to the window's folder.
    kept: u64,
    /// A revision not yet written, and when it was first seen.
    seen: Option<(u64, Duration)>,
    /// Whether the kept book has been read at launch.
    restored: bool,
    /// What the dialogs answer to.
    asker: Option<Entity>,
    flavor: PhantomData<F>,
}

impl<F: Flavor> Default for Files<F> {
    fn default() -> Self {
        Self {
            path: None,
            note: String::new(),
            kept: 0,
            seen: None,
            restored: false,
            asker: None,
            flavor: PhantomData,
        }
    }
}

impl<F: Flavor> Files<F> {
    /// Whether `asker` is the entity the open and save dialogs answer
    /// to.
    pub fn asks(&self, asker: Entity) -> bool {
        self.asker == Some(asker)
    }

    /// The file's name, empty when there is none.
    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map_or(String::new(), |name| {
                name.to_string_lossy().into_owned()
            })
    }
}

/// The folder the window keeps the book in, with its assets' files.
pub(super) fn book_folder<F: Flavor>(folder: &Path) -> PathBuf {
    folder.join(F::FOLDER)
}

/// Where the window keeps the book.
fn kept_at<F: Flavor>(folder: &Path) -> PathBuf {
    book_folder::<F>(folder).join("book.ron")
}

/// Copies the file of each asset `book` adds from the folder `from`
/// to the folder `to`.
fn carry(book: &Book, from: &Path, to: &Path) -> anyhow::Result<()> {
    if from == to {
        return Ok(());
    }
    for asset in book.assets.0.values() {
        let Shape::Mesh(path) = &asset.shape else {
            continue;
        };
        let (source, target) = (from.join(path), to.join(path));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("making {}", parent.display())
            })?;
        }
        fs::copy(&source, &target).with_context(|| {
            format!("copying {}", source.display())
        })?;
    }
    Ok(())
}

/// The folder a book's file is in.
fn beside(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new("."))
}

fn write(path: &Path, book: &Book) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("making {}", parent.display())
        })?;
    }
    let text = book.to_ron().context("writing the book")?;
    fs::write(path, text)
        .with_context(|| format!("writing {}", path.display()))
}

fn read(path: &Path) -> anyhow::Result<Book> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    example(&text).with_context(|| {
        format!("{} is not a graph book", path.display())
    })
}

/// Opens the book the window kept, once, as soon as its folder is
/// known.
fn restore<F: Flavor>(
    folder: Option<Res<ViewFolder>>,
    mut files: ResMut<Files<F>>,
    mut editor: ResMut<Editor<F>>,
) {
    let Some(folder) = folder else {
        return;
    };
    if files.restored {
        return;
    }
    files.restored = true;
    let path = kept_at::<F>(&folder.0);
    if path.exists() {
        match read(&path) {
            Ok(book) => editor.load(book),
            Err(error) => files.note = format!("{error:#}"),
        }
    }
    files.kept = editor.revision;
}

/// Writes the book to the window's folder once it has stayed the same
/// for a moment.
fn keep<F: Flavor>(
    time: Res<Time>,
    folder: Option<Res<ViewFolder>>,
    editor: Res<Editor<F>>,
    mut files: ResMut<Files<F>>,
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
    if let Err(error) =
        write(&kept_at::<F>(&folder.0), &editor.book())
    {
        files.note = format!("Could not keep the graphs: {error:#}");
    }
    files.kept = editor.revision;
}

/// Asks for a file to open the book from.
pub(super) fn open<F: Flavor>(world: &mut World) {
    ask::<F>(world, FileMode::Open, "Open a graph book");
}

/// Saves the book to the file it came from, or into the library under
/// its entry's name.
pub(super) fn save<F: Flavor>(world: &mut World) {
    let Some(path) = world.resource::<Files<F>>().path.clone() else {
        return keep_in_library::<F>(world);
    };
    let book = world.resource::<Editor<F>>().book();
    let kept = world
        .get_resource::<ViewFolder>()
        .map(|folder| book_folder::<F>(&folder.0));
    let note = match kept
        .map_or(Ok(()), |kept| carry(&book, &kept, beside(&path)))
        .and_then(|()| write(&path, &book))
    {
        Ok(()) => {
            format!("Saved {}", world.resource::<Files<F>>().name())
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Files<F>>().note = note;
}

/// Writes the book into the library, as a book of its kind named
/// after its entry, and saves it there from then on.
fn keep_in_library<F: Flavor>(world: &mut World) {
    let book = world.resource::<Editor<F>>().book();
    let mut library = world.resource_mut::<Library>();
    let name = library.free_name(F::KIND, &book.entry);
    let saved = library.save_book(F::KIND, &name, &book);
    let mut files = world.resource_mut::<Files<F>>();
    match saved {
        Ok(path) => {
            files.path = Some(path);
            files.note = format!("Saved {name} in the library");
        }
        Err(error) => files.note = format!("{error:#}"),
    }
}

/// Asks for a file to save the book to.
pub(super) fn save_as<F: Flavor>(world: &mut World) {
    ask::<F>(world, FileMode::Save, "Save the graph book");
}

fn ask<F: Flavor>(world: &mut World, mode: FileMode, title: &str) {
    let asker = match world.resource::<Files<F>>().asker {
        Some(asker) => asker,
        None => {
            let asker = world.spawn_empty().id();
            world.resource_mut::<Files<F>>().asker = Some(asker);
            asker
        }
    };
    let name = match &world.resource::<Files<F>>().path {
        Some(path) => {
            path.file_name().map_or_else(String::new, |name| {
                name.to_string_lossy().into_owned()
            })
        }
        None => {
            format!("{}.ron", world.resource::<Editor<F>>().name)
        }
    };
    world.trigger(ChooseFile {
        asker,
        mode,
        title: title.to_owned(),
        extension: "ron".to_owned(),
        name,
    });
}

/// Opens or saves the book with the file the chief chose.
fn chosen<F: Flavor>(
    chosen: On<FileChosen>,
    folder: Option<Res<ViewFolder>>,
    mut files: ResMut<Files<F>>,
    mut editor: ResMut<Editor<F>>,
    mut commands: Commands,
) {
    let kept = folder.map(|folder| book_folder::<F>(&folder.0));
    if files.asker != Some(chosen.asker) {
        return;
    }
    let path = match chosen.mode {
        FileMode::Save if chosen.path.extension().is_none() => {
            chosen.path.with_extension("ron")
        }
        _ => chosen.path.clone(),
    };
    let done = match chosen.mode {
        FileMode::Open => read(&path).and_then(|book| {
            if let Some(kept) = &kept {
                carry(&book, beside(&path), kept)?;
            }
            editor.load(book);
            commands.queue(canvas::fit::<F>);
            Ok("Opened")
        }),
        FileMode::Save => {
            let book = editor.book();
            kept.as_ref()
                .map_or(Ok(()), |kept| {
                    carry(&book, kept, beside(&path))
                })
                .and_then(|()| write(&path, &book))
                .map(|()| "Saved")
        }
    };
    match done {
        Ok(word) => {
            files.path = Some(path);
            files.note = format!("{word} {}", files.name());
        }
        Err(error) => files.note = format!("{error:#}"),
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::graphs::Blueprint;

    #[test]
    fn a_book_written_where_the_window_keeps_it_reads_back_the_same()
    {
        let folder = Scratch::new("graph-book");
        let book = example(Blueprint::EXAMPLES[0].1)
            .expect("the example parses");
        let path = kept_at::<Blueprint>(&folder);
        write(&path, &book).expect("the book writes");
        assert_eq!(read(&path).expect("the book reads"), book);
    }
}
