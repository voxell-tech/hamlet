//! 3D scenes as components of the plugin's own entities. An entity
//! with a [`Transform`] is an entity of the engine's scene, and its
//! [`Mesh3d`], [`MeshMaterial3d`], [`ForwardDecal`], [`SceneLight`],
//! [`SceneCamera`], [`SceneLabel`] and the rest are sent as they
//! change, in
//! [`PostUpdate`]. Despawning it takes it away in the engine. What
//! the person does over a view comes as messages.
//!
//! ```ignore
//! fn setup(mut commands: Commands, mut assets: ResMut<SceneAssets>) -> Result<(), String> {
//!     commands.spawn((
//!         SceneCamera::viewport(CameraSettings::default()),
//!         Transform::from_xyz(0.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
//!     ));
//!     commands.spawn((
//!         Mesh3d(assets.add(Mesh::shape(Shape::Cuboid((1.0, 1.0, 1.0)))?)),
//!         MeshMaterial3d(assets.add(Material::make(Pbr::colored(RED))?)),
//!     ));
//!     Ok(())
//! }
//! ```

use core::marker::PhantomData;
use std::collections::HashMap;
use std::sync::{Arc, Weak};

use bevy_app::{App, Last, PostUpdate};
use bevy_ecs::prelude::*;
use bevy_math::Vec3;
use bevy_transform::components::Transform;

use crate::audio::SoundsEnding;
pub use crate::bindings::exports::hamlet::engine::scene_guest::MouseButton;
use crate::bindings::exports::hamlet::engine::scene_guest::{
    Hit as WitHit, SceneInput,
};
pub use crate::bindings::hamlet::engine::gpu::{
    Buffer, Compute, Shaded, Shader, ShaderKind, Texture,
};
use crate::bindings::hamlet::engine::gpu::{
    decal_material, set_pass, shaded_decal, shaded_material,
};
use crate::bindings::hamlet::engine::scene::{
    self as wit, Camera, Entity as HostHandle,
    Instances as HostCopies,
};
pub use crate::bindings::hamlet::engine::scene::{
    AlphaMode, Bloom, CameraSettings, Decal, DirectionalLight, Fog,
    FogFalloff, Light, Material, Mesh, MeshData, Pbr, PointLight,
    Projection, Rgba, Shape, SpotLight, Target, Tonemapping,
};
use crate::ui::{AnyView, Cx, Key, Ui, column};

/// Keys made from entities, apart from those a [`SceneKey`] gives.
const MADE_KEYS: u32 = 1 << 31;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<SceneAssets>()
        .init_resource::<SceneKeys>()
        .init_resource::<Viewports>()
        .init_resource::<Animate>()
        .add_message::<SceneOver>()
        .add_message::<SceneOut>()
        .add_message::<SceneClick>()
        .add_message::<ScenePointer>()
        .add_message::<ScenePress>()
        .add_message::<SceneWheel>()
        .add_message::<SceneResize>()
        .add_observer(forget_entity)
        .add_observer(forget_instances)
        .add_systems(
            PostUpdate,
            (
                make_entities,
                (
                    parent,
                    place,
                    draw,
                    lay,
                    light,
                    shadow,
                    label,
                    watch,
                    make_cameras,
                ),
                (set_passes, make_instances, show_viewports),
            )
                .chain()
                .in_set(SceneSync),
        )
        .add_systems(Last, free_unused);
}

/// The systems that send the scene's changes, in [`PostUpdate`].
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct SceneSync;

/// A handle of a scene asset of the type `T`, kept in
/// [`SceneAssets`] while any clone of it is held.
pub struct Handle<T> {
    index: u32,
    // Counts the clones held: the asset is let go after the last.
    user: Arc<()>,
    marker: PhantomData<fn() -> T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            index: self.index,
            user: Arc::clone(&self.user),
            marker: PhantomData,
        }
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T> Eq for Handle<T> {}

impl<T> core::fmt::Debug for Handle<T> {
    fn fmt(
        &self,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result {
        write!(f, "Handle({})", self.index)
    }
}

/// The meshes, materials, shaders, buffers, textures and compute the
/// plugin made in the engine, each kept while a [`Handle`] of it is
/// held, and let go in [`Last`] after the last is dropped, which
/// takes it away in the engine once nothing draws with it.
#[derive(Resource, Default)]
pub struct SceneAssets {
    meshes: Store<Mesh>,
    materials: Store<Material>,
    shaders: Store<Shader>,
    buffers: Store<Buffer>,
    textures: Store<Texture>,
    computes: Store<Compute>,
}

/// The assets of one type, by the index of their handles.
#[doc(hidden)]
pub struct Store<T> {
    slots: Vec<Option<Slot<T>>>,
    /// Slots let go, to be filled again.
    free: Vec<u32>,
}

struct Slot<T> {
    asset: T,
    users: Weak<()>,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Store<T> {
    pub(crate) fn add(&mut self, asset: T) -> Handle<T> {
        let user = Arc::new(());
        let slot = Some(Slot {
            asset,
            users: Arc::downgrade(&user),
        });
        let index = match self.free.pop() {
            Some(index) => {
                self.slots[index as usize] = slot;
                index
            }
            None => {
                self.slots.push(slot);
                self.slots.len() as u32 - 1
            }
        };
        Handle {
            index,
            user,
            marker: PhantomData,
        }
    }

    pub(crate) fn get(&self, handle: &Handle<T>) -> Option<&T> {
        let slot = self.slots.get(handle.index as usize)?.as_ref()?;
        Some(&slot.asset)
    }

    /// Lets go of each asset no handle holds any more.
    pub(crate) fn free_unused(&mut self) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot
                .as_ref()
                .is_some_and(|slot| slot.users.strong_count() == 0)
            {
                *slot = None;
                self.free.push(index as u32);
            }
        }
    }
}

/// A thing of the engine's kept in [`SceneAssets`].
pub trait SceneAsset: Sized {
    #[doc(hidden)]
    fn store(assets: &SceneAssets) -> &Store<Self>;
    #[doc(hidden)]
    fn store_mut(assets: &mut SceneAssets) -> &mut Store<Self>;
}

macro_rules! scene_asset {
    ($type:ty, $field:ident) => {
        impl SceneAsset for $type {
            fn store(assets: &SceneAssets) -> &Store<Self> {
                &assets.$field
            }

            fn store_mut(
                assets: &mut SceneAssets,
            ) -> &mut Store<Self> {
                &mut assets.$field
            }
        }
    };
}

scene_asset!(Mesh, meshes);
scene_asset!(Material, materials);
scene_asset!(Shader, shaders);
scene_asset!(Buffer, buffers);
scene_asset!(Texture, textures);
scene_asset!(Compute, computes);

impl SceneAssets {
    /// Keeps `asset`, made in the engine, while the handle given or a
    /// clone of it is held.
    pub fn add<T: SceneAsset>(&mut self, asset: T) -> Handle<T> {
        T::store_mut(self).add(asset)
    }

    pub fn get<T: SceneAsset>(
        &self,
        handle: &Handle<T>,
    ) -> Option<&T> {
        T::store(self).get(handle)
    }

    /// A material drawn by the material shader `shader`, with
    /// `params`, `texture` and `data` bound.
    pub fn add_shaded_material(
        &mut self,
        shader: &Handle<Shader>,
        settings: Shaded,
        params: &[f32],
        texture: Option<&Handle<Texture>>,
        data: Option<&Handle<Buffer>>,
    ) -> Result<Handle<Material>, String> {
        let shader = self.get(shader).ok_or("the shader is gone")?;
        let texture = texture.and_then(|texture| self.get(texture));
        let data = data.and_then(|data| self.get(data));
        let material =
            shaded_material(shader, settings, params, texture, data)?;
        Ok(self.add(material))
    }

    /// A material for a [`ForwardDecal`], its colour times
    /// `texture`'s.
    pub fn add_decal_material(
        &mut self,
        decal: Decal,
        texture: &Handle<Texture>,
    ) -> Result<Handle<Material>, String> {
        let texture =
            self.get(texture).ok_or("the texture is gone")?;
        let material = decal_material(decal, texture)?;
        Ok(self.add(material))
    }

    /// A material for a [`ForwardDecal`] drawn by the decal shader
    /// `shader` through a box one unit each way, blended by
    /// `alpha_mode`, with `params`, `texture` and `data` bound.
    pub fn add_shaded_decal(
        &mut self,
        shader: &Handle<Shader>,
        alpha_mode: AlphaMode,
        params: &[f32],
        texture: Option<&Handle<Texture>>,
        data: Option<&Handle<Buffer>>,
    ) -> Result<Handle<Material>, String> {
        let shader = self.get(shader).ok_or("the shader is gone")?;
        let texture = texture.and_then(|texture| self.get(texture));
        let data = data.and_then(|data| self.get(data));
        let material =
            shaded_decal(shader, alpha_mode, params, texture, data)?;
        Ok(self.add(material))
    }

    /// Compute of the shader `shader` over `data`, and `extra` when
    /// given.
    pub fn add_compute(
        &mut self,
        shader: &Handle<Shader>,
        data: &Handle<Buffer>,
        extra: Option<&Handle<Buffer>>,
    ) -> Result<Handle<Compute>, String> {
        let shader = self.get(shader).ok_or("the shader is gone")?;
        let data = self.get(data).ok_or("the buffer is gone")?;
        let extra = extra.and_then(|extra| self.get(extra));
        let compute = Compute::make(shader, data, extra)?;
        Ok(self.add(compute))
    }

    fn free_unused(&mut self) {
        self.meshes.free_unused();
        self.materials.free_unused();
        self.shaders.free_unused();
        self.buffers.free_unused();
        self.textures.free_unused();
        self.computes.free_unused();
    }

    /// Gives a material
    /// [`add_shaded_material`](Self::add_shaded_material) or
    /// [`add_shaded_decal`](Self::add_shaded_decal) made new
    /// `params`.
    pub fn set_params(
        &self,
        material: &Handle<Material>,
        params: &[f32],
    ) -> Result<(), String> {
        let material =
            self.get(material).ok_or("the material is gone")?;
        crate::bindings::hamlet::engine::gpu::set_params(
            material, params,
        )
    }
}

impl Pbr {
    /// A material of `color`, rough and not metal, lit and fogged.
    pub fn colored(color: Rgba) -> Self {
        Self {
            base_color: color,
            emissive: (0.0, 0.0, 0.0, 1.0),
            roughness: 0.5,
            metallic: 0.0,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            unlit: false,
            double_sided: false,
            fog: true,
        }
    }
}

impl Default for CameraSettings {
    /// A perspective of 45 degrees from 0.1 to 1000, clearing to
    /// black, in high range, neither fogged nor blooming.
    fn default() -> Self {
        Self {
            projection: Projection::Perspective((
                core::f32::consts::FRAC_PI_4,
                0.1,
                1000.0,
            )),
            clear: Some((0.0, 0.0, 0.0, 1.0)),
            hdr: true,
            tonemapping: Tonemapping::Neutral,
            exposure: 9.7,
            fog: None,
            bloom: None,
            ambient: ((1.0, 1.0, 1.0, 1.0), 80.0),
            msaa: 4,
        }
    }
}

/// The plugin's name for an entity in what the engine tells it, in
/// place of one made from the entity.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SceneKey(pub u32);

/// The mesh an entity draws, with its [`MeshMaterial3d`].
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[require(Transform)]
pub struct Mesh3d(pub Handle<Mesh>);

/// The material an entity draws its [`Mesh3d`] in, or lays as its
/// [`ForwardDecal`].
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[require(Transform)]
pub struct MeshMaterial3d(pub Handle<Material>);

/// Lays the entity's [`MeshMaterial3d`], a decal's
/// (`Material::decal`, [`SceneAssets::add_decal_material`] or
/// [`SceneAssets::add_shaded_decal`]), on what lies under and over a
/// square one unit wide on the entity's x and z, placed, turned and
/// sized by its transform.
#[derive(Component, Clone, Copy, Default, Debug)]
#[require(Transform)]
pub struct ForwardDecal;

/// The light an entity gives.
#[derive(Component, Clone, Debug)]
#[require(Transform)]
pub struct SceneLight(pub Light);

/// Whether an entity casts shadows and is shadowed.
#[derive(Component, Clone, Copy, Debug)]
pub struct Shadows {
    pub cast: bool,
    pub receive: bool,
}

/// Words shown over an entity in each view of the scene.
#[derive(Component, Clone, Debug)]
#[require(Transform)]
pub struct SceneLabel(pub String);

/// What the engine tells of an entity or of [`Instances`]: the
/// pointer coming over it and leaving, as [`SceneOver`] and
/// [`SceneOut`], and clicks, as [`SceneClick`].
#[derive(Component, Clone, Copy, Default, Debug)]
pub struct Pickable {
    pub hover: bool,
    pub click: bool,
}

/// A camera at an entity, looking along its forward.
#[derive(Component, Clone, Debug)]
#[require(Transform)]
pub struct SceneCamera {
    pub settings: CameraSettings,
    pub target: Target,
}

impl SceneCamera {
    /// A camera drawing into a [`viewport`] of a panel.
    pub fn viewport(settings: CameraSettings) -> Self {
        Self {
            settings,
            target: Target::Viewport,
        }
    }
}

/// A pass shader run over the image of the [`SceneCamera`] of its
/// entity, with `params`.
#[derive(Component, Clone, Debug)]
pub struct PostPass {
    pub shader: Handle<Shader>,
    pub params: Vec<f32>,
}

/// Copies of one mesh in one material, drawn together, under the
/// entity of the entity's `ChildOf`, or at the scene's root. Copy `i`
/// is tagged `i` for a shader.
#[derive(Component, Clone, Debug)]
pub struct Instances {
    pub mesh: Handle<Mesh>,
    pub material: Handle<Material>,
    pub copies: Vec<Transform>,
}

/// Has the engine call `tick` each frame while true, so the app is
/// updated each frame.
#[derive(Resource, Clone, Copy, Default, Debug)]
pub struct Animate(pub bool);

/// Where the pointer met a [`Pickable`] entity, or a copy of
/// [`Instances`].
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub entity: Entity,
    pub index: u32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// The pointer came over a [`Pickable`].
#[derive(Message, Clone, Copy, Debug)]
pub struct SceneOver(pub Hit);

/// The pointer left the copy `index` of a [`Pickable`].
#[derive(Message, Clone, Copy, Debug)]
pub struct SceneOut {
    pub entity: Entity,
    pub index: u32,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct SceneClick(pub Hit);

/// The pointer over the view of the camera of `view`, in its pixels
/// from its top left, and how far it moved.
#[derive(Message, Clone, Copy, Debug)]
pub struct ScenePointer {
    pub view: Entity,
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct ScenePress {
    pub view: Entity,
    pub button: MouseButton,
    pub down: bool,
    pub x: f32,
    pub y: f32,
}

/// The wheel turned over a view, in lines.
#[derive(Message, Clone, Copy, Debug)]
pub struct SceneWheel {
    pub view: Entity,
    pub x: f32,
    pub y: f32,
}

/// A view's new size.
#[derive(Message, Clone, Copy, Debug)]
pub struct SceneResize {
    pub view: Entity,
    pub width: f32,
    pub height: f32,
}

/// A frame of a panel showing the image of the [`SceneCamera`] of
/// `camera`, filling the room the layout gives it.
pub fn viewport(camera: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(column(()));
        cx.world.resource_mut::<Viewports>().0.push(Viewport {
            node,
            camera,
            shown: false,
        });
        node
    })
}

/// The engine's entity of an entity.
#[derive(Component)]
pub(crate) struct HostEntity {
    pub(crate) entity: HostHandle,
    key: u32,
}

#[derive(Component)]
struct HostCamera {
    camera: Camera,
    target: Target,
}

#[derive(Component)]
struct HostInstances {
    instances: HostCopies,
    key: u32,
    mesh: Handle<Mesh>,
    material: Handle<Material>,
}

/// An entity the engine refused to make, not asked again.
#[derive(Component)]
struct Refused;

/// The entity of each key the engine tells of.
#[derive(Resource, Default)]
struct SceneKeys(HashMap<u32, Entity>);

struct Viewport {
    node: Key,
    camera: Entity,
    shown: bool,
}

#[derive(Resource, Default)]
struct Viewports(Vec<Viewport>);

fn key_of(entity: Entity, key: Option<&SceneKey>) -> u32 {
    key.map_or(MADE_KEYS | entity.index_u32(), |key| key.0)
}

fn host_transform(transform: &Transform) -> wit::Transform {
    let rotation = transform.rotation;
    wit::Transform {
        translation: transform.translation.into(),
        rotation: (rotation.x, rotation.y, rotation.z, rotation.w),
        scale: transform.scale.into(),
    }
}

fn make_entities(
    mut commands: Commands,
    made: Query<
        (Entity, Option<&SceneKey>),
        (
            With<Transform>,
            Without<HostEntity>,
            Without<Instances>,
            Without<Refused>,
        ),
    >,
    mut keys: ResMut<SceneKeys>,
) {
    for (entity, key) in &made {
        let key = key_of(entity, key);
        match HostHandle::make(None, key) {
            Ok(made) => {
                keys.0.insert(key, entity);
                commands
                    .entity(entity)
                    .insert(HostEntity { entity: made, key });
            }
            Err(why) => {
                crate::error(&format!(
                    "an entity is not made: {why}"
                ));
                commands.entity(entity).insert(Refused);
            }
        }
    }
}

fn parent(
    moved: Query<
        (&HostEntity, Option<&ChildOf>),
        Or<(Added<HostEntity>, Changed<ChildOf>)>,
    >,
    hosts: Query<&HostEntity>,
    mut orphans: RemovedComponents<ChildOf>,
) {
    for (host, child_of) in &moved {
        if let Some(parent) = child_of
            .and_then(|child_of| hosts.get(child_of.parent()).ok())
        {
            host.entity.set_parent(Some(&parent.entity));
        }
    }
    for orphan in orphans.read() {
        if let Ok(host) = hosts.get(orphan) {
            host.entity.set_parent(None);
        }
    }
}

fn place(
    placed: Query<
        (&HostEntity, &Transform),
        Or<(Changed<Transform>, Added<HostEntity>)>,
    >,
) {
    for (host, transform) in &placed {
        host.entity.set_transform(host_transform(transform));
    }
}

fn draw(
    drawn: Query<
        (&HostEntity, &Mesh3d, &MeshMaterial3d),
        Or<(
            Changed<Mesh3d>,
            Changed<MeshMaterial3d>,
            Added<HostEntity>,
        )>,
    >,
    hosts: Query<&HostEntity>,
    assets: Res<SceneAssets>,
    mut undrawn: RemovedComponents<Mesh3d>,
) {
    for (host, mesh, material) in &drawn {
        if let (Some(mesh), Some(material)) =
            (assets.get(&mesh.0), assets.get(&material.0))
        {
            host.entity.draw(mesh, material);
        }
    }
    for entity in undrawn.read() {
        if let Ok(host) = hosts.get(entity) {
            host.entity.undraw();
        }
    }
}

fn lay(
    laid: Query<
        (&HostEntity, &MeshMaterial3d),
        (
            With<ForwardDecal>,
            Or<(
                Changed<ForwardDecal>,
                Changed<MeshMaterial3d>,
                Added<HostEntity>,
            )>,
        ),
    >,
    hosts: Query<&HostEntity>,
    assets: Res<SceneAssets>,
    mut lifted: RemovedComponents<ForwardDecal>,
) {
    for (host, material) in &laid {
        if let Some(material) = assets.get(&material.0) {
            host.entity.draw_decal(material);
        }
    }
    for entity in lifted.read() {
        if let Ok(host) = hosts.get(entity) {
            host.entity.undraw();
        }
    }
}

fn light(
    lit: Query<
        (&HostEntity, &SceneLight),
        Or<(Changed<SceneLight>, Added<HostEntity>)>,
    >,
    hosts: Query<&HostEntity>,
    mut unlit: RemovedComponents<SceneLight>,
) {
    for (host, light) in &lit {
        if let Err(why) = host.entity.set_light(Some(light.0)) {
            crate::error(&format!("a light is not made: {why}"));
        }
    }
    for entity in unlit.read() {
        if let Ok(host) = hosts.get(entity) {
            let _ = host.entity.set_light(None);
        }
    }
}

fn shadow(
    shadowed: Query<
        (&HostEntity, &Shadows),
        Or<(Changed<Shadows>, Added<HostEntity>)>,
    >,
) {
    for (host, shadows) in &shadowed {
        host.entity.set_shadows(shadows.cast, shadows.receive);
    }
}

fn label(
    labelled: Query<
        (&HostEntity, &SceneLabel),
        Or<(Changed<SceneLabel>, Added<HostEntity>)>,
    >,
    hosts: Query<&HostEntity>,
    mut unlabelled: RemovedComponents<SceneLabel>,
) {
    for (host, label) in &labelled {
        host.entity.set_label(Some(&label.0));
    }
    for entity in unlabelled.read() {
        if let Ok(host) = hosts.get(entity) {
            host.entity.set_label(None);
        }
    }
}

fn watch(
    watched: Query<
        (&HostEntity, &Pickable),
        Or<(Changed<Pickable>, Added<HostEntity>)>,
    >,
    hosts: Query<&HostEntity>,
    mut unwatched: RemovedComponents<Pickable>,
) {
    for (host, pickable) in &watched {
        host.entity.watch(pickable.hover, pickable.click);
    }
    for entity in unwatched.read() {
        if let Ok(host) = hosts.get(entity) {
            host.entity.watch(false, false);
        }
    }
}

fn make_cameras(
    mut commands: Commands,
    cameras: Query<
        (Entity, &HostEntity, &SceneCamera, Option<&HostCamera>),
        Or<(Changed<SceneCamera>, Added<HostEntity>)>,
    >,
    mut gone: RemovedComponents<SceneCamera>,
) {
    for (entity, host, camera, made) in &cameras {
        if let Some(made) = made
            && made.target == camera.target
        {
            made.camera.set(camera.settings);
            continue;
        }
        match Camera::make(
            &host.entity,
            camera.settings,
            camera.target,
        ) {
            Ok(made) => {
                commands.entity(entity).insert(HostCamera {
                    camera: made,
                    target: camera.target,
                });
            }
            Err(why) => {
                crate::error(&format!("a camera is not made: {why}"));
            }
        }
    }
    for entity in gone.read() {
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.try_remove::<HostCamera>();
        }
    }
}

fn set_passes(
    passes: Query<
        (&HostCamera, &PostPass),
        Or<(Changed<PostPass>, Added<HostCamera>)>,
    >,
    cameras: Query<&HostCamera>,
    assets: Res<SceneAssets>,
    mut gone: RemovedComponents<PostPass>,
) {
    for (camera, pass) in &passes {
        let Some(shader) = assets.get(&pass.shader) else {
            continue;
        };
        if let Err(why) =
            set_pass(&camera.camera, Some(shader), &pass.params)
        {
            crate::error(&format!("a pass is not set: {why}"));
        }
    }
    for entity in gone.read() {
        if let Ok(camera) = cameras.get(entity) {
            let _ = set_pass(&camera.camera, None, &[]);
        }
    }
}

fn make_instances(
    mut commands: Commands,
    changed: Query<
        (
            Entity,
            &Instances,
            Option<&SceneKey>,
            Option<&ChildOf>,
            Option<&HostInstances>,
            Option<&Pickable>,
        ),
        (
            Or<(Changed<Instances>, Changed<Pickable>)>,
            Without<Refused>,
        ),
    >,
    parents: Query<&HostEntity>,
    assets: Res<SceneAssets>,
    mut keys: ResMut<SceneKeys>,
) {
    for (entity, instances, key, child_of, made, pickable) in &changed
    {
        let copies = instances
            .copies
            .iter()
            .flat_map(|copy| {
                let wit::Transform {
                    translation: (x, y, z),
                    rotation: (rx, ry, rz, rw),
                    scale: (sx, sy, sz),
                } = host_transform(copy);
                [x, y, z, rx, ry, rz, rw, sx, sy, sz]
            })
            .collect::<Vec<_>>();
        let pickable = pickable.copied().unwrap_or_default();
        if let Some(made) = made
            && made.mesh == instances.mesh
            && made.material == instances.material
        {
            if let Err(why) = made.instances.set(&copies) {
                crate::error(&format!("copies are not set: {why}"));
            }
            made.instances.watch(pickable.hover, pickable.click);
            continue;
        }
        let (Some(mesh), Some(material)) = (
            assets.get(&instances.mesh),
            assets.get(&instances.material),
        ) else {
            continue;
        };
        let parent = child_of
            .and_then(|child_of| parents.get(child_of.parent()).ok())
            .map(|parent| &parent.entity);
        let key = key_of(entity, key);
        let made = HostCopies::make(parent, mesh, material, key)
            .and_then(|made| made.set(&copies).map(|()| made));
        match made {
            Ok(made) => {
                made.watch(pickable.hover, pickable.click);
                keys.0.insert(key, entity);
                commands.entity(entity).insert(HostInstances {
                    instances: made,
                    key,
                    mesh: instances.mesh.clone(),
                    material: instances.material.clone(),
                });
            }
            Err(why) => {
                crate::error(&format!(
                    "instances are not made: {why}"
                ));
                commands.entity(entity).insert(Refused);
            }
        }
    }
}

fn show_viewports(
    ui: Res<Ui>,
    mut viewports: ResMut<Viewports>,
    cameras: Query<Ref<HostCamera>>,
) {
    viewports.0.retain(|viewport| ui.nodes.has(viewport.node));
    for viewport in &mut viewports.0 {
        let Ok(camera) = cameras.get(viewport.camera) else {
            continue;
        };
        if viewport.shown && !camera.is_added() {
            continue;
        }
        if let Some(node) = ui.nodes.node(viewport.node) {
            camera.camera.show_in(node);
            viewport.shown = true;
        }
    }
}

/// Asks for a tick each frame while the plugin animates, or a sound
/// plays toward its end.
pub(crate) fn animate(
    animate: Res<Animate>,
    ending: Res<SoundsEnding>,
    mut asked: Local<Option<bool>>,
) {
    let want = animate.0 || ending.0;
    if asked.replace(want) != Some(want) {
        wit::animate(want);
    }
}

fn free_unused(mut assets: ResMut<SceneAssets>) {
    assets.bypass_change_detection().free_unused();
}

fn forget_entity(
    removed: On<Remove, HostEntity>,
    hosts: Query<&HostEntity>,
    mut keys: ResMut<SceneKeys>,
) {
    if let Ok(host) = hosts.get(removed.entity)
        && keys.0.get(&host.key) == Some(&removed.entity)
    {
        keys.0.remove(&host.key);
    }
}

fn forget_instances(
    removed: On<Remove, HostInstances>,
    hosts: Query<&HostInstances>,
    mut keys: ResMut<SceneKeys>,
) {
    if let Ok(host) = hosts.get(removed.entity)
        && keys.0.get(&host.key) == Some(&removed.entity)
    {
        keys.0.remove(&host.key);
    }
}

/// Writes `input` into the world as the message it is, for an entity
/// or view still there.
pub(crate) fn deliver(world: &mut World, input: SceneInput) {
    let entity = |world: &World, key: u32| {
        world.resource::<SceneKeys>().0.get(&key).copied()
    };
    let hit = |world: &World, hit: WitHit| {
        Some(Hit {
            entity: entity(world, hit.key)?,
            index: hit.index,
            point: hit.point.into(),
            normal: hit.normal.into(),
        })
    };
    match input {
        SceneInput::Over(at) => {
            if let Some(at) = hit(world, at) {
                world.write_message(SceneOver(at));
            }
        }
        SceneInput::Out((key, index)) => {
            if let Some(entity) = entity(world, key) {
                world.write_message(SceneOut { entity, index });
            }
        }
        SceneInput::Click(at) => {
            if let Some(at) = hit(world, at) {
                world.write_message(SceneClick(at));
            }
        }
        SceneInput::Moved(pointer) => {
            if let Some(view) = entity(world, pointer.view) {
                world.write_message(ScenePointer {
                    view,
                    x: pointer.x,
                    y: pointer.y,
                    dx: pointer.dx,
                    dy: pointer.dy,
                });
            }
        }
        SceneInput::Pressed(press) => {
            if let Some(view) = entity(world, press.view) {
                world.write_message(ScenePress {
                    view,
                    button: press.button,
                    down: press.down,
                    x: press.x,
                    y: press.y,
                });
            }
        }
        SceneInput::Turned(wheel) => {
            if let Some(view) = entity(world, wheel.view) {
                world.write_message(SceneWheel {
                    view,
                    x: wheel.x,
                    y: wheel.y,
                });
            }
        }
        SceneInput::Resized((key, width, height)) => {
            if let Some(view) = entity(world, key) {
                world.write_message(SceneResize {
                    view,
                    width,
                    height,
                });
            }
        }
    }
}
