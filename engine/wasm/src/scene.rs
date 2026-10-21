//! A plugin's scenes, as it makes, changes and drops what is in them
//! in a call: each held to its budgets and its capabilities, and kept
//! as changes for the render crate to make once the call ends.

mod convert;
mod gpu;
mod serve;

#[cfg(test)]
mod tests;

use core::fmt;
use std::collections::HashMap;
use std::time::Instant;

use bevy::transform::components::Transform as Placed;
use hamlet_engine_api::{Capability, PluginLimits};
use hamlet_engine_render::{GpuInfo, SceneChange, ShaderKind};
use wasmtime::component::Resource;

pub(crate) use self::serve::plugin;
pub use self::serve::{SceneInbox, ShowsCamera};
use crate::instance::State;
use crate::nodes::UiNode;

pub(crate) mod scene_bindings {
    wasmtime::component::bindgen!({
        path: "../wit/hamlet_engine.wit",
        world: "scene-view",
        imports: { default: trappable },
        with: {
            "hamlet:engine/ui": crate::instance::ui_bindings::hamlet::engine::ui,
            "hamlet:engine/scene.mesh": super::MeshHandle,
            "hamlet:engine/scene.material": super::MaterialHandle,
            "hamlet:engine/scene.entity": super::EntityHandle,
            "hamlet:engine/scene.instances": super::InstancesHandle,
            "hamlet:engine/scene.camera": super::CameraHandle,
            "hamlet:engine/gpu.shader": super::gpu::ShaderHandle,
            "hamlet:engine/gpu.buffer": super::gpu::BufferHandle,
            "hamlet:engine/gpu.texture": super::gpu::TextureHandle,
            "hamlet:engine/gpu.compute": super::gpu::ComputeHandle,
        },
    });
}

use self::scene_bindings::hamlet::engine::scene::{
    CameraSettings, Host, HostCamera, HostEntity, HostInstances,
    HostMaterial, HostMesh, Light, MeshData, Pbr, Shape, Target,
    Transform,
};

/// The interfaces of scenes and GPU work a plugin may import.
pub(crate) const SCENE: &str = "hamlet:engine/scene@0.1.0";
pub(crate) const GPU: &str = "hamlet:engine/gpu@0.1.0";

/// Floats of one copy of instances: translation, rotation and scale.
const COPY: usize = 10;

pub struct MeshHandle;
pub struct MaterialHandle;
pub struct EntityHandle;
pub struct InstancesHandle;
pub struct CameraHandle;

/// What one call may spend on scenes, and what a plugin may hold.
#[derive(Clone, Copy, Debug)]
pub struct SceneCaps {
    pub entities: usize,
    pub vertices: usize,
    pub materials: usize,
    pub lights: usize,
    pub cameras: usize,
    pub changes: usize,
    pub gpu_memory: u64,
    pub gpu_objects: usize,
    pub shader_bytes: usize,
    /// Workgroups of one dispatch.
    pub workgroups: u32,
}

impl SceneCaps {
    pub fn of(budgets: &PluginLimits) -> Self {
        Self {
            entities: budgets.scene_entities,
            vertices: budgets.scene_vertices,
            materials: budgets.scene_materials,
            lights: budgets.scene_lights,
            cameras: budgets.scene_cameras,
            changes: budgets.scene_changes_per_call,
            gpu_memory: budgets.gpu_memory,
            gpu_objects: budgets.gpu_objects,
            shader_bytes: budgets.shader_bytes,
            workgroups: budgets.workgroups_per_frame,
        }
    }
}

/// The render capabilities, a flag each.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Powers(u8);

impl Powers {
    pub const SCENE: Self = Self(1);
    pub const SHADER: Self = Self(2);
    pub const COMPUTE: Self = Self(4);

    pub(crate) const ALL: [(Self, Capability); 3] = [
        (Self::SCENE, Capability::RenderScene),
        (Self::SHADER, Capability::RenderShader),
        (Self::COMPUTE, Capability::RenderCompute),
    ];

    pub fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// The numbers one thing of a scene uses.
type Uses = [Option<u32>; 3];

/// What a number of a plugin's scenes stands for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Held {
    Entity {
        light: bool,
        /// The mesh and material it draws.
        draws: Option<(u32, u32)>,
    },
    Mesh {
        vertices: usize,
        bytes: u64,
    },
    /// A material, of a plugin's shader when `shaded`, with the
    /// shader, texture and buffer it uses.
    Material {
        shaded: bool,
        uses: Uses,
    },
    Instances {
        copies: usize,
        mesh: u32,
        material: u32,
    },
    Camera {
        background: bool,
        /// The shader of its pass.
        pass: Option<u32>,
    },
    Shader(ShaderKind),
    Buffer {
        bytes: u64,
    },
    Texture {
        width: u32,
        height: u32,
    },
    /// Compute, with its shader and buffers.
    Compute {
        uses: Uses,
    },
}

impl Held {
    fn material(&self) -> bool {
        matches!(self, Self::Material { .. })
    }

    fn mesh(&self) -> bool {
        matches!(self, Self::Mesh { .. })
    }

    fn compute(&self) -> bool {
        matches!(self, Self::Compute { .. })
    }

    /// The numbers it draws or runs with.
    fn uses(&self) -> Uses {
        match *self {
            Self::Entity {
                draws: Some((mesh, material)),
                ..
            }
            | Self::Instances { mesh, material, .. } => {
                [Some(mesh), Some(material), None]
            }
            Self::Material { uses, .. } | Self::Compute { uses } => {
                uses
            }
            Self::Camera { pass, .. } => [pass, None, None],
            _ => [None; 3],
        }
    }
}

/// What a plugin holds in its scenes, counted against its budgets.
#[derive(Clone, Copy, Default, Debug)]
struct Counts {
    entities: usize,
    vertices: usize,
    materials: usize,
    lights: usize,
    cameras: usize,
    gpu_bytes: u64,
    /// Shaders, buffers, textures, compute and passes.
    gpu_objects: usize,
}

/// The scenes of a plugin, as its host holds them.
pub(crate) struct SceneState {
    /// When the numbering began; the render crate's scene of another
    /// numbering is taken away.
    pub(crate) since: Instant,
    next: u32,
    held: HashMap<u32, Held>,
    /// What the plugin let go of while something still draws or runs
    /// with it, counted against its budgets until nothing does.
    gone: HashMap<u32, Held>,
    /// How many things use each number.
    users: HashMap<u32, usize>,
    counts: Counts,
    pub(crate) caps: SceneCaps,
    pub(crate) changes: Vec<SceneChange>,
    /// Cameras to show in frames, by their numbers.
    pub(crate) shown: Vec<(u32, u32)>,
    /// Changes in the call under way.
    pub(crate) spent: usize,
    pub(crate) animate: bool,
    /// What it may use now, and what waits for the person.
    pub(crate) powers: Powers,
    pub(crate) asking: Powers,
    /// What it tried to use while the person had not answered.
    pub(crate) wanted: Powers,
    /// Whether another plugin draws behind the dock.
    pub(crate) backdrop_taken: bool,
    /// What the GPU allows.
    pub(crate) gpu: GpuInfo,
}

/// A plugin misused its scenes or went past a budget of one call.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SceneError {
    Changes,
    /// A number it never held, or one of another kind.
    Unknown,
    /// A number that is not finite, or out of its range.
    Invalid,
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changes => {
                write!(
                    f,
                    "it changed its scenes more than one call may"
                )
            }
            Self::Unknown => {
                write!(f, "it named a part of a scene it never made")
            }
            Self::Invalid => {
                write!(f, "it gave a scene a value out of its range")
            }
        }
    }
}

impl core::error::Error for SceneError {}

fn trap(error: SceneError) -> wasmtime::Error {
    wasmtime::Error::new(error)
}

impl SceneState {
    pub(crate) fn new(caps: SceneCaps) -> Self {
        Self {
            since: Instant::now(),
            next: 0,
            held: HashMap::new(),
            gone: HashMap::new(),
            users: HashMap::new(),
            counts: Counts::default(),
            caps,
            changes: Vec::new(),
            shown: Vec::new(),
            spent: 0,
            animate: false,
            powers: Powers::default(),
            asking: Powers::default(),
            wanted: Powers::default(),
            backdrop_taken: false,
            gpu: GpuInfo::default(),
        }
    }

    /// Counts `count` changes within the call's cap.
    fn spend(&mut self, count: usize) -> wasmtime::Result<()> {
        if self.spent.saturating_add(count) > self.caps.changes {
            return Err(trap(SceneError::Changes));
        }
        self.spent += count;
        Ok(())
    }

    /// Whether the plugin may use `power`; why not otherwise.
    pub(crate) fn allowed(
        &mut self,
        power: Powers,
        name: &str,
    ) -> Result<(), String> {
        if self.powers.has(power) {
            return Ok(());
        }
        if self.asking.has(power) {
            self.wanted = self.wanted.with(power);
            return Err(format!(
                "{name} waits for the person to allow it"
            ));
        }
        Err(format!("{name} is not allowed"))
    }

    /// Numbers `held`, a new thing of the plugin's.
    fn number(&mut self, held: Held) -> u32 {
        self.next += 1;
        self.keep(self.next, held);
        self.next
    }

    /// Keeps `held` as the number `id`, using what it uses in place
    /// of what it used.
    fn keep(&mut self, id: u32, held: Held) {
        for used in held.uses().into_iter().flatten() {
            *self.users.entry(used).or_default() += 1;
        }
        if let Some(was) = self.held.insert(id, held) {
            self.release(was.uses());
        }
    }

    /// Lets go of `uses`, freeing what was let go of already and is
    /// used no more.
    fn release(&mut self, uses: Uses) {
        for used in uses.into_iter().flatten() {
            let Some(count) = self.users.get_mut(&used) else {
                continue;
            };
            *count -= 1;
            if *count > 0 {
                continue;
            }
            self.users.remove(&used);
            if let Some(gone) = self.gone.remove(&used) {
                self.free(gone);
            }
        }
    }

    /// Takes `held` off the counts, and lets go of what it uses.
    fn free(&mut self, held: Held) {
        let counts = &mut self.counts;
        match held {
            Held::Entity { light, .. } => {
                counts.entities -= 1;
                counts.lights -= usize::from(light);
            }
            Held::Mesh { vertices, bytes } => {
                counts.vertices -= vertices;
                counts.gpu_bytes -= bytes;
            }
            Held::Material { .. } => counts.materials -= 1,
            Held::Instances { copies, .. } => {
                counts.entities -= copies + 1;
            }
            Held::Camera { pass, .. } => {
                counts.cameras -= 1;
                counts.gpu_objects -= usize::from(pass.is_some());
            }
            Held::Shader(_) | Held::Compute { .. } => {
                counts.gpu_objects -= 1
            }
            Held::Buffer { bytes } => {
                counts.gpu_objects -= 1;
                counts.gpu_bytes -= bytes;
            }
            Held::Texture { width, height } => {
                counts.gpu_objects -= 1;
                counts.gpu_bytes -=
                    u64::from(width) * u64::from(height) * 4;
            }
        }
        self.release(held.uses());
    }

    /// The thing of the number `id`, which `is` says is of the kind
    /// wanted.
    fn held(
        &self,
        id: u32,
        is: fn(&Held) -> bool,
    ) -> wasmtime::Result<Held> {
        self.held
            .get(&id)
            .copied()
            .filter(is)
            .ok_or_else(|| trap(SceneError::Unknown))
    }

    fn change(
        &mut self,
        change: SceneChange,
    ) -> wasmtime::Result<()> {
        self.spend(1)?;
        self.changes.push(change);
        Ok(())
    }

    /// Lets go of the number `id`, and tells the scene. What is still
    /// drawn or run with stays counted until nothing uses it, as the
    /// scene keeps it until then.
    fn forget(&mut self, id: u32) -> wasmtime::Result<()> {
        let Some(held) = self.held.remove(&id) else {
            return Ok(());
        };
        if self.users.contains_key(&id) {
            self.gone.insert(id, held);
        } else {
            self.free(held);
        }
        // A drop is not refused, so it is not held to the call's cap.
        self.changes.push(SceneChange::Drop(id));
        Ok(())
    }

    /// Whether `count` more entities fit; why not otherwise.
    fn room(&self, count: usize) -> Result<(), String> {
        let most = self.caps.entities;
        if self.counts.entities.saturating_add(count) > most {
            return Err(format!(
                "a scene holds {most} entities at most"
            ));
        }
        Ok(())
    }

    /// A mesh of `vertices` taking `bytes`, numbered, or why it does
    /// not fit.
    fn fit_mesh(
        &mut self,
        vertices: usize,
        bytes: u64,
    ) -> Result<(), String> {
        let largest = self.gpu.buffer_size;
        if bytes > largest {
            return Err(format!("a mesh is {largest} bytes at most"));
        }
        let most = self.caps.vertices;
        if self.counts.vertices.saturating_add(vertices) > most {
            return Err(format!(
                "meshes hold {most} vertices at most"
            ));
        }
        let memory = self.caps.gpu_memory;
        if self.counts.gpu_bytes.saturating_add(bytes) > memory {
            return Err(format!(
                "a plugin's GPU memory is {memory} bytes at most"
            ));
        }
        self.counts.vertices += vertices;
        self.counts.gpu_bytes += bytes;
        Ok(())
    }

    /// Whether the entity `id` is a light.
    fn light(&self, id: u32) -> wasmtime::Result<bool> {
        match self.held(id, entity)? {
            Held::Entity { light, .. } => Ok(light),
            _ => Err(trap(SceneError::Unknown)),
        }
    }

    /// The scene's number of `parent`, if it is an entity of the
    /// plugin's.
    fn parent(
        &self,
        parent: Option<&Resource<EntityHandle>>,
    ) -> wasmtime::Result<Option<u32>> {
        parent
            .map(|parent| {
                self.held(parent.rep(), |held| {
                    matches!(held, Held::Entity { .. })
                })
                .map(|_| parent.rep())
            })
            .transpose()
    }
}

/// Whether `id` names an entity.
fn entity(held: &Held) -> bool {
    matches!(held, Held::Entity { .. })
}

impl Host for State {
    fn animate(&mut self, on: bool) -> wasmtime::Result<()> {
        self.scene.animate = on;
        Ok(())
    }
}

impl HostMesh for State {
    fn shape(
        &mut self,
        shape: Shape,
    ) -> wasmtime::Result<Result<Resource<MeshHandle>, String>> {
        self.make_mesh(|| convert::shape(shape))
    }

    fn build(
        &mut self,
        data: MeshData,
    ) -> wasmtime::Result<Result<Resource<MeshHandle>, String>> {
        self.keep_bytes(data.bytes())?;
        self.make_mesh(|| convert::mesh(data))
    }

    fn update(
        &mut self,
        mesh: Resource<MeshHandle>,
        data: MeshData,
    ) -> wasmtime::Result<Result<(), String>> {
        self.keep_bytes(data.bytes())?;
        let id = mesh.rep();
        let Held::Mesh { vertices, bytes } =
            self.scene.held(id, Held::mesh)?
        else {
            return Err(trap(SceneError::Unknown));
        };
        let made = match convert::mesh(data) {
            Ok(made) => made,
            Err(why) => return Ok(Err(why)),
        };
        self.scene.spend(1)?;
        let scene = &mut self.scene;
        scene.counts.vertices -= vertices;
        scene.counts.gpu_bytes -= bytes;
        if let Err(why) = scene.fit_mesh(made.vertices, made.bytes) {
            scene.counts.vertices += vertices;
            scene.counts.gpu_bytes += bytes;
            return Ok(Err(why));
        }
        scene.keep(
            id,
            Held::Mesh {
                vertices: made.vertices,
                bytes: made.bytes,
            },
        );
        scene.changes.push(SceneChange::Mesh {
            id,
            mesh: made.mesh,
        });
        Ok(Ok(()))
    }

    fn drop(
        &mut self,
        mesh: Resource<MeshHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(mesh.rep())
    }
}

impl State {
    /// Numbers the mesh `make` makes, within the budgets.
    fn make_mesh(
        &mut self,
        make: impl FnOnce() -> Result<convert::Made, String>,
    ) -> wasmtime::Result<Result<Resource<MeshHandle>, String>> {
        if let Err(why) =
            self.scene.allowed(Powers::SCENE, "render.scene")
        {
            return Ok(Err(why));
        }
        let made = match make() {
            Ok(made) => made,
            Err(why) => return Ok(Err(why)),
        };
        self.scene.spend(1)?;
        if let Err(why) =
            self.scene.fit_mesh(made.vertices, made.bytes)
        {
            return Ok(Err(why));
        }
        let id = self.scene.number(Held::Mesh {
            vertices: made.vertices,
            bytes: made.bytes,
        });
        self.scene.changes.push(SceneChange::Mesh {
            id,
            mesh: made.mesh,
        });
        Ok(Ok(Resource::new_own(id)))
    }
}

impl HostMaterial for State {
    fn make(
        &mut self,
        pbr: Pbr,
    ) -> wasmtime::Result<Result<Resource<MaterialHandle>, String>>
    {
        if let Err(why) =
            self.scene.allowed(Powers::SCENE, "render.scene")
        {
            return Ok(Err(why));
        }
        let most = self.scene.caps.materials;
        if self.scene.counts.materials >= most {
            return Ok(Err(format!(
                "a scene holds {most} materials at most"
            )));
        }
        let material = convert::pbr(pbr)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.spend(1)?;
        self.scene.counts.materials += 1;
        let id = self.scene.number(Held::Material {
            shaded: false,
            uses: [None; 3],
        });
        self.scene
            .changes
            .push(SceneChange::Material { id, material });
        Ok(Ok(Resource::new_own(id)))
    }

    fn set(
        &mut self,
        material: Resource<MaterialHandle>,
        pbr: Pbr,
    ) -> wasmtime::Result<()> {
        let id = material.rep();
        self.scene.held(id, |held| {
            matches!(held, Held::Material { shaded: false, .. })
        })?;
        let material = convert::pbr(pbr)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.change(SceneChange::Material { id, material })
    }

    fn drop(
        &mut self,
        material: Resource<MaterialHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(material.rep())
    }
}

impl HostEntity for State {
    fn make(
        &mut self,
        parent: Option<Resource<EntityHandle>>,
        key: u32,
    ) -> wasmtime::Result<Result<Resource<EntityHandle>, String>>
    {
        let parent = self.scene.parent(parent.as_ref())?;
        if let Err(why) = self
            .scene
            .allowed(Powers::SCENE, "render.scene")
            .and_then(|()| self.scene.room(1))
        {
            return Ok(Err(why));
        }
        self.scene.spend(1)?;
        self.scene.counts.entities += 1;
        let id = self.scene.number(Held::Entity {
            light: false,
            draws: None,
        });
        self.scene.changes.push(SceneChange::Entity {
            id,
            key,
            parent,
        });
        Ok(Ok(Resource::new_own(id)))
    }

    fn set_transform(
        &mut self,
        at: Resource<EntityHandle>,
        transform: Transform,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        let transform = convert::transform(transform)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.change(SceneChange::Transform { id, transform })
    }

    fn set_visible(
        &mut self,
        at: Resource<EntityHandle>,
        visible: bool,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        self.scene.change(SceneChange::Visible { id, visible })
    }

    fn set_parent(
        &mut self,
        at: Resource<EntityHandle>,
        parent: Option<Resource<EntityHandle>>,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        let parent = self.scene.parent(parent.as_ref())?;
        self.scene.change(SceneChange::Parent { id, parent })
    }

    fn draw(
        &mut self,
        at: Resource<EntityHandle>,
        mesh: Resource<MeshHandle>,
        material: Resource<MaterialHandle>,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        let light = self.scene.light(id)?;
        let (mesh, material) = (mesh.rep(), material.rep());
        self.scene.held(mesh, Held::mesh)?;
        self.scene.held(material, Held::material)?;
        self.scene.change(SceneChange::Draw {
            id,
            mesh,
            material,
        })?;
        self.scene.keep(
            id,
            Held::Entity {
                light,
                draws: Some((mesh, material)),
            },
        );
        Ok(())
    }

    fn undraw(
        &mut self,
        at: Resource<EntityHandle>,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        let light = self.scene.light(id)?;
        self.scene.change(SceneChange::Undraw(id))?;
        self.scene.keep(id, Held::Entity { light, draws: None });
        Ok(())
    }

    fn set_light(
        &mut self,
        at: Resource<EntityHandle>,
        light: Option<Light>,
    ) -> wasmtime::Result<Result<(), String>> {
        let id = at.rep();
        let Held::Entity { light: was, draws } =
            self.scene.held(id, entity)?
        else {
            return Err(trap(SceneError::Unknown));
        };
        let now = light.is_some();
        let most = self.scene.caps.lights;
        if now && !was && self.scene.counts.lights >= most {
            return Ok(Err(format!(
                "a scene holds {most} lights at most"
            )));
        }
        let light = light
            .map(|light| {
                convert::light(light)
                    .ok_or_else(|| trap(SceneError::Invalid))
            })
            .transpose()?;
        self.scene.spend(1)?;
        let counts = &mut self.scene.counts;
        counts.lights =
            counts.lights + usize::from(now) - usize::from(was);
        self.scene.keep(id, Held::Entity { light: now, draws });
        self.scene.changes.push(SceneChange::Light { id, light });
        Ok(Ok(()))
    }

    fn set_shadows(
        &mut self,
        at: Resource<EntityHandle>,
        cast: bool,
        receive: bool,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        self.scene
            .change(SceneChange::Shadows { id, cast, receive })
    }

    fn set_label(
        &mut self,
        at: Resource<EntityHandle>,
        text: Option<String>,
    ) -> wasmtime::Result<()> {
        self.keep_bytes(text.as_ref().map_or(0, String::len))?;
        let id = at.rep();
        self.scene.held(id, entity)?;
        let text = text.map(|mut text| {
            text.truncate(
                text.floor_char_boundary(self.caps.line_bytes),
            );
            text
        });
        self.scene.change(SceneChange::Label { id, text })
    }

    fn watch(
        &mut self,
        at: Resource<EntityHandle>,
        hover: bool,
        click: bool,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        self.scene.change(SceneChange::Watch { id, hover, click })
    }

    fn drop(
        &mut self,
        at: Resource<EntityHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(at.rep())
    }
}

impl HostInstances for State {
    fn make(
        &mut self,
        parent: Option<Resource<EntityHandle>>,
        mesh: Resource<MeshHandle>,
        material: Resource<MaterialHandle>,
        key: u32,
    ) -> wasmtime::Result<Result<Resource<InstancesHandle>, String>>
    {
        let parent = self.scene.parent(parent.as_ref())?;
        let (mesh, material) = (mesh.rep(), material.rep());
        self.scene.held(mesh, Held::mesh)?;
        self.scene.held(material, Held::material)?;
        if let Err(why) = self
            .scene
            .allowed(Powers::SCENE, "render.scene")
            .and_then(|()| self.scene.room(1))
        {
            return Ok(Err(why));
        }
        self.scene.spend(1)?;
        self.scene.counts.entities += 1;
        let id = self.scene.number(Held::Instances {
            copies: 0,
            mesh,
            material,
        });
        self.scene.changes.push(SceneChange::Instances {
            id,
            key,
            parent,
            mesh,
            material,
        });
        Ok(Ok(Resource::new_own(id)))
    }

    fn set(
        &mut self,
        instances: Resource<InstancesHandle>,
        transforms: Vec<f32>,
    ) -> wasmtime::Result<Result<(), String>> {
        self.keep_bytes(size_of_val(transforms.as_slice()))?;
        let id = instances.rep();
        let Held::Instances {
            copies,
            mesh,
            material,
        } = self.scene.held(id, |held| {
            matches!(held, Held::Instances { .. })
        })?
        else {
            return Err(trap(SceneError::Unknown));
        };
        if !transforms.len().is_multiple_of(COPY) {
            return Ok(Err(format!(
                "{} floats are not ten for each copy",
                transforms.len()
            )));
        }
        let count = transforms.len() / COPY;
        let others = self.scene.counts.entities - copies;
        let most = self.scene.caps.entities;
        if others.saturating_add(count) > most {
            return Ok(Err(format!(
                "a scene holds {most} entities at most"
            )));
        }
        let placed = transforms
            .as_chunks::<COPY>()
            .0
            .iter()
            .map(|floats| convert::copy(floats))
            .collect::<Option<Vec<Placed>>>()
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.spend(count.max(1))?;
        self.scene.counts.entities = others + count;
        self.scene.keep(
            id,
            Held::Instances {
                copies: count,
                mesh,
                material,
            },
        );
        self.scene.changes.push(SceneChange::Copies {
            id,
            transforms: placed,
        });
        Ok(Ok(()))
    }

    fn watch(
        &mut self,
        instances: Resource<InstancesHandle>,
        hover: bool,
        click: bool,
    ) -> wasmtime::Result<()> {
        let id = instances.rep();
        self.scene.held(id, |held| {
            matches!(held, Held::Instances { .. })
        })?;
        self.scene.change(SceneChange::Watch { id, hover, click })
    }

    fn drop(
        &mut self,
        instances: Resource<InstancesHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(instances.rep())
    }
}

impl HostCamera for State {
    fn make(
        &mut self,
        at: Resource<EntityHandle>,
        settings: CameraSettings,
        target: Target,
    ) -> wasmtime::Result<Result<Resource<CameraHandle>, String>>
    {
        self.scene.held(at.rep(), entity)?;
        let background = target == Target::Background;
        let setup = convert::camera(settings)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        let most = self.scene.caps.cameras;
        let refused = self
            .scene
            .allowed(Powers::SCENE, "render.scene")
            .err()
            .or_else(|| {
                (self.scene.counts.cameras >= most).then(|| {
                    format!("a scene holds {most} cameras at most")
                })
            })
            .or_else(|| {
                let behind = self.scene.held.values().any(|held| {
                    matches!(
                        held,
                        Held::Camera {
                            background: true,
                            ..
                        }
                    )
                });
                (background && (behind || self.scene.backdrop_taken))
                    .then(|| {
                        "another camera draws behind the dock"
                            .to_owned()
                    })
            });
        if let Some(why) = refused {
            return Ok(Err(why));
        }
        self.scene.spend(1)?;
        self.scene.counts.cameras += 1;
        let id = self.scene.number(Held::Camera {
            background,
            pass: None,
        });
        self.scene.changes.push(SceneChange::Camera {
            id,
            at: at.rep(),
            setup,
            background,
        });
        Ok(Ok(Resource::new_own(id)))
    }

    fn set(
        &mut self,
        camera: Resource<CameraHandle>,
        settings: CameraSettings,
    ) -> wasmtime::Result<()> {
        let id = camera.rep();
        self.scene
            .held(id, |held| matches!(held, Held::Camera { .. }))?;
        let setup = convert::camera(settings)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.change(SceneChange::Setup { id, setup })
    }

    fn show_in(
        &mut self,
        camera: Resource<CameraHandle>,
        frame: Resource<UiNode>,
    ) -> wasmtime::Result<()> {
        let id = camera.rep();
        self.scene.held(id, |held| {
            matches!(
                held,
                Held::Camera {
                    background: false,
                    ..
                }
            )
        })?;
        self.nodes.frame(frame.rep())?;
        self.scene.spend(1)?;
        self.scene.shown.push((id, frame.rep()));
        Ok(())
    }

    fn drop(
        &mut self,
        camera: Resource<CameraHandle>,
    ) -> wasmtime::Result<()> {
        self.scene.forget(camera.rep())
    }
}
