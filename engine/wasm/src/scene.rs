//! A plugin's scenes, as it makes, changes and drops what is in them
//! in a call: each held to its capabilities and the limits the person
//! switched on, and kept
//! as changes for the render crate to make once the call ends.

mod convert;
mod effects;
mod gpu;
mod serve;

#[cfg(test)]
mod tests;

use core::fmt;
use std::collections::HashMap;
use std::time::Instant;

use bevy::pbr::StandardMaterial;
use bevy::transform::components::Transform as Placed;
use hamlet_engine_api::{Capability, PluginLimits};
use hamlet_engine_render::{GpuInfo, SceneChange, ShaderKind};
use wasmtime::component::Resource;

use crate::instance::State;
use crate::nodes::UiNode;
pub use crate::scene::serve::ShowsCamera;
pub(crate) use crate::scene::serve::{apply as apply_scenes, plugin};

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

use crate::scene::scene_bindings::hamlet::engine::scene::{
    Balloon as WitBalloon, CameraEffects, CameraSettings, Decal,
    FogVolume, Host, HostCamera, HostEntity, HostInstances,
    HostMaterial, HostMesh, Label, Light, MeshData,
    PointerLabel as WitPointerLabel, Shape, StandardMaterial as Wit,
    Target, Transform,
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

/// What a plugin may hold in its scenes, when the person switched it
/// on.
#[derive(Clone, Copy, Debug)]
pub struct SceneCaps {
    /// Entities, each copy of instances counted.
    pub entities: Option<usize>,
    /// Bytes of meshes, buffers and textures.
    pub gpu_memory: Option<u64>,
}

impl SceneCaps {
    pub fn of(limits: &PluginLimits) -> Self {
        Self {
            entities: limits.scene_entities.map(|entities| {
                usize::try_from(entities).unwrap_or(usize::MAX)
            }),
            gpu_memory: limits.gpu_memory,
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
        /// The mesh, none for a decal, and the material it draws.
        draws: Option<(Option<u32>, u32)>,
    },
    Mesh {
        bytes: u64,
    },
    /// A material, with the shader, texture and buffer it uses.
    Material {
        kind: MaterialKind,
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

/// What draws a material.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MaterialKind {
    Pbr,
    /// A plugin's shader.
    Shaded,
    Decal,
    /// A decal of a plugin's shader.
    ShadedDecal,
}

impl Held {
    /// Whether it is a material a mesh is drawn with.
    fn material(&self) -> bool {
        matches!(
            self,
            Self::Material {
                kind: MaterialKind::Pbr | MaterialKind::Shaded,
                ..
            }
        )
    }

    /// Whether it is a material a decal is laid with.
    fn decal(&self) -> bool {
        matches!(
            self,
            Self::Material {
                kind: MaterialKind::Decal | MaterialKind::ShadedDecal,
                ..
            }
        )
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
            } => [mesh, Some(material), None],
            Self::Instances { mesh, material, .. } => {
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

/// What a plugin holds in its scenes, counted against the limits the
/// person may switch on.
#[derive(Clone, Copy, Default, Debug)]
struct Counts {
    entities: usize,
    gpu_bytes: u64,
}

/// The scenes of a plugin, as its host holds them.
pub(crate) struct SceneState {
    /// When the numbering began; the render crate's scene of another
    /// numbering is taken away.
    pub(crate) since: Instant,
    next: u32,
    held: HashMap<u32, Held>,
    /// What the plugin let go of while something still draws or runs
    /// with it, counted until nothing does.
    gone: HashMap<u32, Held>,
    /// How many things use each number.
    users: HashMap<u32, usize>,
    counts: Counts,
    pub(crate) caps: SceneCaps,
    pub(crate) changes: Vec<SceneChange>,
    /// Cameras to show in frames, by their numbers.
    pub(crate) shown: Vec<(u32, u32)>,
    /// Cards hung over entities, or taken down.
    pub(crate) hangs: Vec<Hang>,
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

/// A plugin misused its scenes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SceneError {
    /// A number it never held, or one of another kind.
    Unknown,
    /// A number that is not finite, or out of its range.
    Invalid,
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            hangs: Vec::new(),
            animate: false,
            powers: Powers::default(),
            asking: Powers::default(),
            wanted: Powers::default(),
            backdrop_taken: false,
            gpu: GpuInfo::default(),
        }
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
            Held::Entity { .. } => counts.entities -= 1,
            Held::Mesh { bytes } | Held::Buffer { bytes } => {
                counts.gpu_bytes -= bytes;
            }
            Held::Instances { copies, .. } => {
                counts.entities -= copies + 1;
            }
            Held::Texture { width, height } => {
                counts.gpu_bytes -=
                    u64::from(width) * u64::from(height) * 4;
            }
            Held::Material { .. }
            | Held::Camera { .. }
            | Held::Shader(_)
            | Held::Compute { .. } => {}
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
        self.changes.push(SceneChange::Drop(id));
        Ok(())
    }

    /// Whether `count` more entities fit within the person's limit;
    /// why not otherwise.
    fn room(&self, count: usize) -> Result<(), String> {
        match self.caps.entities {
            Some(most)
                if self.counts.entities.saturating_add(count)
                    > most =>
            {
                Err(format!(
                    "its scenes hold {most} entities at most, as \
                     you set"
                ))
            }
            _ => Ok(()),
        }
    }

    /// Whether `bytes` more of GPU memory fit within the person's
    /// limit; why not otherwise.
    fn room_gpu(&self, bytes: u64) -> Result<(), String> {
        match self.caps.gpu_memory {
            Some(most)
                if self.counts.gpu_bytes.saturating_add(bytes)
                    > most =>
            {
                Err(format!(
                    "its GPU memory is {most} bytes at most, as you \
                     set"
                ))
            }
            _ => Ok(()),
        }
    }

    /// Numbers a decal's material, `made` with `texture`, if the
    /// plugin may make it; why not otherwise.
    fn make_decal(
        &mut self,
        (material, depth_fade): (StandardMaterial, f32),
        texture: Option<u32>,
    ) -> wasmtime::Result<Result<Resource<MaterialHandle>, String>>
    {
        if let Err(why) = self.allowed(Powers::SCENE, "render.scene")
        {
            return Ok(Err(why));
        }
        let id = self.number(Held::Material {
            kind: MaterialKind::Decal,
            uses: [None, texture, None],
        });
        self.changes.push(SceneChange::Decal {
            id,
            material,
            depth_fade,
            texture,
        });
        Ok(Ok(Resource::new_own(id)))
    }

    /// Counts a mesh of `bytes`, or why it does not fit: past the
    /// device's largest buffer, or the person's GPU memory.
    fn fit_mesh(&mut self, bytes: u64) -> Result<(), String> {
        let largest = self.gpu.buffer_size;
        if bytes > largest {
            return Err(format!("a mesh is {largest} bytes at most"));
        }
        self.room_gpu(bytes)?;
        self.counts.gpu_bytes =
            self.counts.gpu_bytes.saturating_add(bytes);
        Ok(())
    }

    /// Whether `id` names an entity it holds.
    pub(crate) fn holds_entity(&self, id: u32) -> bool {
        self.held.get(&id).is_some_and(entity)
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

    fn set_pointer_label(
        &mut self,
        label: Option<WitPointerLabel>,
    ) -> wasmtime::Result<()> {
        let label = label
            .map(|label| {
                convert::pointer_label(label)
                    .ok_or_else(|| trap(SceneError::Invalid))
            })
            .transpose()?;
        self.scene.change(SceneChange::PointerLabel(label))
    }

    fn shadow_map(
        &mut self,
        size: u32,
    ) -> wasmtime::Result<Result<(), String>> {
        let most = self.scene.gpu.texture_size;
        if !size.is_power_of_two() || size > most {
            return Ok(Err(format!(
                "a shadow map's side is a power of two, {most} at \
                 most"
            )));
        }
        self.scene.change(SceneChange::ShadowMap(size as usize))?;
        Ok(Ok(()))
    }
}

/// A card hung over the entity `id`, or taken down: what a call
/// asked, made once it ends.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hang {
    pub(crate) id: u32,
    /// The node of the card.
    pub(crate) card: Option<u32>,
    pub(crate) gap: f32,
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
        self.make_mesh(|| convert::mesh(data))
    }

    fn update(
        &mut self,
        mesh: Resource<MeshHandle>,
        data: MeshData,
    ) -> wasmtime::Result<Result<(), String>> {
        let id = mesh.rep();
        let Held::Mesh { bytes } = self.scene.held(id, Held::mesh)?
        else {
            return Err(trap(SceneError::Unknown));
        };
        let made = match convert::mesh(data) {
            Ok(made) => made,
            Err(why) => return Ok(Err(why)),
        };
        let scene = &mut self.scene;
        scene.counts.gpu_bytes -= bytes;
        if let Err(why) = scene.fit_mesh(made.bytes) {
            scene.counts.gpu_bytes += bytes;
            return Ok(Err(why));
        }
        scene.keep(id, Held::Mesh { bytes: made.bytes });
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
    /// Numbers the mesh `make` makes, if it fits.
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
        if let Err(why) = self.scene.fit_mesh(made.bytes) {
            return Ok(Err(why));
        }
        let id = self.scene.number(Held::Mesh { bytes: made.bytes });
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
        standard_material: Wit,
    ) -> wasmtime::Result<Result<Resource<MaterialHandle>, String>>
    {
        if let Err(why) =
            self.scene.allowed(Powers::SCENE, "render.scene")
        {
            return Ok(Err(why));
        }
        let material = convert::standard_material(standard_material)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        let id = self.scene.number(Held::Material {
            kind: MaterialKind::Pbr,
            uses: [None; 3],
        });
        self.scene
            .changes
            .push(SceneChange::Material { id, material });
        Ok(Ok(Resource::new_own(id)))
    }

    fn decal(
        &mut self,
        decal: Decal,
    ) -> wasmtime::Result<Result<Resource<MaterialHandle>, String>>
    {
        let made = convert::decal(decal)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.make_decal(made, None)
    }

    fn set(
        &mut self,
        material: Resource<MaterialHandle>,
        standard_material: Wit,
    ) -> wasmtime::Result<()> {
        let id = material.rep();
        self.scene.held(id, |held| {
            matches!(
                held,
                Held::Material {
                    kind: MaterialKind::Pbr,
                    ..
                }
            )
        })?;
        let material = convert::standard_material(standard_material)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.change(SceneChange::Material { id, material })
    }

    fn set_decal(
        &mut self,
        material: Resource<MaterialHandle>,
        decal: Decal,
    ) -> wasmtime::Result<()> {
        let id = material.rep();
        self.scene.held(id, |held| {
            matches!(
                held,
                Held::Material {
                    kind: MaterialKind::Decal,
                    ..
                }
            )
        })?;
        let (material, depth_fade) = convert::decal(decal)
            .ok_or_else(|| trap(SceneError::Invalid))?;
        self.scene.change(SceneChange::Decal {
            id,
            material,
            depth_fade,
            texture: None,
        })
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
        self.scene.counts.entities += 1;
        let id = self.scene.number(Held::Entity { draws: None });
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
        self.scene.held(id, entity)?;
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
                draws: Some((Some(mesh), material)),
            },
        );
        Ok(())
    }

    fn draw_decal(
        &mut self,
        at: Resource<EntityHandle>,
        material: Resource<MaterialHandle>,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        let material = material.rep();
        self.scene.held(material, Held::decal)?;
        self.scene.change(SceneChange::DrawDecal { id, material })?;
        self.scene.keep(
            id,
            Held::Entity {
                draws: Some((None, material)),
            },
        );
        Ok(())
    }

    fn undraw(
        &mut self,
        at: Resource<EntityHandle>,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        self.scene.change(SceneChange::Undraw(id))?;
        self.scene.keep(id, Held::Entity { draws: None });
        Ok(())
    }

    fn set_light(
        &mut self,
        at: Resource<EntityHandle>,
        light: Option<Light>,
    ) -> wasmtime::Result<Result<(), String>> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        let light = light
            .map(|light| {
                convert::light(light)
                    .ok_or_else(|| trap(SceneError::Invalid))
            })
            .transpose()?;
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

    fn set_volumetric(
        &mut self,
        at: Resource<EntityHandle>,
        on: bool,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        self.scene.change(SceneChange::Volumetric { id, on })
    }

    fn set_fog_volume(
        &mut self,
        at: Resource<EntityHandle>,
        volume: Option<FogVolume>,
    ) -> wasmtime::Result<Result<(), String>> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        let volume = volume
            .map(|volume| {
                effects::fog_volume(volume)
                    .ok_or_else(|| trap(SceneError::Invalid))
            })
            .transpose()?;
        if let Err(why) =
            self.scene.allowed(Powers::SCENE, "render.scene")
        {
            return Ok(Err(why));
        }
        self.scene.change(SceneChange::FogVolume { id, volume })?;
        Ok(Ok(()))
    }

    fn set_label(
        &mut self,
        at: Resource<EntityHandle>,
        label: Option<Label>,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        let label = label
            .map(|label| {
                convert::label(label)
                    .ok_or_else(|| trap(SceneError::Invalid))
            })
            .transpose()?;
        self.scene.change(SceneChange::Label { id, label })
    }

    fn set_balloon(
        &mut self,
        at: Resource<EntityHandle>,
        balloon: Option<WitBalloon>,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        let balloon = balloon
            .map(|balloon| {
                convert::balloon(balloon)
                    .ok_or_else(|| trap(SceneError::Invalid))
            })
            .transpose()?;
        self.scene.change(SceneChange::Balloon { id, balloon })
    }

    fn hang_card(
        &mut self,
        at: Resource<EntityHandle>,
        card: Option<Resource<UiNode>>,
        gap: f32,
    ) -> wasmtime::Result<()> {
        let id = at.rep();
        self.scene.held(id, entity)?;
        if !gap.is_finite() {
            return Err(trap(SceneError::Invalid));
        }
        let card = card.map(|card| card.rep());
        if let Some(card) = card {
            self.nodes.card(card)?;
        }
        self.scene.hangs.push(Hang { id, card, gap });
        Ok(())
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
        if let Some(most) = self.scene.caps.entities
            && others.saturating_add(count) > most
        {
            return Ok(Err(format!(
                "its scenes hold {most} entities at most, as you set"
            )));
        }
        let placed = transforms
            .as_chunks::<COPY>()
            .0
            .iter()
            .map(|floats| convert::copy(floats))
            .collect::<Option<Vec<Placed>>>()
            .ok_or_else(|| trap(SceneError::Invalid))?;
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
        let refused = self
            .scene
            .allowed(Powers::SCENE, "render.scene")
            .err()
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

    fn set_effects(
        &mut self,
        camera: Resource<CameraHandle>,
        effects: CameraEffects,
    ) -> wasmtime::Result<Result<(), String>> {
        let id = camera.rep();
        self.scene
            .held(id, |held| matches!(held, Held::Camera { .. }))?;
        let effects = match effects::effects(effects)
            .ok_or_else(|| trap(SceneError::Invalid))?
        {
            Ok(effects) => effects,
            Err(why) => return Ok(Err(why)),
        };
        self.scene.change(SceneChange::Effects { id, effects })?;
        Ok(Ok(()))
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
