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
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::tasks::IoTaskPool;
use hamlet_client::realm::{RealmFolder, Works};
use hamlet_desktop::files::{self, Reading, Watch};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use super::tree::{Listing, Tree, touched};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ShowHidden>()
        .add_systems(Update, (let_go, keep).chain());
}

/// Whether the explorers list what VS Code leaves out, as `.git`.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub(crate) struct ShowHidden(pub bool);

/// A hamlet's explorer: the tree of the folder it works on.
#[derive(Component)]
pub(crate) struct Explorer(pub Tree);

/// A folder, and its entries as read.
type Read = (PathBuf, Listing);

/// The keeper of a hamlet's [`Explorer`], which holds it to the disk.
#[derive(Component)]
struct Keeper {
    /// The folder as the server names it.
    folder: PathBuf,
    /// The paths never listed: the realm's folder, which holds the
    /// lodges.
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
    fn new(folder: PathBuf, hidden: Vec<PathBuf>) -> Self {
        let watch = Watch::new()
            .inspect_err(|e| {
                bevy::log::warn!("could not watch for changes: {e}");
            })
            .ok();
        let (tx, rx) = unbounded_channel();
        Keeper {
            folder,
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

/// Gives `hamlet` an explorer of `folder`. One it has of that folder
/// already is kept, and what it shows read again.
pub(crate) fn open(world: &mut World, hamlet: Entity, folder: &Path) {
    if world
        .get::<Keeper>(hamlet)
        .is_some_and(|keeper| keeper.folder == folder)
    {
        refresh(world, hamlet);
        return;
    }
    let root = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    // Kept out of sight as it is kept out of the agents' reach.
    let hidden = world
        .get_resource::<RealmFolder>()
        .and_then(|realm| Path::new(&realm.0).canonicalize().ok())
        .into_iter()
        .collect();
    if let Ok(mut hamlet) = world.get_entity_mut(hamlet) {
        hamlet.insert((
            Explorer(Tree::new(root)),
            Keeper::new(folder.to_path_buf(), hidden),
        ));
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
            let folder = world
                .get::<Works>(hamlet)
                .and_then(|works| works.folder.as_ref())
                .map(PathBuf::from);
            let moved =
                world.get::<Keeper>(hamlet).is_some_and(|keeper| {
                    folder.as_ref() != Some(&keeper.folder)
                });
            if moved {
                world
                    .entity_mut(hamlet)
                    .remove::<(Explorer, Keeper)>();
                super::editor::close(world, hamlet);
            }
        });
    }
}

/// Takes in what was read, watches the folders shown, and reads
/// those that came into sight or changed on the disk.
fn keep(
    mut explorers: Query<(&mut Explorer, &mut Keeper)>,
    every: Res<ShowHidden>,
) {
    for (mut explorer, mut keeper) in &mut explorers {
        let Keeper {
            folder: _,
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
        if every.is_changed() {
            due.extend(shown.iter().cloned());
        }
        if explorer.is_changed() {
            let now = explorer.0.shown();
            due.extend(now.difference(shown).cloned());
            *shown = now;
        }
        if let Some(watch) = watch.as_mut() {
            // Each update, so a folder that came back is watched
            // again.
            watch.follow(shown);
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
                    Reading {
                        ignored,
                        all: every.0,
                    },
                    hidden.clone(),
                    tx.clone(),
                );
            }
            false
        });
    }
}

/// Reads `folder` off the main thread as `reading` asks, and sends
/// what it holds by `tx`.
fn read(
    folder: PathBuf,
    reading: Reading,
    hidden: Vec<PathBuf>,
    tx: UnboundedSender<Read>,
) {
    IoTaskPool::get()
        .spawn(async move {
            let found = files::list(&folder, &hidden, reading);
            let listing = match found {
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
        std::fs::create_dir(folder.join(".git")).unwrap();
        std::fs::write(folder.join("a.txt"), "").unwrap();
        std::fs::write(folder.join("src/main.rs"), "").unwrap();
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), plugin));
        let hamlet = app.world_mut().spawn_empty().id();
        open(app.world_mut(), hamlet, &folder);

        until(&mut app, hamlet, &["src", "a.txt"]);
        app.world_mut().resource_mut::<ShowHidden>().0 = true;
        until(&mut app, hamlet, &[".git", "src", "a.txt"]);
        app.world_mut().resource_mut::<ShowHidden>().0 = false;
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

        // A folder removed and made again, as a checkout does, is
        // watched again.
        std::fs::remove_dir_all(folder.join("src")).unwrap();
        until(&mut app, hamlet, &[]);
        std::fs::create_dir(folder.join("src")).unwrap();
        std::fs::write(folder.join("src/new.rs"), "").unwrap();
        until(&mut app, hamlet, &["src", "new.rs"]);
        std::fs::write(folder.join("src/again.rs"), "").unwrap();
        until(&mut app, hamlet, &["src", "again.rs", "new.rs"]);
    }
}
