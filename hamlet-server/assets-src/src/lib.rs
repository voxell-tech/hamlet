//! The official assets, defined in code: their catalogue, skins,
//! emotes and meshes, baked into the files hamlet-server publishes.

pub mod agents;
pub mod catalog;
pub mod emotes;
pub mod hamlets;
pub mod previews;
pub mod skins;

use anyhow::{Context, Result};
use hamlet_model::wardrobe::{Category, EmoteFile, FORMAT, Target};

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
    use hamlet_world::keeper::MESH_PARTS;

    use super::*;
    use crate::catalog::ALL;

    /// The names of the glTF meshes in a `.glb`.
    fn meshes(glb: &[u8]) -> Vec<String> {
        assert_eq!(&glb[..4], b"glTF");
        let json = u32::from_le_bytes(glb[12..16].try_into().unwrap())
            as usize;
        let doc = serde_json::from_slice::<serde_json::Value>(
            &glb[20..20 + json],
        )
        .unwrap();
        doc["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mesh| mesh["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn every_mesh_loads_through_bevys_gltf_loader_from_memory() {
        use std::path::Path;

        use bevy::gltf::{Gltf, GltfMesh};
        use bevy::prelude::*;
        use bevy::render::RenderPlugin;
        use bevy::render::settings::WgpuSettings;
        use bevy::window::ExitCondition;
        use bevy::winit::WinitPlugin;
        use hamlet_world::dress::{self, Closet};

        let mut app = App::new();
        dress::register_source(&mut app);
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<WinitPlugin>()
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        backends: None,
                        ..default()
                    }
                    .into(),
                    ..default()
                }),
        );
        app.finish();
        app.cleanup();
        let mut loads = Vec::new();
        for entry in
            ALL.iter().filter(|e| e.category == Category::Mesh)
        {
            let path = format!("{}.glb", entry.id);
            let bytes = file(entry).unwrap();
            app.world()
                .resource::<Closet>()
                .keep(Path::new(&path), bytes.clone());
            let gltf = app
                .world()
                .resource::<AssetServer>()
                .load::<Gltf>(format!("{}://{path}", dress::SOURCE));
            loads.push((entry.id, bytes, gltf));
        }
        for _ in 0..600 {
            app.update();
            let server = app.world().resource::<AssetServer>();
            if loads
                .iter()
                .all(|(_, _, gltf)| dress::loaded(gltf, server))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let world = app.world();
        let (gltfs, meshes_of) = (
            world.resource::<Assets<Gltf>>(),
            world.resource::<Assets<GltfMesh>>(),
        );
        for (id, bytes, gltf) in &loads {
            let server = world.resource::<AssetServer>();
            assert!(
                dress::loaded(gltf, server),
                "{id}: {:?}",
                server.get_load_states(gltf)
            );
            for name in meshes(bytes) {
                assert!(
                    dress::part(gltf, &name, gltfs, meshes_of)
                        .is_some(),
                    "{id}: {name}"
                );
            }
        }
    }

    #[test]
    fn every_asset_bakes_and_each_mesh_names_parts_the_client_finds()
    {
        for entry in ALL {
            let bytes = file(&entry).unwrap();
            assert!(!bytes.is_empty(), "{}", entry.id);
            if entry.category != Category::Mesh {
                continue;
            }
            let names = meshes(&bytes);
            match entry.target {
                Target::Agent => {
                    for name in &names {
                        assert!(
                            MESH_PARTS.contains(&name.as_str()),
                            "{}: {name}",
                            entry.id
                        );
                    }
                }
                Target::Hamlet => {
                    assert!(names.contains(&"walls".to_string()))
                }
            }
        }
    }
}
