//! The official assets, defined in code: their catalogue, skins,
//! emotes and meshes, baked into the files hamlet-server publishes,
//! with pictures of each worn by the realm's own robot or house.

pub mod agents;
pub mod catalog;
pub mod emotes;
pub mod glb;
pub mod hamlets;
pub mod previews;
pub mod skins;

use anyhow::{Context, Result};
use looks::wardrobe::{Category, EmoteFile, FORMAT, Target};

use crate::catalog::Entry;

/// The file of `entry`, as the server sends it to its owners.
pub fn file(entry: &Entry) -> Result<Vec<u8>> {
    let missing = || format!("`{}` is not defined", entry.id);
    Ok(match (entry.category, entry.target) {
        (Category::Skin, _) => serde_json::to_vec_pretty(
            &skins::of(entry.id).with_context(missing)?,
        )?,
        (Category::Emote, _) => {
            serde_json::to_vec_pretty(&EmoteFile {
                format: FORMAT,
                emote: emotes::of(entry.id).with_context(missing)?,
            })?
        }
        (Category::Mesh, Target::Agent) => {
            agents::of(entry.id).with_context(missing)?.glb()
        }
        (Category::Mesh, Target::Hamlet) => {
            hamlets::of(entry.id).with_context(missing)?.glb()
        }
    })
}

#[cfg(test)]
mod tests {
    use realm::bake::{Glb, MESH_PARTS, Shell};

    use super::*;
    use crate::catalog::ALL;

    #[test]
    fn every_mesh_reads_back_as_the_realm_wears_it() {
        for entry in
            ALL.iter().filter(|e| e.category == Category::Mesh)
        {
            let glb = Glb::read(&file(entry).unwrap())
                .unwrap_or_else(|why| panic!("{}: {why}", entry.id));
            match entry.target {
                Target::Agent => {
                    assert!(!glb.meshes.is_empty(), "{}", entry.id);
                    for (name, _) in &glb.meshes {
                        assert!(
                            MESH_PARTS.contains(&name.as_str()),
                            "{}: {name}",
                            entry.id
                        );
                    }
                }
                Target::Hamlet => {
                    let shell = Shell::of(&glb).expect(entry.id);
                    assert!(shell.panes.is_some(), "{}", entry.id);
                }
            }
        }
    }
}
