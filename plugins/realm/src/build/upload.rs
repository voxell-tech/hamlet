//! What a frame may build and hand the engine: the engine stops a
//! plugin that hands over more than its `bytes_per_call`, and a frame
//! builds for no longer than half of one at 60 frames a second, so
//! the view stays smooth. What is left is built in the next frame.
//! [`Draw`] is what its systems change the scene with.

use std::time::{Duration, Instant};

use bevy_ecs::system::SystemParam;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    Buffer, Material, Mesh, MeshData, Pbr,
};

use crate::build::frame::Frame;

/// Bytes of meshes, buffers and copies one call hands over, of the
/// engine's `bytes_per_call`, the rest left for the call's other
/// changes.
const CALL_BYTES: usize = 896 * 1024;

/// The longest a frame builds for.
const FRAME_TIME: Duration = Duration::from_millis(8);

/// What the frame under way may still hand over and build for.
#[derive(Resource)]
pub struct Upload {
    bytes: usize,
    until: Instant,
}

/// A mesh the engine was not handed.
pub enum Unsent {
    /// The call has no room left for it: hand it over in a later
    /// one.
    Full(MeshData),
    /// The engine refused it, saying why.
    Refused(String),
}

impl Default for Upload {
    /// Nothing left, until a frame begins.
    fn default() -> Self {
        Self {
            bytes: 0,
            until: Instant::now(),
        }
    }
}

impl Upload {
    /// What a frame begun now may hand over and build for.
    pub fn frame() -> Self {
        Self {
            bytes: CALL_BYTES,
            until: Instant::now() + FRAME_TIME,
        }
    }

    /// True while the frame has time and bytes left to build with.
    pub fn left(&self) -> bool {
        self.bytes > 0 && Instant::now() < self.until
    }

    /// Takes `bytes` from what the call may hand over. False when it
    /// has no room left for them.
    fn take(&mut self, bytes: usize) -> bool {
        if bytes > self.bytes {
            return false;
        }
        self.bytes -= bytes;
        true
    }
}

/// What the realm's systems change the scene with: its commands, the
/// scene's assets, and what the call may still hand over.
#[derive(SystemParam)]
pub struct Draw<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub assets: ResMut<'w, SceneAssets>,
    pub upload: ResMut<'w, Upload>,
}

impl Draw<'_, '_> {
    /// True while the call has time and bytes left to build with.
    pub fn left(&self) -> bool {
        self.upload.left()
    }

    /// Hands `data` to the engine as a mesh.
    pub fn mesh(
        &mut self,
        data: MeshData,
    ) -> Result<Handle<Mesh>, Unsent> {
        let bytes = 4
            * (data.positions.len()
                + data.normals.len()
                + data.uvs.len()
                + data.colors.len()
                + data.indices.len());
        if !self.upload.take(bytes) {
            // A mesh larger than any call takes is refused, so that
            // it does not wait for ever.
            if bytes > CALL_BYTES {
                return Err(Unsent::Refused(format!(
                    "a mesh of {bytes} bytes is more than a call \
                     hands over"
                )));
            }
            self.upload.bytes = 0;
            return Err(Unsent::Full(data));
        }
        let mesh = Mesh::build(&data).map_err(Unsent::Refused)?;
        Ok(self.assets.add(mesh))
    }

    /// Writes `bytes` to the start of `buffer`. False when the call
    /// has no room left for them.
    pub fn write(
        &mut self,
        buffer: &Handle<Buffer>,
        bytes: &[u8],
    ) -> Result<bool, String> {
        if !self.upload.take(bytes.len()) {
            return Ok(false);
        }
        let buffer =
            self.assets.get(buffer).ok_or("the buffer is gone")?;
        buffer.write(0, bytes).map(|()| true)
    }

    /// Takes the room of `count` copies of instances from what the
    /// call may hand over. False when it has none left for them.
    pub fn copies(&mut self, count: usize) -> bool {
        self.upload.take(count * 10 * 4)
    }

    /// A material of `pbr`.
    pub fn material(
        &mut self,
        pbr: Pbr,
    ) -> Result<Handle<Material>, String> {
        Ok(self.assets.add(Material::make(pbr)?))
    }

    /// Gives `material` the look `pbr`.
    pub fn set_material(
        &self,
        material: &Handle<Material>,
        pbr: Pbr,
    ) {
        if let Some(material) = self.assets.get(material) {
            material.set(pbr);
        }
    }

    /// An entity of the scene under `parent`, or at its root, named
    /// `key` for the engine unless it is 0.
    pub fn spawn(
        &mut self,
        parent: Option<Entity>,
        key: u32,
    ) -> Entity {
        let mut entity = self.commands.spawn(Transform::IDENTITY);
        if key != 0 {
            entity.insert(SceneKey(key));
        }
        if let Some(parent) = parent {
            entity.insert(ChildOf(parent));
        }
        entity.id()
    }

    /// Puts `entity` where `frame` says.
    pub fn place(&mut self, entity: Entity, frame: Frame) {
        self.commands.entity(entity).try_insert(frame.transform());
    }

    /// Shows `entity` and what is under it, or hides them.
    pub fn show(&mut self, entity: Entity, shown: bool) {
        let visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        self.commands.entity(entity).try_insert(visibility);
    }

    /// Has `entity` draw `mesh` in `material`.
    pub fn draw(
        &mut self,
        entity: Entity,
        mesh: &Handle<Mesh>,
        material: &Handle<Material>,
    ) {
        self.commands.entity(entity).try_insert((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
        ));
    }

    /// Has `entity` draw nothing.
    pub fn undraw(&mut self, entity: Entity) {
        self.commands
            .entity(entity)
            .try_remove::<(Mesh3d, MeshMaterial3d)>();
    }

    pub fn shadows(
        &mut self,
        entity: Entity,
        cast: bool,
        receive: bool,
    ) {
        self.commands
            .entity(entity)
            .try_insert(Shadows { cast, receive });
    }

    /// Has the engine tell of the pointer over `entity` and of clicks
    /// on it.
    pub fn watch(&mut self, entity: Entity) {
        self.commands.entity(entity).try_insert(Pickable {
            hover: true,
            click: true,
        });
    }

    /// Takes `entity` away, and what is under it.
    pub fn despawn(&mut self, entity: Entity) {
        self.commands.entity(entity).try_despawn();
    }
}
