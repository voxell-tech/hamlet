//! What agents and hamlets wear: the official assets the core keeps
//! in memory, made ready for the world. A mesh's `.glb` is given to
//! the asset server from memory, through the `wardrobe://` source,
//! and is never written to disk.

mod glb;

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::prelude::*;
use hamlet_core::wardrobe::{File, Wardrobe};
use hamlet_model::wardrobe::{Category, Emote, Skin, Target};

pub use self::glb::Glb;

/// The asset source a worn mesh is loaded from.
pub const SOURCE: &str = "wardrobe";

pub fn plugin(app: &mut App) {
    app.init_resource::<Dressing>().add_systems(
        Update,
        stock.run_if(|wardrobe: Option<Res<Wardrobe>>| {
            wardrobe.is_some_and(|wardrobe| wardrobe.is_changed())
        }),
    );
}

/// Makes the `wardrobe://` source the worn meshes are loaded from. It
/// must be called before the asset plugin is added, that is, before
/// `DefaultPlugins`; without it no mesh is worn.
pub fn register_source(app: &mut App) {
    let dir = Dir::default();
    let root = dir.clone();
    app.register_asset_source(
        AssetSourceId::from(SOURCE),
        AssetSourceBuilder::new(move || {
            Box::new(MemoryAssetReader { root: root.clone() })
        }),
    )
    .insert_resource(Closet(dir));
}

/// The files the `wardrobe://` source serves, in memory.
#[derive(Resource)]
pub struct Closet(Dir);

/// The meshes of the worn assets, as the asset server loads them, by
/// asset id; and a count that moves each time what is worn may have
/// changed.
#[derive(Resource, Default)]
pub struct Dressing {
    meshes: HashMap<String, Handle<Gltf>>,
    /// Moves when anything worn, or its file, changes.
    pub version: u64,
}

/// What one agent or hamlet wears, as the world draws it.
#[derive(Clone, Debug, Default)]
pub struct Look<'a> {
    pub skin: Option<&'a Skin>,
    pub emote: Option<&'a Emote>,
    /// The mesh worn, by its asset id, and its glTF once it is
    /// loaded.
    pub mesh: Option<(&'a str, Handle<Gltf>)>,
}

impl Dressing {
    /// What `name` of `target` wears, of what is downloaded.
    pub fn look<'a>(
        &self,
        wardrobe: &'a Wardrobe,
        target: Target,
        name: &str,
    ) -> Look<'a> {
        let skin = match wardrobe.worn(target, name, Category::Skin) {
            Some((_, File::Skin(skin))) => Some(skin),
            _ => None,
        };
        let emote = match wardrobe.worn(target, name, Category::Emote)
        {
            Some((_, File::Emote(emote))) => Some(emote),
            _ => None,
        };
        let mesh = match wardrobe.worn(target, name, Category::Mesh) {
            Some((id, File::Mesh(_))) => {
                self.meshes.get(id).map(|gltf| (id, gltf.clone()))
            }
            _ => None,
        };
        Look { skin, emote, mesh }
    }

    /// The glTF of the mesh asset `id`, if it is worn and given to
    /// the asset server.
    pub fn gltf(&self, id: &str) -> Option<&Handle<Gltf>> {
        self.meshes.get(id)
    }
}

/// Hands every mesh downloaded to the asset server, and moves the
/// count of what is worn.
fn stock(
    wardrobe: Res<Wardrobe>,
    closet: Option<Res<Closet>>,
    server: Res<AssetServer>,
    mut dressing: ResMut<Dressing>,
) {
    dressing.version += 1;
    let Some(closet) = closet else {
        return;
    };
    for (id, file) in &wardrobe.files {
        let File::Mesh(bytes) = file else {
            continue;
        };
        if dressing.meshes.contains_key(id) {
            continue;
        }
        let path = format!("{id}.glb");
        closet.0.insert_asset(Path::new(&path), bytes.clone());
        let gltf = server.load::<Gltf>(format!("{SOURCE}://{path}"));
        dressing.meshes.insert(id.clone(), gltf);
    }
}

/// A skin's colour for `slot`, as the world's meshes are coloured:
/// linear RGB.
pub fn colour(skin: &Skin, slot: &str) -> Option<Vec3> {
    skin.colour(slot).map(|[r, g, b]| {
        LinearRgba::from(Srgba::rgb(r, g, b)).to_vec3()
    })
}

/// The mesh named `name` in `gltf`, its first primitive, once loaded.
pub fn part(
    gltf: &Handle<Gltf>,
    name: &str,
    gltfs: &Assets<Gltf>,
    meshes: &Assets<GltfMesh>,
) -> Option<Handle<Mesh>> {
    let gltf = gltfs.get(gltf)?;
    let mesh = meshes.get(gltf.named_meshes.get(name)?)?;
    mesh.primitives
        .first()
        .map(|primitive| primitive.mesh.clone())
}

/// Where the node named `name` in `gltf` stands, once loaded.
pub fn anchor(
    gltf: &Handle<Gltf>,
    name: &str,
    gltfs: &Assets<Gltf>,
    nodes: &Assets<GltfNode>,
) -> Option<Transform> {
    let gltf = gltfs.get(gltf)?;
    nodes
        .get(gltf.named_nodes.get(name)?)
        .map(|node| node.transform)
}

/// True once the glTF and all it holds are loaded.
pub fn loaded(gltf: &Handle<Gltf>, server: &AssetServer) -> bool {
    server.is_loaded_with_dependencies(gltf)
}

#[cfg(test)]
mod tests {
    use bevy::render::RenderPlugin;
    use bevy::render::settings::WgpuSettings;
    use bevy::window::ExitCondition;
    use bevy::winit::WinitPlugin;

    use super::*;
    use crate::meshes::Mb;

    #[test]
    fn a_mesh_written_and_kept_in_memory_loads_back_whole() {
        let mut mb = Mb::new();
        mb.rbox(
            Vec3::Y,
            Vec3::splat(0.5),
            0.05,
            Quat::IDENTITY,
            Vec3::new(0.8, 0.2, 0.1),
            0.0,
        );
        let walls = mb.build();
        let bytes = Glb::new()
            .mesh("walls", &walls, Transform::IDENTITY)
            .spot("door", Transform::from_xyz(0.0, 0.0, 1.0))
            .write();

        let mut app = App::new();
        register_source(&mut app);
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
        // As `App::run` does: the glTF loader is registered here.
        app.finish();
        app.cleanup();
        app.world()
            .resource::<Closet>()
            .0
            .insert_asset(Path::new("house.glb"), bytes);
        let gltf = app
            .world()
            .resource::<AssetServer>()
            .load::<Gltf>(format!("{SOURCE}://house.glb"));
        for _ in 0..500 {
            app.update();
            if loaded(&gltf, app.world().resource::<AssetServer>()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let world = app.world();
        let (gltfs, meshes, nodes) = (
            world.resource::<Assets<Gltf>>(),
            world.resource::<Assets<GltfMesh>>(),
            world.resource::<Assets<GltfNode>>(),
        );
        let mesh =
            part(&gltf, "walls", gltfs, meshes).expect("the walls");
        let mesh =
            world.resource::<Assets<Mesh>>().get(&mesh).unwrap();
        assert_eq!(mesh.count_vertices(), walls.count_vertices());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
        let door =
            anchor(&gltf, "door", gltfs, nodes).expect("the door");
        assert_eq!(door.translation, Vec3::new(0.0, 0.0, 1.0));
    }
}
