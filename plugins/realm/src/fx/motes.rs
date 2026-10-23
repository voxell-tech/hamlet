//! Motes of light on the GPU: copies of one small ball, each placed,
//! sized and faded by `motes.wgsl` from its number and the time, so
//! that a cloud costs nothing a frame but its age while it plays.

use glam::Vec3;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    AlphaMode, Material, Mesh, Shaded, Shader, ShaderKind,
};

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::upload::Draw;

/// The way a cloud's motes move, as `motes.wgsl` reads it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Way {
    /// Born in turn over and over, as fireflies are.
    Swarm,
    /// Leaping from the ground at once.
    Burst,
    /// Climbing one after another.
    Rise,
    /// Flying from one place to another.
    Flight,
}

/// What a cloud of motes looks like and how they move.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    /// The colour, as bright as it shines.
    pub colour: Vec3,
    pub way: Way,
    pub count: u32,
    /// The furthest a mote goes from the cloud's middle.
    pub reach: f32,
    /// A number of the cloud's own.
    pub salt: f32,
    /// The way's own params, as `motes.wgsl` lists them.
    pub own: [f32; 12],
    /// Added to what is behind it, or blended over it.
    pub added: bool,
}

impl Look {
    fn params(&self, age: f32) -> [f32; 20] {
        let way = match self.way {
            Way::Swarm => 0.0,
            Way::Burst => 1.0,
            Way::Rise => 2.0,
            Way::Flight => 3.0,
        };
        let mut params = [0.0; 20];
        params[..4]
            .copy_from_slice(&self.colour.extend(way).to_array());
        params[4..8].copy_from_slice(&[
            self.reach.max(1e-3),
            self.count as f32,
            age,
            self.salt,
        ]);
        params[8..].copy_from_slice(&self.own);
        params
    }
}

/// The shader and the ball every mote is drawn with.
#[derive(Resource)]
pub struct Motes {
    shader: Handle<Shader>,
    ball: Handle<Mesh>,
}

/// A cloud of motes in the scene, and its material.
pub struct Cloud {
    pub entity: Entity,
    material: Handle<Material>,
    look: Look,
    age: f32,
}

impl Motes {
    pub fn make(draw: &mut Draw) -> Result<Self, String> {
        let shader = draw.assets.add(Shader::compile(
            ShaderKind::Material,
            include_str!("motes.wgsl"),
        )?);
        let ball = draw.assets.add(Mesh::build(&ball())?);
        Ok(Self { shader, ball })
    }

    /// The ball every mote is drawn with, one unit about its middle.
    pub fn ball(&self) -> &Handle<Mesh> {
        &self.ball
    }

    /// The material of motes that look as `look` says, `age` seconds
    /// into their play.
    pub fn material(
        &self,
        look: &Look,
        age: f32,
        draw: &mut Draw,
    ) -> Result<Handle<Material>, String> {
        draw.assets.add_shaded_material(
            &self.shader,
            Shaded {
                alpha_mode: if look.added {
                    AlphaMode::Add
                } else {
                    AlphaMode::Blend
                },
                double_sided: false,
                lit: false,
            },
            &look.params(age),
            None,
            None,
        )
    }

    /// The motes of `look` about `at`, in `material`, under an entity
    /// of their own.
    pub fn spawn(
        &self,
        material: &Handle<Material>,
        look: &Look,
        at: Vec3,
        draw: &mut Draw,
    ) -> Entity {
        let entity = draw.spawn(None, 0);
        draw.place(entity, Frame::at(at));
        let copy = Transform::from_scale(Vec3::splat(look.reach));
        draw.commands.spawn((
            Instances {
                mesh: self.ball.clone(),
                material: material.clone(),
                copies: vec![copy; look.count as usize],
            },
            Shadows {
                cast: false,
                receive: false,
            },
            ChildOf(entity),
        ));
        entity
    }

    /// A cloud of `look` about `at` that plays once, `age` seconds
    /// into its play.
    pub fn cloud(
        &self,
        look: Look,
        at: Vec3,
        age: f32,
        draw: &mut Draw,
    ) -> Result<Cloud, String> {
        let material = self.material(&look, age, draw)?;
        let entity = self.spawn(&material, &look, at, draw);
        Ok(Cloud {
            entity,
            material,
            look,
            age,
        })
    }
}

impl Cloud {
    /// Plays it on to `age` seconds.
    pub fn age(&mut self, age: f32, draw: &Draw) {
        if self.age == age {
            return;
        }
        self.age = age;
        if let Err(why) = draw
            .assets
            .set_params(&self.material, &self.look.params(age))
        {
            hamlet_engine_bevy::warn(&format!("motes: {why}"));
        }
    }
}

/// A ball one unit about its middle: a twenty-sided one with each
/// face cut in four.
fn ball() -> hamlet_engine_bevy::scene::MeshData {
    let t = (1.0 + 5f32.sqrt()) * 0.5;
    let corners = [
        Vec3::new(-1.0, t, 0.0),
        Vec3::new(1.0, t, 0.0),
        Vec3::new(-1.0, -t, 0.0),
        Vec3::new(1.0, -t, 0.0),
        Vec3::new(0.0, -1.0, t),
        Vec3::new(0.0, 1.0, t),
        Vec3::new(0.0, -1.0, -t),
        Vec3::new(0.0, 1.0, -t),
        Vec3::new(t, 0.0, -1.0),
        Vec3::new(t, 0.0, 1.0),
        Vec3::new(-t, 0.0, -1.0),
        Vec3::new(-t, 0.0, 1.0),
    ]
    .map(Vec3::normalize);
    let faces = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let mut mb = Mb::new();
    let mut tri = |a: Vec3, b: Vec3, c: Vec3| {
        let ids = [a, b, c].map(|p| mb.v(p, p, Vec3::ONE));
        mb.tri(ids[0], ids[1], ids[2]);
    };
    for [a, b, c] in faces.map(|face| face.map(|i| corners[i])) {
        let (ab, bc, ca) = (
            (a + b).normalize(),
            (b + c).normalize(),
            (c + a).normalize(),
        );
        tri(a, ab, ca);
        tri(ab, b, bc);
        tri(ca, bc, c);
        tri(ab, bc, ca);
    }
    mb.data()
}
