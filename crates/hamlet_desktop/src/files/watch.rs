//! Word of a change in the folders shown.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// The system's watch over some folders, each for its own entries
/// alone.
pub struct Watch {
    watcher: RecommendedWatcher,
    changed: UnboundedReceiver<PathBuf>,
    /// The folders followed, as they were named.
    folders: HashSet<PathBuf>,
    /// Each real folder watched, with the folders followed that are
    /// it. The system watches and tells by real path, and a link
    /// makes another name for the same folder.
    real: HashMap<PathBuf, Vec<PathBuf>>,
}

impl Watch {
    /// A watch over no folder yet.
    pub fn new() -> notify::Result<Self> {
        let (tx, changed) = unbounded_channel();
        let watcher = notify::recommended_watcher(
            move |event: notify::Result<Event>| {
                for path in event.map(|e| e.paths).unwrap_or_default()
                {
                    let _ = tx.send(path);
                }
            },
        )?;
        Ok(Watch {
            watcher,
            changed,
            folders: HashSet::new(),
            real: HashMap::new(),
        })
    }

    /// Watches `folders`, and no other.
    pub fn follow(&mut self, folders: &HashSet<PathBuf>) {
        if *folders == self.folders {
            return;
        }
        let mut real = HashMap::<PathBuf, Vec<PathBuf>>::new();
        for folder in folders {
            if let Ok(path) = folder.canonicalize() {
                real.entry(path).or_default().push(folder.clone());
            }
        }
        let mut paths = self.watcher.paths_mut();
        // A folder that is gone can be neither watched nor let go.
        for gone in
            self.real.keys().filter(|path| !real.contains_key(*path))
        {
            let _ = paths.remove(gone);
        }
        for new in
            real.keys().filter(|path| !self.real.contains_key(*path))
        {
            let _ = paths.add(new, RecursiveMode::NonRecursive);
        }
        if let Err(e) = paths.commit() {
            bevy::log::warn!("could not watch the folders: {e}");
        }
        self.folders = folders.clone();
        self.real = real;
    }

    /// The paths changed since this was last asked, each named once
    /// under every name its folder is followed by.
    pub fn changed(&mut self) -> HashSet<PathBuf> {
        let mut changed = HashSet::new();
        while let Ok(path) = self.changed.try_recv() {
            changed.extend(self.named(&path));
        }
        changed
    }

    /// `path`, a real path, under the names of the folders followed.
    fn named(&self, path: &Path) -> HashSet<PathBuf> {
        let mut named = HashSet::new();
        if let Some(folders) = self.real.get(path) {
            named.extend(folders.iter().cloned());
        }
        if let (Some(parent), Some(name)) =
            (path.parent(), path.file_name())
            && let Some(folders) = self.real.get(parent)
        {
            named.extend(
                folders.iter().map(|folder| folder.join(name)),
            );
        }
        if named.is_empty() {
            named.insert(path.to_path_buf());
        }
        named
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_change_is_told_under_every_name_of_its_folder() {
        let all = hamlet_testing::Scratch::new("desktop-watch");
        std::fs::create_dir(all.join("v2")).unwrap();
        std::os::unix::fs::symlink(
            all.join("v2"),
            all.join("current"),
        )
        .unwrap();
        let mut watch = Watch::new().unwrap();
        watch.follow(&HashSet::from([
            all.to_path_buf(),
            all.join("v2"),
            all.join("current"),
        ]));

        assert_eq!(
            watch.named(&all.join("v2/a.txt")),
            HashSet::from([
                all.join("current/a.txt"),
                all.join("v2/a.txt")
            ])
        );
        assert_eq!(
            watch.named(&all.join("v2")),
            HashSet::from([all.join("current"), all.join("v2")])
        );
    }
}
