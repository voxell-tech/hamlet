//! Looking at the files of the installed plugins for a change.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use bevy::prelude::*;
use hamlet_sdk::capability::Uninstalled;
use sha2::{Digest as _, Sha256};

use super::Polling;
use crate::install::FILES;

/// The modification time and size of each file of a plugin's folder.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Stamp(Vec<Option<(SystemTime, u64)>>);

impl Stamp {
    pub fn of(folder: &Path) -> Self {
        Self(
            FILES
                .iter()
                .map(|file| {
                    let meta =
                        std::fs::metadata(folder.join(file)).ok()?;
                    Some((meta.modified().ok()?, meta.len()))
                })
                .collect(),
        )
    }
}

/// A hash of the names and contents of the files of a plugin's
/// folder. The default is that of no folder.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Digest(Vec<u8>);

impl Digest {
    pub fn of(folder: &Path) -> Self {
        let mut hash = Sha256::new();
        for file in FILES {
            hash.update(file.as_bytes());
            if let Ok(bytes) = std::fs::read(folder.join(file)) {
                hash.update(bytes);
            }
        }
        Self(hash.finalize().to_vec())
    }
}

/// On the entity of an installed plugin: its folder, and how its
/// files looked when they were last looked at.
#[derive(Component, Debug)]
pub struct Watch {
    pub folder: PathBuf,
    pub stamp: Stamp,
    pub digest: Digest,
}

impl Watch {
    pub fn new(folder: PathBuf) -> Self {
        let stamp = Stamp::of(&folder);
        let digest = Digest::of(&folder);
        Self {
            folder,
            stamp,
            digest,
        }
    }
}

/// The files of the plugin on this entity are not as they were, or
/// the plugin is to be looked at again.
#[derive(Message)]
pub struct Modified(pub Entity);

/// Looks at the folder of each installed plugin once in
/// [`Polling::interval`].
pub fn poll(
    polling: Res<Polling>,
    mut last: Local<Option<Instant>>,
    mut plugins: Query<(Entity, &mut Watch), Without<Uninstalled>>,
    mut modified: MessageWriter<Modified>,
) {
    let now = Instant::now();
    if last.is_some_and(|at| now - at < polling.interval()) {
        return;
    }
    *last = Some(now);
    for (plugin, mut watch) in &mut plugins {
        let stamp = Stamp::of(&watch.folder);
        if watch.stamp == stamp {
            continue;
        }
        watch.stamp = stamp;
        // A file saved again with the same contents is not a new
        // version.
        let digest = Digest::of(&watch.folder);
        if watch.digest != digest {
            watch.digest = digest;
            modified.write(Modified(plugin));
        }
    }
}
