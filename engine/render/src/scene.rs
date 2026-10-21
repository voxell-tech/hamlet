//! Each owner's scene: an entity of its own at its root, holding the
//! Bevy entity, mesh and material of each number the owner's changes
//! name.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Instant;

use bevy::app::{App, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::visibility::Visibility;
use bevy::camera::{
    Camera, Camera3d, Hdr, ImageRenderTarget, RenderTarget,
};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
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
use bevy::pbr::{DistanceFog, MeshMaterial3d, StandardMaterial};
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
use crate::guard::{GpuInfo, TimedSlots};
use crate::label::WorldLabel;
use crate::material::{self, ShadedMaterial};
use crate::pass::ScenePass;
use crate::pick::Watched;
use crate::shader::CheckedShader;
use crate::viewport::{RenderTuning, ViewPointer};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Backdrop>()
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
}

/// A material of a scene: Bevy's own, or one of the owner's shader.
#[derive(Clone, Debug)]
enum Look {
    Standard(Handle<StandardMaterial>),
    Shaded(Handle<ShadedMaterial>),
}

/// A buffer bound where an owner gave none.
#[derive(Resource, Clone, Debug)]
struct SpareBuffer(Handle<ShaderBuffer>);

/// The copies of instances, under their own entity.
struct Copies {
    root: Entity,
    key: u32,
    mesh: u32,
    material: u32,
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
    fn new(owner: Entity, since: Instant) -> Self {
        Self {
            owner,
            since,
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
                let under = self.under(parent, root);
                if self.entity(id) != Some(under) {
                    self.insert(world, id, ChildOf(under));
                }
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
                }
            }
            SceneChange::Undraw(id) => {
                self.remove::<(
                    Mesh3d,
                    MeshMaterial3d<StandardMaterial>,
                    MeshMaterial3d<ShadedMaterial>,
                )>(world, id);
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
                        world.entity_mut(copy).insert(watched);
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
                let Some((held, kept)) = self.buffers.get_mut(&id)
                else {
                    return;
                };
                let at = offset as usize;
                let Some(into) = kept.get_mut(at..at + bytes.len())
                else {
                    return;
                };
                into.copy_from_slice(&bytes);
                if let Some(mut buffer) = world
                    .resource_mut::<Assets<ShaderBuffer>>()
                    .get_mut(held.id())
                {
                    buffer.data = Some(kept.clone());
                }
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
                let Some(entity) = self.camera(camera) else {
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
                self.instances.insert(
                    id,
                    Copies {
                        root: made,
                        key,
                        mesh,
                        material,
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
                world.set_up(camera, setup);
                self.cameras.insert(id, camera);
            }
            SceneChange::Setup { id, setup } => {
                if let Some(camera) = self.camera(id) {
                    world.set_up(camera, setup);
                }
            }
            SceneChange::Drop(id) => self.drop(world, id),
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
        let mesh = self.instances.get(&id).and_then(|copies| {
            self.meshes.get(&copies.mesh).cloned()
        });
        let material = self.instances.get(&id).and_then(|copies| {
            self.materials.get(&copies.material).cloned()
        });
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
                ChildOf(copies.root),
            ));
            if let (Some(mesh), Some(look)) = (&mesh, &material) {
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
        if let Some(gone) = gone
            && let Ok(gone) = world.get_entity_mut(gone)
        {
            gone.despawn();
        }
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
                self.spawn((Scene::new(owner, since), SceneOf(owner)))
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
            // Drawn once a viewport shows it.
            (target, Some(image), 0, false)
        };
        self.spawn((
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
        ))
        .id()
    }

    fn set_up(&mut self, camera: Entity, setup: CameraSetup) {
        let Ok(mut camera) = self.get_entity_mut(camera) else {
            return;
        };
        if let Some(mut drawn) = camera.get_mut::<Camera>() {
            drawn.clear_color = setup.clear;
        }
        camera.insert((
            setup.projection,
            setup.tonemapping,
            setup.exposure,
            setup.ambient,
            setup.msaa,
        ));
        if setup.hdr {
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
}

impl SceneEntityExt for EntityWorldMut<'_> {
    fn draw(&mut self, mesh: Handle<Mesh>, look: Look) {
        self.insert(Mesh3d(mesh));
        match look {
            Look::Standard(material) => {
                self.remove::<MeshMaterial3d<ShadedMaterial>>()
                    .insert(MeshMaterial3d(material));
            }
            Look::Shaded(material) => {
                self.remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert(MeshMaterial3d(material));
            }
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
