//! Looking at the install folder for a plugin folder that appears
//! while the app runs, by hand or from a build.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::{Instant, SystemTime};

use bevy::prelude::*;
use hamlet_base::HomeDir;
use hamlet_sdk::capability::Appeared;
use hamlet_sdk::plugin::PluginInfo;

use super::Polling;

/// The name, size and modification time of each thing in a folder.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
struct Listing(Vec<(OsString, u64, Option<SystemTime>)>);

impl Listing {
    fn of(folder: &Path) -> Self {
        let mut things = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let meta = entry.metadata().ok()?;
                Some((
                    entry.file_name(),
                    meta.len(),
                    meta.modified().ok(),
                ))
            })
            .collect::<Vec<_>>();
        things.sort();
        Self(things)
    }
}

/// The folders of the install folder that no plugin has, each as it
/// was at the last look and whether its host was told of it.
#[derive(Default)]
pub struct Seen {
    looked: bool,
    folders: HashMap<OsString, (Listing, bool)>,
}

/// Looks at the install folder once in [`Polling::interval`] and
/// tells of each folder no plugin has once it is the same at two
/// looks in a row, so that a folder being copied is read when it is
/// whole. What is there at the first look was found at start.
pub fn look(
    polling: Res<Polling>,
    home: Option<Res<HomeDir>>,
    plugins: Query<&PluginInfo>,
    mut last: Local<Option<Instant>>,
    mut seen: Local<Seen>,
    mut commands: Commands,
) {
    let now = Instant::now();
    if last.is_some_and(|at| now - at < polling.interval()) {
        return;
    }
    *last = Some(now);
    let Some(home) = home else {
        return;
    };
    let folders = std::fs::read_dir(home.0.plugins())
        .into_iter()
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.is_dir())
        .filter_map(|path| Some((path.file_name()?.to_owned(), path)))
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            !name.starts_with('.')
                && !plugins.iter().any(|info| info.id == name)
        })
        .collect::<Vec<_>>();
    let first = !seen.looked;
    seen.looked = true;
    seen.folders.retain(|name, _| {
        folders.iter().any(|(held, _)| held == name)
    });
    for (name, path) in folders {
        let listing = Listing::of(&path);
        let entry = seen.folders.entry(name.clone());
        let (was, told) =
            entry.or_insert((Listing::default(), first));
        if *was != listing {
            *was = listing;
            *told = first;
            continue;
        }
        if !*told {
            *told = true;
            commands.trigger(Appeared {
                plugin: name.to_string_lossy().into_owned(),
                asked: false,
            });
        }
    }
}
