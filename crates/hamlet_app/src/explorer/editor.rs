//! The file open beside a hamlet's tree: read off the main thread,
//! and read again when it changes on the disk. What it shows is a
//! [`Page`], one per kind of file, where other viewers will join.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::tasks::IoTaskPool;
use hamlet_desktop::files::Watch;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

/// The largest file shown.
const LARGEST: u64 = 500 * 1024;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, keep);
}

/// What the editor shows of its file.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Page {
    Reading,
    Text(String),
    Binary,
    /// Larger than the most shown, by its size in bytes.
    TooBig(u64),
    /// Not there to read, and why.
    Gone(String),
}

/// The file open in a hamlet's editor, and what it shows of it.
#[derive(Component)]
pub(crate) struct Opened {
    pub file: PathBuf,
    /// Its path inside the hamlet's folder.
    pub name: String,
    pub page: Page,
}

/// A file, and its page as read.
type Read = (PathBuf, Page);

/// The reader of a hamlet's [`Opened`] file, which holds it to the
/// disk.
#[derive(Component)]
struct Reader {
    /// None where the system would not watch.
    watch: Option<Watch>,
    reading: bool,
    /// The file changed while it was read, so it is read again.
    due: bool,
    tx: UnboundedSender<Read>,
    rx: UnboundedReceiver<Read>,
}

/// Opens `file` in `hamlet`'s editor, in place of what it showed,
/// under its path inside `root`.
pub(crate) fn open(
    world: &mut World,
    hamlet: Entity,
    file: &Path,
    root: &Path,
) {
    let mut watch = Watch::new()
        .inspect_err(|e| {
            bevy::log::warn!("could not watch the file: {e}");
        })
        .ok();
    if let (Some(watch), Some(folder)) =
        (watch.as_mut(), file.parent())
    {
        watch.follow(&HashSet::from([folder.to_path_buf()]));
    }
    let (tx, rx) = unbounded_channel();
    if let Ok(mut hamlet) = world.get_entity_mut(hamlet) {
        hamlet.insert((
            Opened {
                file: file.to_path_buf(),
                name: file
                    .strip_prefix(root)
                    .unwrap_or(file)
                    .display()
                    .to_string(),
                page: Page::Reading,
            },
            Reader {
                watch,
                reading: false,
                due: true,
                tx,
                rx,
            },
        ));
    }
}

/// Empties `hamlet`'s editor.
pub(crate) fn close(world: &mut World, hamlet: Entity) {
    if let Ok(mut hamlet) = world.get_entity_mut(hamlet) {
        hamlet.remove::<(Opened, Reader)>();
    }
}

/// Takes in what was read, and reads the file again when it changed.
fn keep(mut editors: Query<(&mut Opened, &mut Reader)>) {
    for (mut opened, mut reader) in &mut editors {
        let reader = &mut *reader;
        while let Ok((file, page)) = reader.rx.try_recv() {
            reader.reading = false;
            if file == opened.file && opened.page != page {
                opened.page = page;
            }
        }
        if let Some(watch) = reader.watch.as_mut()
            && watch.changed().contains(&opened.file)
        {
            reader.due = true;
        }
        if reader.due && !reader.reading {
            reader.due = false;
            reader.reading = true;
            let (file, tx) = (opened.file.clone(), reader.tx.clone());
            IoTaskPool::get()
                .spawn(async move {
                    let page = page_of(&file);
                    let _ = tx.send((file, page));
                })
                .detach();
        }
    }
}

/// `file` as the editor shows it.
fn page_of(file: &Path) -> Page {
    let size = match std::fs::metadata(file) {
        Ok(found) => found.len(),
        Err(e) => return Page::Gone(e.to_string()),
    };
    if size > LARGEST {
        return Page::TooBig(size);
    }
    match std::fs::read(file) {
        Err(e) => Page::Gone(e.to_string()),
        Ok(bytes) if bytes.contains(&0) => Page::Binary,
        Ok(bytes) => {
            Page::Text(String::from_utf8_lossy(&bytes).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bevy::app::TaskPoolPlugin;

    use super::*;

    #[test]
    fn a_file_is_shown_by_its_kind() {
        let all = hamlet_testing::Scratch::new("ui-editor-pages");
        let text = all.join("notes.md");
        std::fs::write(&text, b"pears\n\xffapples").unwrap();
        std::fs::write(all.join("app"), b"\x7fELF\0\0").unwrap();
        let large = vec![b'a'; LARGEST as usize + 1];
        std::fs::write(all.join("big.log"), &large).unwrap();

        assert_eq!(
            page_of(&text),
            Page::Text("pears\n\u{fffd}apples".into())
        );
        assert_eq!(page_of(&all.join("app")), Page::Binary);
        assert_eq!(
            page_of(&all.join("big.log")),
            Page::TooBig(LARGEST + 1)
        );
        assert!(matches!(
            page_of(&all.join("gone.md")),
            Page::Gone(_)
        ));
    }

    #[test]
    fn the_open_file_follows_the_disk() {
        let all = hamlet_testing::Scratch::new("ui-editor-follows");
        let file = all.join("plan.md");
        std::fs::write(&file, "one").unwrap();
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), plugin));
        let hamlet = app.world_mut().spawn_empty().id();
        open(app.world_mut(), hamlet, &file, &all);
        let until = |app: &mut App, want: Page| {
            let start = Instant::now();
            loop {
                app.update();
                let page =
                    &app.world().get::<Opened>(hamlet).unwrap().page;
                if *page == want {
                    return;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(10),
                    "shows {page:?}, not {want:?}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        };

        until(&mut app, Page::Text("one".into()));
        std::fs::write(&file, "two").unwrap();
        until(&mut app, Page::Text("two".into()));
        std::fs::remove_file(&file).unwrap();
        let start = Instant::now();
        while !matches!(
            app.world().get::<Opened>(hamlet).unwrap().page,
            Page::Gone(_)
        ) {
            assert!(start.elapsed() < Duration::from_secs(10));
            app.update();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
