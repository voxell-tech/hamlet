//! A dummy: the realm's own robot, or a house, wearing one look read
//! from its file in memory, as the world would draw it, on a round
//! stand.

use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::prelude::*;

use super::{Framing, Subject};
use crate::dress::{self, Closet, Look};
use crate::generator::attire::{Attire, Shell, can_raise, raise};
use crate::keeper::{Apart, Gesture, Kit, Wears, Who, spawn_dressed};
use crate::meshes::Meshing;
use crate::palette::Palette;
use crate::wardrobe::{Category, Emote, Target};
use crate::{Clock, Stock};

/// The robot the dummy is, of those an agent may be.
const ROBOT: u32 = 3;

/// One look worn by the realm's own robot or house.
pub struct Dummy {
    target: Target,
    category: Category,
    /// The name its mesh is put in the closet under.
    name: String,
    /// Its file, until it is read.
    file: Option<Vec<u8>>,
    look: Option<Look>,
    /// The robot, once raised, to play its clip on.
    robot: Option<Entity>,
}

impl Dummy {
    /// A dummy of `target` wearing `file`, a look of `category`,
    /// known as `name`.
    pub fn new(
        target: Target,
        category: Category,
        name: impl Into<String>,
        file: Vec<u8>,
    ) -> Self {
        Dummy {
            target,
            category,
            name: name.into(),
            file: Some(file),
            look: None,
            robot: None,
        }
    }

    /// The look read from the file, once; None when it cannot be.
    fn read(&mut self, world: &World) -> Option<&Look> {
        if let Some(file) = self.file.take() {
            let read = dress::try_on(
                self.target,
                self.category,
                &self.name,
                &file,
                world.get_resource::<Closet>(),
                world.resource::<AssetServer>(),
            );
            match read {
                Ok(look) => self.look = Some(look),
                Err(e) => {
                    warn!(
                        "the look `{}` cannot be shown: {e:#}",
                        self.name
                    );
                }
            }
        }
        self.look.as_ref()
    }
}

impl Subject for Dummy {
    fn raise(&mut self, world: &mut World) -> Option<Entity> {
        let look = self.read(world)?.clone();
        if let Some(gltf) = &look.mesh
            && !world
                .resource::<AssetServer>()
                .is_loaded_with_dependencies(gltf)
        {
            return None;
        }
        let worn = match self.target {
            Target::Agent => {
                let robot = world
                    .run_system_cached_with(
                        |In(wears): In<Wears>,
                         mut commands: Commands,
                         kit: Res<Kit>,
                         mut materials: ResMut<
                            Assets<StandardMaterial>,
                        >| {
                            spawn_dressed(
                                &mut commands,
                                &kit,
                                &mut materials,
                                Who::agent(ROBOT),
                                Transform::IDENTITY,
                                wears,
                            )
                        },
                        Wears::of(&look),
                    )
                    .ok()?;
                world.entity_mut(robot).insert(Apart);
                self.robot = Some(robot);
                robot
            }
            Target::Hamlet => {
                if !can_raise(world) {
                    return None;
                }
                let mesh = look.mesh.as_ref().and_then(|gltf| {
                    Shell::of(
                        gltf,
                        world.resource::<Assets<Gltf>>(),
                        world.resource::<Assets<GltfMesh>>(),
                        world.resource::<Assets<GltfNode>>(),
                        world.resource::<Assets<Mesh>>(),
                    )
                });
                let attire = Attire {
                    skin: look.skin,
                    mesh,
                    emote: look.emote,
                };
                raise(world, &attire, false, Transform::IDENTITY)
            }
        };
        let radius = match self.target {
            Target::Agent => 0.55,
            Target::Hamlet => 2.6,
        };
        let stand =
            stand(world.resource::<Palette>(), radius, radius * 0.12);
        let stand = world.resource_mut::<Assets<Mesh>>().add(stand);
        let matte = world.resource::<Stock>().matte.clone();
        let root = world
            .spawn((Transform::IDENTITY, Visibility::Inherited))
            .id();
        world.spawn((
            Mesh3d(stand),
            MeshMaterial3d(matte),
            ChildOf(root),
        ));
        world.entity_mut(worn).insert(ChildOf(root));
        Some(root)
    }

    fn framing(&self) -> Framing {
        match self.target {
            Target::Agent => Framing {
                middle: Vec3::Y * 0.6,
                distance: 3.2,
            },
            Target::Hamlet => Framing {
                middle: Vec3::Y * 1.1,
                distance: 9.5,
            },
        }
    }

    /// Plays an agent's clip over and over.
    fn tend(&mut self, world: &mut World, _root: Entity) {
        let clip = matches!(
            self.look.as_ref().and_then(|look| look.emote.as_ref()),
            Some(Emote::Clip(_))
        );
        let Some(robot) = self.robot.filter(|_| clip) else {
            return;
        };
        let now = world.resource::<Clock>().t;
        if let Some(mut gesture) = world.get_mut::<Gesture>(robot)
            && !gesture.is_playing()
        {
            gesture.play(now);
        }
    }

    fn release(&mut self, world: &mut World) {
        if self.category == Category::Mesh
            && let Some(closet) = world.get_resource::<Closet>()
        {
            closet.take_down(&self.name);
        }
    }
}

/// A round wooden stand `radius` wide and `high` deep, grassed on
/// top, its top a little under the ground the subject stands on.
fn stand(pal: &Palette, radius: f32, high: f32) -> Mesh {
    const SEGMENTS: usize = 64;
    let top = -0.01;
    let mut mb = Meshing::new();
    mb.lathe(
        Vec3::ZERO,
        &[(radius, top - high), (radius, top)],
        SEGMENTS,
        |_, _| pal.timber_dark,
    );
    mb.lathe(
        Vec3::ZERO,
        &[(radius, top), (0.0, top)],
        SEGMENTS,
        |_, _| pal.grass,
    );
    mb.build()
}
