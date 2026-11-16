//! The images nothing uses any more, taken away once a day has gone
//! by without them.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use bevy_ecs::system::ResMut;
use diesel::dsl::exists;
use diesel::prelude::*;
use hamlet_protocol::client::ImageId;
use hamlet_store::Db;
use hamlet_store::schema::{input, item, marker};

use super::SEEN;
use super::store::Images;
use crate::layout::Layout;
use crate::realm::Realm;

/// The time an image nothing uses is kept.
const GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// Takes away the images nothing uses any more: those no kept input,
/// transcript item or tool result names and no agent's folder holds,
/// untouched for a day, which includes uploads never sent with a
/// line.
pub(crate) fn sweep(mut realm: ResMut<Realm>) -> Result<()> {
    let layout = realm.layout().clone();
    let images = Images::new(layout.images());
    let held = held_by_agents(&layout);
    let db = realm.store().connection();
    for (id, at) in images.kept() {
        let idle = at.elapsed().is_ok_and(|age| age >= GRACE);
        if idle && !held.contains(&id) && !is_used(db, id.as_str())? {
            images.remove(&id)?;
        }
    }
    Ok(())
}

/// The check that a kept input, a transcript item or a tool's result
/// names the image or file `image`.
pub(crate) fn is_used(db: &mut Db, image: &str) -> Result<bool> {
    let named = format!("%{image}%");
    Ok(diesel::select(exists(
        input::table.filter(input::mark.like(&named)),
    ))
    .get_result::<bool>(db)?
        || diesel::select(exists(
            item::table.filter(item::body.like(&named)),
        ))
        .get_result::<bool>(db)?
        || diesel::select(exists(
            marker::table.filter(marker::outcome.like(&named)),
        ))
        .get_result::<bool>(db)?)
}

/// The images the folders of the agents' images hold.
fn held_by_agents(layout: &Layout) -> HashSet<ImageId> {
    let children = |path: PathBuf| {
        std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<_>>()
    };
    let mut lodges = vec![layout.hand()];
    lodges.extend(children(layout.workers()));
    for hamlet in children(layout.hamlets()) {
        lodges.extend(children(hamlet));
    }
    lodges
        .into_iter()
        .flat_map(|lodge| children(lodge.join(SEEN)))
        .filter_map(|file| {
            file.file_name()?
                .to_str()?
                .split_once('.')?
                .0
                .parse()
                .ok()
        })
        .collect()
}
