//! Each hamlet's explorer kept up with the disk: its folders read
//! off the main thread, and read again when the system says they
//! changed or when they come into sight.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::tasks::IoTaskPool;
use hamlet_core::agents::Works;
use hamlet_core::core::HomeDir;
use hamlet_desktop::files::{self, Watch};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use super::tree::{Listing, Tree, touched};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (let_go, keep).chain());
}

/// A hamlet's explorer: the tree of the folder it works on.
#[derive(Component)]
pub(crate) struct Explorer(pub Tree);

/// A folder and what it was read to hold.
type Read = (PathBuf, Listing);

/// What keeps a hamlet's [`Explorer`] up with the disk.
#[derive(Component)]
struct Keeper {
    /// The paths never listed: Hamlet's own home.
    hidden: Vec<PathBuf>,
    /// None where the system would not watch.
    watch: Option<Watch>,
    /// The tree's shown folders, when it last changed.
    shown: HashSet<PathBuf>,
    /// The folders to read, or to read again once the read under way
    /// is done.
    due: HashSet<PathBuf>,
    reading: HashSet<PathBuf>,
    tx: UnboundedSender<Read>,
    rx: UnboundedReceiver<Read>,
}

impl Keeper {
    fn new(hidden: Vec<PathBuf>) -> Self {
        let watch = Watch::new()
            .inspect_err(|e| {
                bevy::log::warn!("could not watch for changes: {e}");
            })
            .ok();
        let (tx, rx) = unbounded_channel();
        Keeper {
            hidden,
            watch,
            shown: HashSet::new(),
            due: HashSet::new(),
            reading: HashSet::new(),
            tx,
            rx,
        }
    }
}

/// Gives `hamlet` an explorer of `folder`, unless it has one of that
/// folder already.
pub(crate) fn open(world: &mut World, hamlet: Entity, folder: &Path) {
    let root = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    if world
        .get::<Explorer>(hamlet)
        .is_some_and(|explorer| explorer.0.root() == root)
    {
        return;
    }
    let hidden = world
        .get_resource::<HomeDir>()
        .and_then(|home| home.0.root.canonicalize().ok())
        .into_iter()
        .collect();
    if let Ok(mut hamlet) = world.get_entity_mut(hamlet) {
        hamlet
            .insert((Explorer(Tree::new(root)), Keeper::new(hidden)));
    }
}

/// Reads every folder of `hamlet`'s explorer that is shown again.
pub(crate) fn refresh(world: &mut World, hamlet: Entity) {
    if let Some(mut keeper) = world.get_mut::<Keeper>(hamlet) {
        let shown = keeper.shown.clone();
        keeper.due.extend(shown);
    }
}

/// Lets go of the explorer of a hamlet that works on another folder
/// now, or on none.
fn let_go(
    hamlets: Query<Entity, (Changed<Works>, With<Explorer>)>,
    mut commands: Commands,
) {
    for hamlet in &hamlets {
        // Looked at once the command runs: by then the explorer may
        // have been opened again on the new folder.
        commands.queue(move |world: &mut World| {
            let folder =
                world.get::<Works>(hamlet).and_then(|works| {
                    works.0.folder.as_ref()?.canonicalize().ok()
                });
            let moved = world.get::<Explorer>(hamlet).is_some_and(
                |explorer| {
                    folder.as_deref() != Some(explorer.0.root())
                },
            );
            if moved {
                world
                    .entity_mut(hamlet)
                    .remove::<(Explorer, Keeper)>();
            }
        });
    }
}

/// Takes in what was read, watches the folders shown, and reads
/// those that came into sight or changed on the disk.
fn keep(mut explorers: Query<(&mut Explorer, &mut Keeper)>) {
    for (mut explorer, mut keeper) in &mut explorers {
        let Keeper {
            hidden,
            watch,
            shown,
            due,
            reading,
            tx,
            rx,
        } = &mut *keeper;
        while let Ok((folder, listing)) = rx.try_recv() {
            reading.remove(&folder);
            // Rewritten only when it differs, so the rows are not
            // drawn again at every read.
            if explorer.0.listing(&folder) != Some(&listing) {
                due.extend(explorer.0.read(folder, listing));
            }
        }
        if explorer.is_changed() {
            let now = explorer.0.shown();
            due.extend(now.difference(shown).cloned());
            if let Some(watch) = watch.as_mut() {
                watch.follow(&now);
            }
            *shown = now;
        }
        if let Some(watch) = watch.as_mut() {
            for path in watch.changed() {
                due.extend(touched(shown, &path));
            }
        }
        due.retain(|folder| {
            if reading.contains(folder) {
                return true;
            }
            if shown.contains(folder) {
                reading.insert(folder.clone());
                let ignored = explorer.0.ignored(folder);
                read(
                    folder.clone(),
                    ignored,
                    hidden.clone(),
                    tx.clone(),
                );
            }
            false
        });
    }
}

/// Reads `folder` off the main thread, and sends what it holds by
/// `tx`.
fn read(
    folder: PathBuf,
    ignored: bool,
    hidden: Vec<PathBuf>,
    tx: UnboundedSender<Read>,
) {
    IoTaskPool::get()
        .spawn(async move {
            let listing = match files::list(&folder, &hidden, ignored)
            {
                Ok(entries) => Listing::Read(entries),
                Err(e) => Listing::Failed(e.to_string()),
            };
            let _ = tx.send((folder, listing));
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bevy::app::TaskPoolPlugin;

    use super::*;
    use crate::explorer::tree::Line;

    /// The names of the entries shown, from the top.
    fn names(app: &App, hamlet: Entity) -> Vec<String> {
        let explorer = app.world().get::<Explorer>(hamlet).unwrap();
        explorer
            .0
            .rows()
            .into_iter()
            .filter_map(|row| match row.line {
                Line::Entry { entry, .. } => Some(entry.name),
                _ => None,
            })
            .collect()
    }

    /// Updates `app` until `hamlet`'s explorer shows `want`, failing
    /// after a few seconds.
    fn until(app: &mut App, hamlet: Entity, want: &[&str]) {
        let start = Instant::now();
        while names(app, hamlet) != want {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "shows {:?}, not {want:?}",
                names(app, hamlet)
            );
            app.update();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_tree_follows_the_disk() {
        let folder = hamlet_testing::Scratch::new("ui-explorer-sync");
        std::fs::create_dir(folder.join("src")).unwrap();
        std::fs::write(folder.join("a.txt"), "").unwrap();
        std::fs::write(folder.join("src/main.rs"), "").unwrap();
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), plugin));
        let hamlet = app.world_mut().spawn_empty().id();
        open(app.world_mut(), hamlet, &folder);

        until(&mut app, hamlet, &["src", "a.txt"]);
        app.world_mut()
            .get_mut::<Explorer>(hamlet)
            .unwrap()
            .0
            .toggle(&folder.join("src"));
        until(&mut app, hamlet, &["src", "main.rs", "a.txt"]);

        std::fs::write(folder.join("src/lib.rs"), "").unwrap();
        until(
            &mut app,
            hamlet,
            &["src", "lib.rs", "main.rs", "a.txt"],
        );
        std::fs::remove_file(folder.join("a.txt")).unwrap();
        until(&mut app, hamlet, &["src", "lib.rs", "main.rs"]);
    }
}
