//! Where the book of graphs is kept: in the window's folder, at
//! `graphs/book.ron`, written a moment after each change and read
//! again at launch; and in any file it is opened from or saved to.

use std::fs;
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
use hamlet_client::ViewFolder;
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_proc::Book;

use super::{Editor, canvas, example};

/// How long the book is left as it is before it is written.
const SETTLE: Duration = Duration::from_secs(1);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Files>()
        .add_observer(chosen)
        .add_systems(Update, (restore, keep));
}

/// What the book is kept in, and the last word on it.
#[derive(Resource, Default)]
pub(super) struct Files {
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
}

impl Files {
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

/// Where the window keeps the book.
fn kept_at(folder: &Path) -> PathBuf {
    folder.join("graphs").join("book.ron")
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
fn restore(
    folder: Option<Res<ViewFolder>>,
    mut files: ResMut<Files>,
    mut editor: ResMut<Editor>,
) {
    let Some(folder) = folder else {
        return;
    };
    if files.restored {
        return;
    }
    files.restored = true;
    let path = kept_at(&folder.0);
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
fn keep(
    time: Res<Time>,
    folder: Option<Res<ViewFolder>>,
    editor: Res<Editor>,
    mut files: ResMut<Files>,
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
    if let Err(error) = write(&kept_at(&folder.0), &editor.book()) {
        files.note = format!("Could not keep the graphs: {error:#}");
    }
    files.kept = editor.revision;
}

/// Asks for a file to open the book from.
pub(super) fn open(world: &mut World) {
    ask(world, FileMode::Open, "Open a graph book");
}

/// Saves the book to the file it came from, or asks for one.
pub(super) fn save(world: &mut World) {
    let Some(path) = world.resource::<Files>().path.clone() else {
        return save_as(world);
    };
    let book = world.resource::<Editor>().book();
    let note = match write(&path, &book) {
        Ok(()) => {
            format!("Saved {}", world.resource::<Files>().name())
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Files>().note = note;
}

/// Asks for a file to save the book to.
pub(super) fn save_as(world: &mut World) {
    ask(world, FileMode::Save, "Save the graph book");
}

fn ask(world: &mut World, mode: FileMode, title: &str) {
    let asker = match world.resource::<Files>().asker {
        Some(asker) => asker,
        None => {
            let asker = world.spawn_empty().id();
            world.resource_mut::<Files>().asker = Some(asker);
            asker
        }
    };
    let name = match &world.resource::<Files>().path {
        Some(path) => {
            path.file_name().map_or_else(String::new, |name| {
                name.to_string_lossy().into_owned()
            })
        }
        None => format!("{}.ron", world.resource::<Editor>().name),
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
fn chosen(
    chosen: On<FileChosen>,
    mut files: ResMut<Files>,
    mut editor: ResMut<Editor>,
    mut commands: Commands,
) {
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
        FileMode::Open => read(&path).map(|book| {
            editor.load(book);
            commands.queue(canvas::fit);
            "Opened"
        }),
        FileMode::Save => {
            write(&path, &editor.book()).map(|()| "Saved")
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
    use crate::graphs::{EXAMPLES, example};

    #[test]
    fn a_book_written_where_the_window_keeps_it_reads_back_the_same()
    {
        let folder = Scratch::new("graph-book");
        let book =
            example(EXAMPLES[0].1).expect("the example parses");
        let path = kept_at(&folder);
        write(&path, &book).expect("the book writes");
        assert_eq!(read(&path).expect("the book reads"), book);
    }
}
