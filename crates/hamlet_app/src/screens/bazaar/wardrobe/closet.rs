//! The closet: the files of what is worn, downloaded from the
//! bazaar's server a part at a time and handed to the realm, which
//! keeps them in memory in [`Looks`]. A file is downloaded once the
//! chief owns its asset; one no one wears any more is let go of.

use std::collections::{BTreeMap, BTreeSet};

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use hamlet_realm::dress::Looks;

use super::{Call, Dressed, Wardrobe, asset_of};
use crate::bazaar::errand::Fetched;
use crate::bazaar::market::client::bytes;
use crate::bazaar::market::{self, Market};
use crate::bazaar::shelf::now;

/// The most bytes of a file asked for at once.
pub(crate) const PART: u64 = 512 * 1024;

/// The wait before a file that could not be downloaded is asked for
/// again, in milliseconds.
pub(crate) const RETRY: u64 = 30_000;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Closet>().add_systems(Update, tend);
}

/// The file being downloaded, and those that could not be lately.
#[derive(Resource, Default)]
pub(crate) struct Closet {
    pub download: Option<Download>,
    /// When each file that could not be downloaded was last asked
    /// for, in milliseconds since 1970.
    missed: BTreeMap<String, u64>,
}

/// A file being downloaded.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Download {
    /// Its name, the `id@version` worn.
    pub file: String,
    /// The bytes come so far.
    bytes: Vec<u8>,
}

impl Download {
    /// The download of the file `file`, from its first byte.
    pub fn of(file: String) -> Self {
        Download {
            file,
            bytes: Vec::new(),
        }
    }

    /// Takes in the next part; true once it was the last, shorter
    /// than [`PART`].
    pub fn took(&mut self, part: Vec<u8>) -> bool {
        let last = (part.len() as u64) < PART;
        self.bytes.extend(part);
        last
    }

    /// The byte the next part begins at.
    pub fn from(&self) -> u64 {
        self.bytes.len() as u64
    }
}

impl Closet {
    /// Whether the file `file` could not be downloaded lately.
    pub fn missed(&self, file: &str) -> bool {
        self.missed.contains_key(file)
    }
}

/// Whether the file `file` is in the realm.
pub(crate) fn has(world: &World, file: &str) -> bool {
    world
        .get_resource::<Looks>()
        .is_some_and(|looks| looks.files.contains_key(file))
}

/// Lets go of the files no one wears, and begins to download the
/// first one worn and owned that the realm lacks, unless it could not
/// be lately.
fn tend(world: &mut World) {
    let Some(looks) = world.get_resource::<Looks>() else {
        return;
    };
    let worn = world.resource::<Dressed>().files();
    let kept =
        worn.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let unworn =
        looks.files.keys().any(|file| !kept.contains(file.as_str()));
    if unworn {
        world
            .resource_mut::<Looks>()
            .files
            .retain(|file, _| kept.contains(file.as_str()));
    }
    if world.resource::<Closet>().download.is_some()
        || !world.resource::<Market>().signed_in()
    {
        return;
    }
    let now = now();
    let wardrobe = world.resource::<Wardrobe>();
    let closet = world.resource::<Closet>();
    let next = worn.into_iter().find(|file| {
        wardrobe.owns(asset_of(file))
            && !has(world, file)
            && closet
                .missed
                .get(file)
                .is_none_or(|at| now.saturating_sub(*at) >= RETRY)
    });
    let Some(file) = next else {
        return;
    };
    let mut closet = world.resource_mut::<Closet>();
    closet.missed.remove(&file);
    closet.download = Some(Download::of(file));
    fetch(world);
}

/// Asks for the next part of the file downloaded.
fn fetch(world: &mut World) {
    let Some(download) = &world.resource::<Closet>().download else {
        return;
    };
    let (file, from) = (download.file.clone(), download.from());
    let request = world.resource::<Market>().server.asset_file_part(
        asset_of(&file),
        from,
        PART,
    );
    let call = market::Call::Wardrobe(Call::Part(file, from));
    market::send(world, call, request);
}

/// Takes in the part of the file `file` from the byte `from` on.
pub(crate) fn came(
    world: &mut World,
    file: String,
    from: u64,
    answer: Fetched,
) {
    let mut closet = world.resource_mut::<Closet>();
    let Some(download) = &mut closet.download else {
        return;
    };
    if download.file != file || download.from() != from {
        return;
    }
    let part = match bytes(answer) {
        Ok(part) => part,
        Err(why) => {
            bevy::log::warn!(
                "the look {file} did not come: {}",
                why.words()
            );
            closet.download = None;
            closet.missed.insert(file, now());
            return;
        }
    };
    if !download.took(part) {
        return fetch(world);
    }
    let Some(download) = closet.download.take() else {
        return;
    };
    if let Some(mut looks) = world.get_resource_mut::<Looks>() {
        looks.files.insert(download.file, download.bytes);
    }
}
