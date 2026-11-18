//! The files and folders the field attaches after `#`: those in the
//! folder of the hamlet of the agent the chat is with, while that
//! folder is on this machine.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use bevy::app::{App, PreUpdate};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::world::World;
use hamlet_client::realm::{LivesIn, Works};
use hamlet_protocol::signs::{self, Sign};

use super::{ChatField, Entry, Pick, Source, field, follow};
use crate::PanelSystems;

/// The most files and folders read from a hamlet's folder.
const MOST: usize = 5000;

/// The folders never read: what tools build and keep.
const SKIPPED: [&str; 3] = ["target", "node_modules", "dist"];

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        PreUpdate,
        index.before(follow).in_set(PanelSystems::Bind),
    );
}

pub(super) struct Files;

impl Source for Files {
    const SIGN: Sign = Sign::File;
    const LISTS: &str = "files";

    fn entries(world: &World, hub: Entity, rest: &str) -> Vec<Entry> {
        let Some(index) = world.get::<Index>(hub) else {
            return Vec::new();
        };
        let Some(folder) = &index.folder else {
            return Vec::new();
        };
        let rest = rest.to_lowercase();
        let mut found = index
            .paths
            .iter()
            .filter_map(|path| {
                let shown = path.to_string_lossy().replace('\\', "/");
                let lower = shown.to_lowercase();
                let name = lower.rsplit('/').next().unwrap_or(&lower);
                let rank = match () {
                    () if name.starts_with(&rest) => 0,
                    () if lower.contains(&rest) => 1,
                    () => return None,
                };
                Some((rank, shown.matches('/').count(), shown, path))
            })
            .collect::<Vec<_>>();
        found.sort();
        found
            .into_iter()
            .take(super::MOST)
            .map(|(_, _, shown, path)| Entry {
                title: format!("#{shown}"),
                detail: String::new(),
                pick: Pick::Attach(folder.join(path)),
            })
            .collect()
    }
}

/// The files and folders of the folder a Hub last read, relative to
/// it.
#[derive(Component, Default)]
struct Index {
    folder: Option<PathBuf>,
    paths: Vec<PathBuf>,
    /// The start of the word it was read for.
    word: Option<usize>,
}

/// Reads the hamlet's folder again for a Hub each time a word of `#`
/// starts in its field.
fn index(world: &mut World) {
    let mut fields = world.query::<&ChatField>();
    let mut hubs =
        fields.iter(world).map(|field| field.0).collect::<Vec<_>>();
    hubs.sort();
    hubs.dedup();
    for hub in hubs {
        index_in(world, hub);
    }
}

/// Reads the hamlet's folder again for `hub` when a word of `#`
/// starts in its field.
fn index_in(world: &mut World, hub: Entity) {
    let word = field::with_field(world, hub, |field| {
        let caret = field.caret()?;
        let text = field.plain().text;
        signs::at_caret(&text, caret)
            .filter(|token| token.sign == Sign::File)
            .map(|token| token.start)
    })
    .flatten();
    let Ok(mut held) = world.get_entity_mut(hub) else {
        return;
    };
    let mut was = held.entry::<Index>().or_default().into_mut();
    if word.is_none() || word == was.word {
        was.word = word;
        return;
    }
    let folder =
        hamlet_folder(world, hub).filter(|folder| folder.is_dir());
    let paths = folder.as_deref().map(read).unwrap_or_default();
    world.entity_mut(hub).insert(Index {
        folder,
        paths,
        word,
    });
}

/// The folder of the hamlet of the agent `hub`'s chat is with.
fn hamlet_folder(world: &World, hub: Entity) -> Option<PathBuf> {
    let agent = crate::convo::of(world, hub)?.agent;
    let hamlet = world.get::<LivesIn>(agent)?.0;
    world
        .get::<Works>(hamlet)?
        .folder
        .as_ref()
        .map(PathBuf::from)
}

/// The files and folders under `folder`, nearest first, without the
/// hidden ones and those of [`SKIPPED`].
fn read(folder: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut waiting = VecDeque::from([PathBuf::new()]);
    while let Some(under) = waiting.pop_front() {
        let Ok(entries) = std::fs::read_dir(folder.join(&under))
        else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                || SKIPPED.contains(&name.as_ref())
            {
                continue;
            }
            let path = under.join(&*name);
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                waiting.push_back(path.clone());
            }
            paths.push(path);
            if paths.len() >= MOST {
                return paths;
            }
        }
    }
    paths
}
