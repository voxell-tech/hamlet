//! The asset whose page is open, in the realm's viewer: its file
//! fetched veiled from the bazaar's server, signed in or not,
//! unveiled in memory and worn by a [`Dummy`], let go of once the
//! page is not shown.

use std::collections::BTreeMap;

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use hamlet_realm::viewer::{Dummy, Viewer};
use hamlet_realm::wardrobe::Asset;

use super::{Call, Fitting, ask, asset_of, file_of};
use crate::bazaar::errand::Fetched;
use crate::bazaar::market::client::bytes;
use crate::bazaar::shelf::{Shelf, Stock};
use crate::bazaar::{Stall, StallShown};

/// The start of a veiled file, before its key.
const MAGIC: &[u8; 4] = b"HLV1";
const KEY: usize = 16;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Viewing>().add_systems(Update, tend);
}

/// The file of the asset whose page is open, as it is fetched.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Viewing {
    /// Its `id@version`.
    pub file: Option<String>,
    pub sight: Sight,
    /// Files given by hand, by asset id, in place of the server's.
    given: BTreeMap<String, Vec<u8>>,
}

/// How the file of the open asset stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Sight {
    #[default]
    Fetching,
    Shown,
    Failed,
}

/// The asset whose page is shown, if one is.
fn open(world: &World) -> Option<Asset> {
    if world.resource::<StallShown>().0 != Stall::Assets
        || world.resource::<Fitting>().on
    {
        return None;
    }
    let id = world.resource::<Shelf<Asset>>().open.clone()?;
    world.resource::<Stock<Asset>>().get(&id).cloned()
}

/// Asks for the file of the asset whose page opened, and lets go of
/// the one shown once its page is not.
fn tend(world: &mut World) {
    let open = open(world);
    let wanted = open.as_ref().map(file_of);
    if world.resource::<Viewing>().file == wanted {
        return;
    }
    let mut viewing = world.resource_mut::<Viewing>();
    viewing.file = wanted.clone();
    viewing.sight = Sight::Fetching;
    if let Some(mut viewer) = world.get_resource_mut::<Viewer>() {
        viewer.clear();
    }
    let (Some(asset), Some(file)) = (open, wanted) else {
        return;
    };
    match world.resource::<Viewing>().given.get(&asset.id).cloned() {
        Some(bytes) => show(world, &asset, file, bytes),
        None => ask(world, Call::View(file)),
    }
}

/// Takes in the veiled file `file`.
pub(crate) fn came(world: &mut World, file: String, answer: Fetched) {
    if world.resource::<Viewing>().file.as_ref() != Some(&file) {
        return;
    }
    let asset =
        open(world).filter(|asset| asset.id == asset_of(&file));
    let unveiled =
        bytes(answer).map_err(|why| why.words()).and_then(|veiled| {
            unveil(&veiled).ok_or_else(|| "it was not veiled".into())
        });
    match (asset, unveiled) {
        (Some(asset), Ok(bytes)) => show(world, &asset, file, bytes),
        (_, Err(why)) => {
            bevy::log::warn!(
                "the look {file} cannot be shown: {why}"
            );
            world.resource_mut::<Viewing>().sight = Sight::Failed;
        }
        _ => {}
    }
}

/// Has the realm's viewer show `asset` worn, its file `bytes`.
fn show(
    world: &mut World,
    asset: &Asset,
    file: String,
    bytes: Vec<u8>,
) {
    let Some(mut viewer) = world.get_resource_mut::<Viewer>() else {
        world.resource_mut::<Viewing>().sight = Sight::Failed;
        return;
    };
    let dummy = Dummy::new(
        asset.target,
        asset.category,
        format!("viewer/{file}"),
        bytes,
    );
    viewer.show(file, dummy);
    world.resource_mut::<Viewing>().sight = Sight::Shown;
}

/// The file in `veiled` as `GET /v1/assets/:id/view` veils it;
/// None when it is not veiled so.
pub(crate) fn unveil(veiled: &[u8]) -> Option<Vec<u8>> {
    let rest = veiled.strip_prefix(MAGIC)?;
    let (key, bytes) = rest.split_at_checked(KEY)?;
    let mut state =
        key.iter().fold(0x811c_9dc5_u32, |state, byte| {
            (state ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
        });
    Some(
        bytes
            .iter()
            .zip(key.iter().cycle())
            .map(|(byte, key)| {
                state = state
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                byte ^ (state >> 24) as u8 ^ key
            })
            .collect(),
    )
}

/// Gives the files of the assets of `files`, by id, as if the
/// bazaar's server had sent them, so nothing is asked of it.
pub(crate) fn stage(
    world: &mut World,
    files: Vec<(String, Vec<u8>)>,
) {
    world.resource_mut::<Viewing>().given =
        files.into_iter().collect();
}
