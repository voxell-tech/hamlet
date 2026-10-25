//! What agents and hamlets wear (docs/looks.md), read from
//! [`Looks`] for the world. A mesh's `.glb` reaches the asset server
//! from memory, through the `wardrobe://` source, and is never
//! written to disk.

mod glb;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::prelude::*;

pub use self::glb::Glb;
use crate::wardrobe::{Category, Emote, Skin, Target};

/// The asset source a worn mesh is loaded from.
pub const SOURCE: &str = "wardrobe";

pub fn plugin(app: &mut App) {
    app.init_resource::<Looks>()
        .init_resource::<Dressing>()
        .add_systems(
            PreUpdate,
            stock.run_if(resource_changed::<Looks>),
        );
}

/// What every agent and hamlet wears, and the files of what is worn.
#[derive(Resource, Clone, Debug, Default)]
pub struct Looks {
    /// By the agent's name.
    pub agents: BTreeMap<String, Worn>,
    /// By the hamlet's id.
    pub hamlets: BTreeMap<String, Worn>,
    /// The file of each asset worn, by its `id@version`: a skin's or
    /// an emote's JSON, or a mesh's `.glb`. The file of one is read
    /// once.
    pub files: HashMap<String, Vec<u8>>,
}

/// The asset one agent or hamlet wears of each kind, by its
/// `id@version`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Worn {
    pub skin: Option<String>,
    pub mesh: Option<String>,
    pub emote: Option<String>,
}

/// Has the robot of the agent of this name play the emote it wears,
/// now.
#[derive(Event, Clone, Debug)]
pub struct PlayEmote(pub String);

/// Makes the `wardrobe://` source the worn meshes are loaded from. It
/// must be called before the asset plugin is added; without it no
/// mesh is worn.
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

/// What every agent and hamlet of [`Looks`] wears, as the world
/// draws it.
#[derive(Resource, Default)]
pub struct Dressing {
    agents: HashMap<String, Look>,
    hamlets: HashMap<String, Look>,
    /// Each file worn, read, by what wears it, its kind and its id.
    read: HashMap<(Target, Category, String), Read>,
}

/// What one agent or hamlet wears, as the world draws it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub skin: Option<Skin>,
    pub emote: Option<Emote>,
    /// The glTF of the mesh worn, loading or loaded.
    pub mesh: Option<Handle<Gltf>>,
}

/// A file worn, as it was read.
enum Read {
    Skin(Skin),
    Emote(Emote),
    Mesh(Handle<Gltf>),
    Failed,
}

impl Dressing {
    /// What `name` of `target` wears, of what is in memory.
    pub fn look(&self, target: Target, name: &str) -> Option<&Look> {
        match target {
            Target::Agent => self.agents.get(name),
            Target::Hamlet => self.hamlets.get(name),
        }
    }

    /// The look of `worn`, of the files read.
    fn dress(&self, target: Target, worn: &Worn) -> Look {
        let read = |category, id: &Option<String>| {
            self.read.get(&(target, category, id.clone()?))
        };
        Look {
            skin: match read(Category::Skin, &worn.skin) {
                Some(Read::Skin(skin)) => Some(skin.clone()),
                _ => None,
            },
            emote: match read(Category::Emote, &worn.emote) {
                Some(Read::Emote(emote)) => Some(emote.clone()),
                _ => None,
            },
            mesh: match read(Category::Mesh, &worn.mesh) {
                Some(Read::Mesh(gltf)) => Some(gltf.clone()),
                _ => None,
            },
        }
    }
}

/// Reads the files of what is worn that are not read yet, hands each
/// mesh to the asset server, and dresses everyone again.
fn stock(
    looks: Res<Looks>,
    closet: Option<Res<Closet>>,
    server: Res<AssetServer>,
    mut dressing: ResMut<Dressing>,
) {
    let mut worn = HashSet::new();
    for (target, all) in [
        (Target::Agent, &looks.agents),
        (Target::Hamlet, &looks.hamlets),
    ] {
        for one in all.values() {
            for (category, id) in [
                (Category::Skin, &one.skin),
                (Category::Emote, &one.emote),
                (Category::Mesh, &one.mesh),
            ] {
                let Some(id) = id else {
                    continue;
                };
                let key = (target, category, id.clone());
                worn.insert(key.clone());
                if dressing.read.contains_key(&key) {
                    continue;
                }
                let Some(bytes) = looks.files.get(id) else {
                    continue;
                };
                let read =
                    read(&key, bytes, closet.as_deref(), &server);
                dressing.read.insert(key, read);
            }
        }
    }
    dressing.read.retain(|key, read| {
        let kept = worn.contains(key);
        if !kept
            && matches!(read, Read::Mesh(_))
            && let Some(closet) = &closet
        {
            closet.take_down(&key.2);
        }
        kept
    });
    let agents = looks
        .agents
        .iter()
        .map(|(name, worn)| {
            (name.clone(), dressing.dress(Target::Agent, worn))
        })
        .collect();
    let hamlets = looks
        .hamlets
        .iter()
        .map(|(name, worn)| {
            (name.clone(), dressing.dress(Target::Hamlet, worn))
        })
        .collect();
    dressing.agents = agents;
    dressing.hamlets = hamlets;
}

/// The file `bytes` of the asset `key` names, read.
fn read(
    (target, category, id): &(Target, Category, String),
    bytes: &[u8],
    closet: Option<&Closet>,
    server: &AssetServer,
) -> Read {
    try_on(*target, *category, id, bytes, closet, server)
        .map(|look| match look {
            Look {
                skin: Some(skin), ..
            } => Read::Skin(skin),
            Look {
                emote: Some(emote), ..
            } => Read::Emote(emote),
            Look {
                mesh: Some(gltf), ..
            } => Read::Mesh(gltf),
            _ => Read::Failed,
        })
        .unwrap_or_else(|e| {
            warn!("the look `{id}` is not worn: {e:#}");
            Read::Failed
        })
}

/// The look of the one file `bytes` of `category`, worn by `target`.
/// A mesh is put in the closet as `name` and loaded from there; take
/// it down with [`Closet::take_down`] once it is not wanted.
pub fn try_on(
    target: Target,
    category: Category,
    name: &str,
    bytes: &[u8],
    closet: Option<&Closet>,
    server: &AssetServer,
) -> anyhow::Result<Look> {
    Ok(match category {
        Category::Skin => Look {
            skin: Some(Skin::read(bytes, target)?),
            ..default()
        },
        Category::Emote => Look {
            emote: Some(Emote::read(bytes, target)?),
            ..default()
        },
        Category::Mesh => {
            let Some(closet) = closet else {
                anyhow::bail!(
                    "there is no `{SOURCE}://` source to load it \
                     from"
                );
            };
            let path = glb(name);
            closet.0.insert_asset(&path, bytes.to_vec());
            Look {
                mesh: Some(
                    server.load(format!(
                        "{SOURCE}://{}",
                        path.display()
                    )),
                ),
                ..default()
            }
        }
    })
}

impl Closet {
    /// Takes the mesh put in as `name` out of the closet.
    pub fn take_down(&self, name: &str) {
        self.0.remove_asset(&glb(name));
    }
}

/// The path of the mesh asset `id` in the closet.
fn glb(id: &str) -> PathBuf {
    PathBuf::from(format!("{id}.glb"))
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

#[cfg(test)]
mod tests {
    use bevy::render::RenderPlugin;
    use bevy::render::settings::WgpuSettings;
    use bevy::window::ExitCondition;
    use bevy::winit::WinitPlugin;

    use super::*;
    use crate::meshes::Meshing;

    #[test]
    fn a_mesh_written_and_worn_from_memory_loads_back_whole() {
        let mut meshing = Meshing::new();
        meshing.rbox(
            Vec3::Y,
            Vec3::splat(0.5),
            0.05,
            Quat::IDENTITY,
            Vec3::new(0.8, 0.2, 0.1),
            0.0,
        );
        let walls = meshing.build();
        let bytes = Glb::new()
            .mesh("walls", &walls, Transform::IDENTITY)
            .spot("door", Transform::from_xyz(0.0, 0.0, 1.0))
            .write();

        let mut app = App::new();
        register_source(&mut app);
        app.add_plugins((
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
            plugin,
        ));
        // As `App::run` does: the glTF loader is registered here.
        app.finish();
        app.cleanup();
        let mut looks = app.world_mut().resource_mut::<Looks>();
        looks.hamlets.insert(
            "mill".into(),
            Worn {
                mesh: Some("cottage".into()),
                ..default()
            },
        );
        looks.files.insert("cottage".into(), bytes);
        app.update();
        let gltf = app
            .world()
            .resource::<Dressing>()
            .look(Target::Hamlet, "mill")
            .and_then(|look| look.mesh.clone())
            .expect("the mesh, loading");
        for _ in 0..500 {
            app.update();
            let server = app.world().resource::<AssetServer>();
            if server.is_loaded_with_dependencies(&gltf) {
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
