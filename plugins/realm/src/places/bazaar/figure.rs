//! A figure of the bazaar on the rig of a chief's mascot: its parts
//! made from a look, what it wears over them, and where it stands.

use glam::{Quat, Vec3};
use hamlet_engine_sdk::scene::{Entity, Material, Mesh};

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::folk::chief::look::Look;
use crate::folk::chief::mascot::{Pose, Rig, parts};
use crate::world::palette::Palette;

/// Where a mesh a figure wears hangs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum On {
    Head,
    /// In its hand, drawn in matte.
    Hand,
    /// In its hand, glowing.
    Glowing,
}

/// A figure, dressed once its meshes are handed over.
pub struct Figure {
    pub rig: Rig,
    pub look: Look,
    /// Its parts, in the order of `Parts::each`, as they come.
    parts: [Option<Mesh>; 8],
    /// What it wears, kept so that they stay.
    worn: Vec<(Entity, Mesh)>,
    dressed: bool,
    /// The frame it was last stood at.
    stood: Option<Frame>,
}

impl Figure {
    /// A figure in `look` under `parent`, answering the pointer as
    /// `key`, or not at all when `key` is 0.
    pub fn make(
        parent: &Entity,
        key: u32,
        look: Look,
    ) -> Result<Self, String> {
        let rig = Rig::make(key)?;
        rig.root.set_parent(Some(parent));
        if key == 0 {
            for (part, _) in rig.dressed() {
                part.watch(false, false);
            }
        }
        Ok(Self {
            rig,
            look,
            parts: Default::default(),
            worn: Vec::new(),
            dressed: false,
            stood: None,
        })
    }

    /// The meshes of its parts, in the order of `Parts::each`.
    pub fn meshes(&self, pal: &Palette) -> [Mb; 8] {
        parts(pal, self.look).each()
    }

    /// Takes part `i` of its meshes.
    pub fn part(&mut self, i: usize, mesh: Mesh) {
        if let Some(part) = self.parts.get_mut(i) {
            *part = Some(mesh);
        }
    }

    /// Wears `mesh` `on` it, drawn in `matte` or `gleam`.
    pub fn wear(
        &mut self,
        on: On,
        mesh: Mesh,
        (matte, gleam): (&Material, &Material),
    ) {
        let (under, look) = match on {
            On::Head => (&self.rig.head, matte),
            On::Hand => (&self.rig.item, matte),
            On::Glowing => (&self.rig.item, gleam),
        };
        let Ok(entity) = Entity::make(Some(under), 0) else {
            return;
        };
        if on == On::Glowing {
            entity.set_shadows(false, false);
        }
        entity.draw(&mesh, look);
        self.worn.push((entity, mesh));
    }

    /// Dresses it once every part is there, its wick of `wick` in
    /// `flame`.
    pub fn dress(
        &mut self,
        matte: &Material,
        (wick, flame): (&Mesh, &Material),
    ) {
        if self.dressed {
            return;
        }
        let parts = self.parts.iter().flatten().collect::<Vec<_>>();
        if parts.len() == self.parts.len() {
            self.rig.dress(&parts, matte, (wick, flame));
            self.dressed = true;
        }
    }

    /// Stands it at `at` in its parent's frame, `size` times the size
    /// it is built at, in `pose`.
    pub fn place(&mut self, at: Frame, pose: &Pose, size: f32) {
        if self.stood != Some(at) {
            self.rig.root.set_transform(at.scene());
            self.stood = Some(at);
        }
        self.rig.place(pose, size, self.look.hat.crest());
    }
}

/// The frame of a figure standing at `at` and facing `yaw`, as a
/// rotation about Y from +Z.
pub fn standing(at: Vec3, yaw: f32) -> Frame {
    Frame::at(at).with_turn(Quat::from_rotation_y(yaw))
}
