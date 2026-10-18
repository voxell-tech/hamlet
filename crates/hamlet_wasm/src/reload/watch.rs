//! Looking at the files of the installed plugins for a change.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use bevy::prelude::*;
use hamlet_sdk::capability::Uninstalled;

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

/// On the entity of an installed plugin: its folder, and how its
/// files looked when they were last looked at.
#[derive(Component, Debug)]
pub struct Watch {
    pub folder: PathBuf,
    pub stamp: Stamp,
}

impl Watch {
    pub fn new(folder: PathBuf) -> Self {
        let stamp = Stamp::of(&folder);
        Self { folder, stamp }
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
        if watch.stamp != stamp {
            watch.stamp = stamp;
            modified.write(Modified(plugin));
        }
    }
}
