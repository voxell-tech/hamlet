//! Each owner's scene: an entity of its own at its root, holding the
//! Bevy entity, mesh and material of each number the owner's changes
//! name.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Instant;

use bevy::app::{App, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::visibility::{RenderLayers, Visibility};
use bevy::camera::{
    Camera, Camera3d, Hdr, ImageRenderTarget, RenderTarget,
};
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::lifecycle::Despawn;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::{EntityWorldMut, World};
use bevy::image::Image;
use bevy::light::{
    DirectionalLight, NotShadowCaster, NotShadowReceiver, PointLight,
    SpotLight,
};
use bevy::mesh::{Mesh, Mesh3d, MeshTag};
use bevy::pbr::decal::{ForwardDecal, ForwardDecalMaterialExt};
use bevy::pbr::{
    DistanceFog, ExtendedMaterial, MeshMaterial3d, StandardMaterial,
};
use bevy::post_process::bloom::Bloom;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::{Shader, ValidateShader};
use bevy::transform::components::Transform;
use bevy::window::WindowRef;
use hamlet_engine_core::LogLine;
use hamlet_engine_shell::Backdrop;

use crate::change::{CameraSetup, SceneChange, SceneLight};
use crate::compute::ComputeJob;
use crate::decal::{DecalMaterial, DrawsDecals};
use crate::guard::{GpuInfo, TimedSlots};
use crate::label::WorldLabel;
use crate::material::{self, ShadedMaterial};
use crate::pass::ScenePass;
use crate::pick::Watched;
use crate::shader::CheckedShader;
use crate::viewport::{RenderTuning, ViewPointer};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Backdrop>()
        .init_resource::<SceneLayers>()
        .add_observer(give_layer)
        .add_systems(Update, release_backdrop);
}

/// On the root of a scene: its owner, and the entities and assets of
/// the numbers its changes name.
#[derive(Component)]
#[require(Transform, Visibility)]
pub struct Scene {
    pub owner: Entity,
    /// When the owner's host began numbering; a scene of another
    /// numbering is taken away first.
    pub since: Instant,
    entities: HashMap<u32, Entity>,
    meshes: HashMap<u32, Handle<Mesh>>,
    materials: HashMap<u32, Look>,
    instances: HashMap<u32, Copies>,
    cameras: HashMap<u32, Entity>,
    shaders: HashMap<u32, Handle<Shader>>,
    /// Each buffer, and its bytes as the owner last wrote them.
    buffers: HashMap<u32, (Handle<ShaderBuffer>, Vec<u8>)>,
    textures: HashMap<u32, Handle<Image>>,
    computes: HashMap<u32, Entity>,
    /// The cameras with a pass.
    passes: HashSet<u32>,
    /// The entities that lay a decal.
    decals: HashSet<u32>,
    /// The buffers written in the changes being made.
    written: HashSet<u32>,
    /// The render layer of its entities, seen by its cameras alone.
    layer: usize,
}

/// The render layers of scenes, each held by one scene.
#[derive(Resource, Default, Debug)]
struct SceneLayers {
    free: Vec<usize>,
    /// The last layer handed out; the default layer 0 is never one.
    last: usize,
}

impl SceneLayers {
    fn take(&mut self) -> usize {
        self.free.pop().unwrap_or_else(|| {
            self.last += 1;
            self.last
        })
    }
}

fn give_layer(
    gone: On<Despawn, Scene>,
    scenes: Query<&Scene>,
    mut layers: ResMut<SceneLayers>,
) {
    if let Ok(scene) = scenes.get(gone.entity) {
        layers.free.push(scene.layer);
    }
}

/// A material of a scene: Bevy's own, one of the owner's shader, or a
/// decal's.
#[derive(Clone, Debug)]
enum Look {
    Standard(Handle<StandardMaterial>),
    Shaded(Handle<ShadedMaterial>),
    Decal(Handle<DecalMaterial>),
}

/// What an entity draws with: a mesh and its material, or a decal.
type Drawn = (
    Mesh3d,
    MeshMaterial3d<StandardMaterial>,
    MeshMaterial3d<ShadedMaterial>,
    MeshMaterial3d<DecalMaterial>,
    ForwardDecal,
);

/// A buffer bound where an owner gave none.
#[derive(Resource, Clone, Debug)]
struct SpareBuffer(Handle<ShaderBuffer>);

/// The copies of instances, under their own entity, and what they
/// draw, kept after the owner drops its numbers.
struct Copies {
    root: Entity,
    key: u32,
    drawn: Option<(Handle<Mesh>, Look)>,
    copies: Vec<Entity>,
    watched: Option<Watched>,
}

/// On the root of a scene: its owner.
#[derive(Component, Clone, Copy, Debug)]
#[relationship(relationship_target = Scenes)]
pub struct SceneOf(pub Entity);

/// On an owner: the root of its scene.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = SceneOf, linked_spawn)]
pub struct Scenes(Vec<Entity>);

impl Scenes {
    pub fn root(&self) -> Option<Entity> {
        self.0.first().copied()
    }
}

/// On each entity of a scene: its owner and the owner's key, with the
/// copy for instances.
#[derive(Component, Clone, Copy, Debug)]
pub struct ScenePart {
    pub owner: Entity,
    pub key: u32,
    pub index: u32,
}

/// On a scene's camera: its owner, the key of the entity it is at,
/// and the image it draws into unless it draws behind the dock.
#[derive(Component, Clone, Debug)]
#[require(ViewPointer)]
pub struct SceneCamera {
    pub owner: Entity,
    pub key: u32,
    pub image: Option<Handle<Image>>,
}

impl Scene {
    fn new(owner: Entity, since: Instant, layer: usize) -> Self {
        Self {
            owner,
            since,
            layer,
            entities: HashMap::new(),
            meshes: HashMap::new(),
            materials: HashMap::new(),
            instances: HashMap::new(),
            cameras: HashMap::new(),
            shaders: HashMap::new(),
            buffers: HashMap::new(),
            textures: HashMap::new(),
            computes: HashMap::new(),
            passes: HashSet::new(),
            decals: HashSet::new(),
            written: HashSet::new(),
        }
    }

    /// Gives each buffer written since the last upload its bytes,
    /// once however many writes it had.
    fn upload(&mut self, world: &mut World) {
        if self.written.is_empty() {
            return;
        }
        let mut assets = world.resource_mut::<Assets<ShaderBuffer>>();
        for id in self.written.drain() {
            if let Some((held, kept)) = self.buffers.get(&id)
                && let Some(mut buffer) = assets.get_mut(held.id())
            {
                buffer.data = Some(kept.clone());
            }
        }
    }

    /// Whether the owner has GPU work of its own: a shaded material,
    /// compute or a pass.
    pub fn gpu_work(&self) -> bool {
        !self.computes.is_empty()
            || !self.passes.is_empty()
            || self
                .materials
                .values()
                .any(|look| matches!(look, Look::Shaded(_)))
    }

    /// Entities, each copy of instances counted.
    pub fn entities(&self) -> usize {
        self.entities.len()
            + self
                .instances
                .values()
                .map(|copies| copies.copies.len())
                .sum::<usize>()
    }

    pub fn meshes(&self) -> usize {
        self.meshes.len()
    }

    pub fn materials(&self) -> usize {
        self.materials.len()
    }

    pub fn cameras(&self) -> usize {
        self.cameras.len()
    }

    /// Entities that lay a decal.
    pub fn decals(&self) -> usize {
        self.decals.len()
    }

    /// The camera of the number `id`.
    pub fn camera(&self, id: u32) -> Option<Entity> {
        self.cameras.get(&id).copied()
    }

    fn entity(&self, id: u32) -> Option<Entity> {
        self.entities.get(&id).copied()
    }

    /// The entity of `parent`, or the scene's `root`.
    fn under(&self, parent: Option<u32>, root: Entity) -> Entity {
        parent
            .and_then(|parent| self.entity(parent))
            .unwrap_or(root)
    }

    /// Makes `change` in `world`, under the scene's `root`.
    fn apply(
        &mut self,
        world: &mut World,
        root: Entity,
        change: SceneChange,
    ) {
        match change {
            SceneChange::Entity { id, key, parent } => {
                let under = self.under(parent, root);
                let made = world
                    .spawn((
                        Transform::default(),
                        Visibility::default(),
                        self.part(key, 0),
                        self.layers(),
                        ChildOf(under),
                    ))
                    .id();
                self.entities.insert(id, made);
            }
            SceneChange::Transform { id, transform } => {
                self.insert(world, id, transform);
            }
            SceneChange::Visible { id, visible } => {
                let shown = if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                self.insert(world, id, shown);
            }
            SceneChange::Parent { id, parent } => {
                let Some(entity) = self.entity(id) else {
                    return;
                };
                let under = self.under(parent, root);
                // Under itself, it would leave the scene's tree and
                // outlive the scene.
                let mut at = Some(under);
                while let Some(above) = at {
                    if above == entity {
                        return;
                    }
                    at = world
                        .get::<ChildOf>(above)
                        .map(ChildOf::parent);
                }
                self.insert(world, id, ChildOf(under));
            }
            SceneChange::Draw { id, mesh, material } => {
                if let (Some(mesh), Some(look)) = (
                    self.meshes.get(&mesh).cloned(),
                    self.materials.get(&material).cloned(),
                ) && let Some(entity) = self.entity(id)
                    && let Ok(mut entity) =
                        world.get_entity_mut(entity)
                {
                    entity.draw(mesh, look);
                    self.decals.remove(&id);
                }
            }
            SceneChange::DrawDecal { id, material } => {
                let drawn = world.contains_resource::<DrawsDecals>();
                if let Some(Look::Decal(material)) =
                    self.materials.get(&material).cloned()
                    && let Some(entity) = self.entity(id)
                    && let Ok(mut entity) =
                        world.get_entity_mut(entity)
                {
                    entity.draw_decal(material, drawn);
                    self.decals.insert(id);
                    self.prepass(world);
                }
            }
            SceneChange::Undraw(id) => {
                self.remove::<Drawn>(world, id);
                self.decals.remove(&id);
            }
            SceneChange::Light { id, light } => {
                self.remove::<(DirectionalLight, PointLight, SpotLight)>(
                    world, id,
                );
                match light {
                    Some(SceneLight::Directional(light)) => {
                        self.insert(world, id, light);
                    }
                    Some(SceneLight::Point(light)) => {
                        self.insert(world, id, light);
                    }
                    Some(SceneLight::Spot(light)) => {
                        self.insert(world, id, light);
                    }
                    None => {}
                }
            }
            SceneChange::Shadows { id, cast, receive } => {
                self.remove::<(NotShadowCaster, NotShadowReceiver)>(
                    world, id,
                );
                if !cast {
                    self.insert(world, id, NotShadowCaster);
                }
                if !receive {
                    self.insert(world, id, NotShadowReceiver);
                }
            }
            SceneChange::Label { id, text } => match text {
                Some(text) => {
                    self.insert(world, id, WorldLabel(text))
                }
                None => self.remove::<WorldLabel>(world, id),
            },
            SceneChange::Watch { id, hover, click } => {
                let watched = Watched { hover, click };
                if let Some(copies) = self.instances.get_mut(&id) {
                    copies.watched = Some(watched);
                    for &copy in &copies.copies {
                        if let Ok(mut copy) =
                            world.get_entity_mut(copy)
                        {
                            copy.insert(watched);
                        }
                    }
                } else {
                    self.insert(world, id, watched);
                }
            }
            SceneChange::Mesh { id, mesh } => {
                let mut meshes = world.resource_mut::<Assets<Mesh>>();
                match self.meshes.get(&id) {
                    Some(held) => {
                        let _ = meshes.insert(held.id(), mesh);
                    }
                    None => {
                        self.meshes.insert(id, meshes.add(mesh));
                    }
                }
            }
            SceneChange::Material { id, material } => {
                let mut materials =
                    world.resource_mut::<Assets<StandardMaterial>>();
                match self.materials.get(&id) {
                    Some(Look::Standard(held)) => {
                        let _ = materials.insert(held.id(), material);
                    }
                    _ => {
                        let made = materials.add(material);
                        self.materials
                            .insert(id, Look::Standard(made));
                    }
                }
            }
            SceneChange::Decal {
                id,
                mut material,
                depth_fade,
                texture,
            } => {
                let extension = ForwardDecalMaterialExt {
                    depth_fade_factor: depth_fade,
                };
                let mut materials =
                    world.resource_mut::<Assets<DecalMaterial>>();
                match self.materials.get(&id) {
                    Some(Look::Decal(held)) => {
                        if let Some(mut decal) =
                            materials.get_mut(held.id())
                        {
                            material.base_color_texture =
                                decal.base.base_color_texture.take();
                            decal.base = material;
                            decal.extension = extension;
                        }
                    }
                    _ => {
                        material.base_color_texture = texture
                            .and_then(|texture| {
                                self.textures.get(&texture)
                            })
                            .cloned();
                        let made = materials.add(ExtendedMaterial {
                            base: material,
                            extension,
                        });
                        self.materials.insert(id, Look::Decal(made));
                    }
                }
            }
            SceneChange::Shader { id, shader } => {
                let made = world
                    .resource_mut::<Assets<Shader>>()
                    .add(checked(shader));
                self.shaders.insert(id, made);
            }
            SceneChange::Buffer { id, size } => {
                let bytes = vec![0; size as usize];
                let made = world
                    .resource_mut::<Assets<ShaderBuffer>>()
                    .add(ShaderBuffer::new(
                        &bytes,
                        RenderAssetUsages::default(),
                    ));
                self.buffers.insert(id, (made, bytes));
            }
            SceneChange::Write { id, offset, bytes } => {
                let Some((_, kept)) = self.buffers.get_mut(&id)
                else {
                    return;
                };
                let at = offset as usize;
                let Some(into) = kept.get_mut(at..at + bytes.len())
                else {
                    return;
                };
                into.copy_from_slice(&bytes);
                self.written.insert(id);
            }
            SceneChange::Texture {
                id,
                width,
                height,
                pixels,
            } => {
                let image = Image::new(
                    Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    pixels,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                );
                let mut images =
                    world.resource_mut::<Assets<Image>>();
                match self.textures.get(&id) {
                    Some(held) => {
                        let _ = images.insert(held.id(), image);
                    }
                    None => {
                        self.textures.insert(id, images.add(image));
                    }
                }
            }
            SceneChange::Shaded {
                id,
                shader,
                alpha,
                double_sided,
                lit,
                params,
                texture,
                data,
            } => {
                let Some(shader) = self.shaders.get(&shader).cloned()
                else {
                    return;
                };
                let data = match data
                    .and_then(|data| self.buffers.get(&data))
                {
                    Some((held, _)) => held.clone(),
                    None => world.spare_buffer(),
                };
                let material = ShadedMaterial {
                    params: material::params(&params),
                    picture: texture
                        .and_then(|texture| {
                            self.textures.get(&texture)
                        })
                        .cloned(),
                    data,
                    shader,
                    alpha,
                    double_sided,
                    lit,
                };
                let made = world
                    .resource_mut::<Assets<ShadedMaterial>>()
                    .add(material);
                self.materials.insert(id, Look::Shaded(made));
            }
            SceneChange::Params { id, params } => {
                if let Some(Look::Shaded(held)) =
                    self.materials.get(&id)
                {
                    if let Some(mut material) = world
                        .resource_mut::<Assets<ShadedMaterial>>()
                        .get_mut(held.id())
                    {
                        material.params = material::params(&params);
                    }
                } else if let Some(&job) = self.computes.get(&id)
                    && let Some(mut job) =
                        world.get_mut::<ComputeJob>(job)
                {
                    job.params = material::params(&params);
                }
            }
            SceneChange::Compute {
                id,
                shader,
                data,
                extra,
            } => {
                let shader = self.shaders.get(&shader).cloned();
                let data = self
                    .buffers
                    .get(&data)
                    .map(|(held, _)| held.clone());
                let (Some(shader), Some(data)) = (shader, data)
                else {
                    return;
                };
                let extra = extra
                    .and_then(|extra| self.buffers.get(&extra))
                    .map(|(held, _)| held.clone());
                let slot = world.timed_slot(self.owner);
                let job = world
                    .spawn((
                        ComputeJob {
                            owner: self.owner,
                            shader,
                            data,
                            extra,
                            params: material::params(&[]),
                            groups: [0; 3],
                            every_frame: false,
                            asked: 0,
                            slot,
                        },
                        ChildOf(root),
                    ))
                    .id();
                self.computes.insert(id, job);
            }
            SceneChange::Dispatch {
                id,
                groups,
                every_frame,
            } => {
                if let Some(&job) = self.computes.get(&id)
                    && let Some(mut job) =
                        world.get_mut::<ComputeJob>(job)
                {
                    job.groups = groups;
                    job.every_frame = every_frame;
                    job.asked += 1;
                }
            }
            SceneChange::Pass {
                camera,
                shader,
                params,
            } => {
                let Some(entity) =
                    self.camera(camera).filter(|&entity| {
                        world.get_entity(entity).is_ok()
                    })
                else {
                    return;
                };
                let shader = shader.and_then(|shader| {
                    self.shaders.get(&shader).cloned()
                });
                let Some(shader) = shader else {
                    self.passes.remove(&camera);
                    if let Ok(mut entity) =
                        world.get_entity_mut(entity)
                    {
                        entity.remove::<ScenePass>();
                    }
                    return;
                };
                let slot = match world.get::<ScenePass>(entity) {
                    Some(pass) => pass.slot,
                    None => world.timed_slot(self.owner),
                };
                self.passes.insert(camera);
                if let Ok(mut entity) = world.get_entity_mut(entity) {
                    entity.insert(ScenePass {
                        owner: self.owner,
                        shader,
                        params: material::params(&params),
                        slot,
                    });
                }
            }
            SceneChange::Instances {
                id,
                key,
                parent,
                mesh,
                material,
            } => {
                let under = self.under(parent, root);
                let made = world
                    .spawn((
                        Transform::default(),
                        Visibility::default(),
                        self.part(key, 0),
                        ChildOf(under),
                    ))
                    .id();
                let drawn = self
                    .meshes
                    .get(&mesh)
                    .cloned()
                    .zip(self.materials.get(&material).cloned());
                self.instances.insert(
                    id,
                    Copies {
                        root: made,
                        key,
                        drawn,
                        copies: Vec::new(),
                        watched: None,
                    },
                );
            }
            SceneChange::Copies { id, transforms } => {
                self.copy(world, id, transforms);
            }
            SceneChange::Camera {
                id,
                at,
                setup,
                background,
            } => {
                let Some(at) = self.entity(at) else {
                    return;
                };
                let key = world
                    .get::<ScenePart>(at)
                    .map_or(0, |part| part.key);
                let camera = world
                    .spawn_camera(self.owner, key, at, background);
                world.entity_mut(camera).insert(self.layers());
                world.set_up(camera, setup);
                self.cameras.insert(id, camera);
                self.prepass(world);
            }
            SceneChange::Setup { id, setup } => {
                if let Some(camera) = self.camera(id) {
                    world.set_up(camera, setup);
                }
            }
            SceneChange::Drop(id) => self.drop(world, id),
        }
    }

    fn layers(&self) -> RenderLayers {
        RenderLayers::layer(self.layer)
    }

    /// Gives each camera the depth prepass a decal reads, once the
    /// scene has one.
    fn prepass(&self, world: &mut World) {
        if self.decals.is_empty() {
            return;
        }
        for &camera in self.cameras.values() {
            if let Ok(mut camera) = world.get_entity_mut(camera)
                && !camera.contains::<DepthPrepass>()
            {
                camera.insert(DepthPrepass);
            }
        }
    }

    fn part(&self, key: u32, index: u32) -> ScenePart {
        ScenePart {
            owner: self.owner,
            key,
            index,
        }
    }

    /// Inserts `bundle` on the entity `id`, if it is still there.
    fn insert(
        &self,
        world: &mut World,
        id: u32,
        bundle: impl bevy::ecs::bundle::Bundle,
    ) {
        if let Some(entity) = self.entity(id)
            && let Ok(mut entity) = world.get_entity_mut(entity)
        {
            entity.insert(bundle);
        }
    }

    fn remove<B: bevy::ecs::bundle::Bundle>(
        &self,
        world: &mut World,
        id: u32,
    ) {
        if let Some(entity) = self.entity(id)
            && let Ok(mut entity) = world.get_entity_mut(entity)
        {
            entity.remove::<B>();
        }
    }

    /// Gives the instances `id` a copy for each of `transforms`.
    fn copy(
        &mut self,
        world: &mut World,
        id: u32,
        transforms: Vec<Transform>,
    ) {
        let owner = self.owner;
        let layers = self.layers();
        let Some(copies) = self.instances.get_mut(&id) else {
            return;
        };
        for gone in copies
            .copies
            .drain(transforms.len().min(copies.copies.len())..)
        {
            if let Ok(gone) = world.get_entity_mut(gone) {
                gone.despawn();
            }
        }
        for (at, transform) in transforms.into_iter().enumerate() {
            if let Some(&copy) = copies.copies.get(at) {
                if let Ok(mut copy) = world.get_entity_mut(copy) {
                    copy.insert(transform);
                }
                continue;
            }
            let mut made = world.spawn((
                transform,
                Visibility::default(),
                MeshTag(at as u32),
                ScenePart {
                    owner,
                    key: copies.key,
                    index: at as u32,
                },
                layers.clone(),
                ChildOf(copies.root),
            ));
            if let Some((mesh, look)) = &copies.drawn {
                made.draw(mesh.clone(), look.clone());
            }
            if let Some(watched) = copies.watched {
                made.insert(watched);
            }
            copies.copies.push(made.id());
        }
    }

    /// Takes away whatever has the number `id`.
    fn drop(&mut self, world: &mut World, id: u32) {
        let gone = if let Some(entity) = self.entities.remove(&id) {
            self.decals.remove(&id);
            Some(entity)
        } else if let Some(copies) = self.instances.remove(&id) {
            Some(copies.root)
        } else if let Some(camera) = self.cameras.remove(&id) {
            self.passes.remove(&id);
            Some(camera)
        } else if let Some(job) = self.computes.remove(&id) {
            Some(job)
        } else {
            self.meshes.remove(&id);
            self.materials.remove(&id);
            self.shaders.remove(&id);
            self.buffers.remove(&id);
            self.textures.remove(&id);
            None
        };
        let Some(gone) =
            gone.filter(|&gone| world.get_entity(gone).is_ok())
        else {
            return;
        };
        let under = if world.get::<Children>(gone).is_some() {
            world
                .query::<&Children>()
                .query(world)
                .iter_descendants(gone)
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        world.entity_mut(gone).despawn();
        if under.is_empty() {
            return;
        }
        // What was under it went with it, so its numbers name
        // nothing.
        self.entities.retain(|_, entity| !under.contains(entity));
        let entities = &self.entities;
        self.decals.retain(|id| entities.contains_key(id));
        self.instances
            .retain(|_, copies| !under.contains(&copies.root));
        self.cameras.retain(|_, camera| !under.contains(camera));
        let cameras = &self.cameras;
        self.passes.retain(|camera| cameras.contains_key(camera));
    }
}

/// The render crate's methods on [`World`].
pub trait SceneWorldExt {
    /// Makes `changes` to the scene of `owner`, whose host began
    /// numbering at `since`.
    fn apply_scene(
        &mut self,
        owner: Entity,
        since: Instant,
        changes: Vec<SceneChange>,
    );

    /// Takes the scene of `owner` away.
    fn clear_scene(&mut self, owner: Entity);

    /// The scene of `owner`, if it has one.
    fn scene_of(&self, owner: Entity) -> Option<&Scene>;

    /// A camera of `owner` at `at`, of the key `key`, behind the dock
    /// when `background`.
    fn spawn_camera(
        &mut self,
        owner: Entity,
        key: u32,
        at: Entity,
        background: bool,
    ) -> Entity;

    /// Gives `camera` the components `setup` says.
    fn set_up(&mut self, camera: Entity, setup: CameraSetup);

    /// A zeroed buffer bound where an owner gave none.
    fn spare_buffer(&mut self) -> Handle<ShaderBuffer>;

    /// A slot of the clock for work of `owner`, if the GPU has
    /// timestamps and a slot is left.
    fn timed_slot(&mut self, owner: Entity) -> Option<u32>;
}

impl SceneWorldExt for World {
    fn apply_scene(
        &mut self,
        owner: Entity,
        since: Instant,
        changes: Vec<SceneChange>,
    ) {
        if self.get_entity(owner).is_err() {
            return;
        }
        let held = self
            .get::<Scenes>(owner)
            .and_then(Scenes::root)
            .filter(|&root| {
                self.get::<Scene>(root)
                    .is_some_and(|scene| scene.since == since)
            });
        let root = match held {
            Some(root) => root,
            None => {
                self.clear_scene(owner);
                let layer =
                    self.get_resource_or_init::<SceneLayers>().take();
                self.spawn((
                    Scene::new(owner, since, layer),
                    SceneOf(owner),
                ))
                .id()
            }
        };
        let Some(mut scene) = self.entity_mut(root).take::<Scene>()
        else {
            return;
        };
        for change in changes {
            scene.apply(self, root, change);
        }
        scene.upload(self);
        self.entity_mut(root).insert(scene);
    }

    fn clear_scene(&mut self, owner: Entity) {
        let Some(root) =
            self.get::<Scenes>(owner).and_then(Scenes::root)
        else {
            return;
        };
        self.entity_mut(root).despawn();
    }

    fn scene_of(&self, owner: Entity) -> Option<&Scene> {
        let root = self.get::<Scenes>(owner)?.root()?;
        self.get::<Scene>(root)
    }

    fn spawn_camera(
        &mut self,
        owner: Entity,
        key: u32,
        at: Entity,
        background: bool,
    ) -> Entity {
        let (target, image, order, active) = if background {
            let mut backdrop = self.resource_mut::<Backdrop>();
            let free = backdrop.0.is_none_or(|held| held == owner);
            if free {
                backdrop.0 = Some(owner);
            } else {
                self.write_message(LogLine::warn(
                    "render",
                    "another plugin draws behind the dock, so this \
                     camera does not",
                ));
            }
            (RenderTarget::Window(WindowRef::Primary), None, -1, free)
        } else {
            let image = self
                .resource_mut::<Assets<Image>>()
                .add(target_image(1, 1));
            let target = RenderTarget::Image(ImageRenderTarget {
                handle: image.clone(),
                scale_factor: 1.0,
            });
            // Drawn once a viewport shows it, and before the
            // interface, which shows its image in the same frame;
            // drawn after, a new or resized image would show empty
            // for a frame.
            (target, Some(image), -1, false)
        };
        let mut camera = self.spawn((
            Camera3d::default(),
            Camera {
                order,
                is_active: active,
                ..Camera::default()
            },
            target,
            SceneCamera { owner, key, image },
            ScenePart {
                owner,
                key,
                index: 0,
            },
            Transform::default(),
            ChildOf(at),
        ));
        if background {
            camera.insert(Hdr);
        }
        camera.id()
    }

    fn set_up(&mut self, camera: Entity, setup: CameraSetup) {
        // Samples the GPU has not would fail the device.
        let msaa = self
            .get_resource::<GpuInfo>()
            .map_or(setup.msaa, |gpu| gpu.msaa(setup.msaa));
        let Ok(mut camera) = self.get_entity_mut(camera) else {
            return;
        };
        if let Some(mut drawn) = camera.get_mut::<Camera>() {
            drawn.clear_color = setup.clear;
        }
        // A camera behind the dock always draws in HDR, so its image
        // is never the interface's, which is cleared to nothing each
        // frame and blended over it.
        let background = camera
            .get::<SceneCamera>()
            .is_some_and(|camera| camera.image.is_none());
        camera.insert((
            setup.projection,
            setup.tonemapping,
            setup.exposure,
            setup.ambient,
            msaa,
        ));
        if setup.hdr || background {
            camera.insert(Hdr);
        } else {
            camera.remove::<Hdr>();
        }
        match setup.fog {
            Some(fog) => camera.insert(fog),
            None => camera.remove::<DistanceFog>(),
        };
        match setup.bloom {
            Some(bloom) => camera.insert(bloom),
            None => camera.remove::<Bloom>(),
        };
    }

    fn spare_buffer(&mut self) -> Handle<ShaderBuffer> {
        if let Some(spare) = self.get_resource::<SpareBuffer>() {
            return spare.0.clone();
        }
        let spare = self.resource_mut::<Assets<ShaderBuffer>>().add(
            ShaderBuffer::new(&[0; 16], RenderAssetUsages::default()),
        );
        self.insert_resource(SpareBuffer(spare.clone()));
        spare
    }

    fn timed_slot(&mut self, owner: Entity) -> Option<u32> {
        if !self
            .get_resource::<GpuInfo>()
            .is_some_and(|gpu| gpu.timed)
        {
            return None;
        }
        let most = self.resource::<RenderTuning>().timed_slots;
        self.resource_mut::<TimedSlots>().take(owner, most)
    }
}

/// The render crate's methods on [`EntityWorldMut`].
trait SceneEntityExt {
    /// Draws `mesh` in `look`, in place of what it drew.
    fn draw(&mut self, mesh: Handle<Mesh>, look: Look);

    /// Lays a decal of `material` in place of what it drew, drawn
    /// when `drawn`.
    fn draw_decal(
        &mut self,
        material: Handle<DecalMaterial>,
        drawn: bool,
    );
}

impl SceneEntityExt for EntityWorldMut<'_> {
    fn draw(&mut self, mesh: Handle<Mesh>, look: Look) {
        match look {
            Look::Standard(material) => {
                self.remove::<Drawn>()
                    .insert((Mesh3d(mesh), MeshMaterial3d(material)));
            }
            Look::Shaded(material) => {
                self.remove::<Drawn>()
                    .insert((Mesh3d(mesh), MeshMaterial3d(material)));
            }
            // A decal's material draws only a decal.
            Look::Decal(_) => {}
        }
    }

    fn draw_decal(
        &mut self,
        material: Handle<DecalMaterial>,
        drawn: bool,
    ) {
        self.remove::<Drawn>().insert(MeshMaterial3d(material));
        // Bevy gives a decal its square as it is added, so it is
        // added anew each time.
        if drawn {
            self.insert(ForwardDecal);
        }
    }
}

/// The shader Bevy is given for `checked`, with wgpu's checks on as
/// it is made: bounds and loops.
fn checked(checked: CheckedShader) -> Shader {
    let mut hasher = DefaultHasher::new();
    checked.wgsl.hash(&mut hasher);
    let path = format!("hamlet_plugin_{:016x}.wgsl", hasher.finish());
    let mut shader = Shader::from_wgsl(checked.wgsl, path);
    shader.validate_shader = ValidateShader::Enabled;
    shader
}

/// An image a camera draws into, `width` by `height` pixels.
pub(crate) fn target_image(width: u32, height: u32) -> Image {
    Image::new_target_texture(
        width,
        height,
        TextureFormat::Rgba8UnormSrgb,
        None,
    )
}

/// Lets go of the window's background once its owner draws there no
/// more.
fn release_backdrop(
    cameras: Query<&SceneCamera, With<Camera>>,
    mut backdrop: ResMut<Backdrop>,
) {
    let Some(owner) = backdrop.0 else {
        return;
    };
    let held = cameras.iter().any(|camera| {
        camera.owner == owner && camera.image.is_none()
    });
    if !held {
        backdrop.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::query::Without;

    use super::*;

    #[test]
    fn what_was_under_a_dropped_entity_is_named_no_more() {
        let mut world = World::new();
        let owner = world.spawn_empty().id();
        let since = Instant::now();
        let parent = |id, parent| SceneChange::Parent { id, parent };
        world.apply_scene(
            owner,
            since,
            vec![
                SceneChange::Entity {
                    id: 1,
                    key: 0,
                    parent: None,
                },
                SceneChange::Entity {
                    id: 2,
                    key: 0,
                    parent: Some(1),
                },
                // Under its own child, it would leave the scene.
                parent(1, Some(2)),
                SceneChange::Instances {
                    id: 3,
                    key: 0,
                    parent: Some(2),
                    mesh: 0,
                    material: 0,
                },
                SceneChange::Copies {
                    id: 3,
                    transforms: vec![Transform::default()],
                },
                SceneChange::Drop(1),
                SceneChange::Watch {
                    id: 3,
                    hover: true,
                    click: true,
                },
                SceneChange::Entity {
                    id: 4,
                    key: 0,
                    parent: Some(2),
                },
            ],
        );
        let scene = world.scene_of(owner).expect("a scene");
        assert_eq!(scene.entities(), 1);
        let outside = world
            .query_filtered::<(), (With<ScenePart>, Without<ChildOf>)>()
            .iter(&world)
            .count();
        assert_eq!(outside, 0);
    }
}
