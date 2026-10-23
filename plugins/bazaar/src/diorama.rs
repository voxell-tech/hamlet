//! The town of a hamlet template on a plinth, turning at the top of
//! its page: asked of the realm's `diorama` service a part at a time
//! when the page is shown, built in a scene of the bazaar's own, and
//! let go when the page closes.

// Shared with the realm, which writes what is read here.
#[path = "../../realm/src/diorama/wire.rs"]
#[allow(dead_code)]
mod wire;

use std::f32::consts::TAU;

use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    CameraSettings, DirectionalLight, Light, Material, Mesh,
    MeshData, Pbr, Projection, Tonemapping,
};
use hamlet_engine_bevy::{
    Failure, Reply, Ticket, decode, encode, warn,
};

use crate::diorama::wire::{Ask, Part, Piece, Stage, Sun};
use crate::samples::hamlets::Template;
use crate::shelf::Shelf;
use crate::{Stall, StallShown};

/// The realm's service that hands over a template's town.
const SERVICE: &str = "hamlet.realm/diorama";

/// Seconds the town takes to turn once.
const TURN: f32 = 20.0;

/// The camera's height over the plinth, as an angle, and its field of
/// view, in radians.
const TILT: f32 = 0.45;
const FOV: f32 = 0.42;

/// Room left about the plinth in the view, as a share of it.
const ROOM: f32 = 0.06;

/// The shape of the view until the engine tells its size: its width
/// over its height.
const ASPECT: f32 = 2.6;

/// Points about each round of the plinth that the camera keeps in
/// view.
const RIM: usize = 24;

/// What is cleared behind the town: nothing, so the panel shows.
const CLEAR: (f32, f32, f32, f32) = (0.0, 0.0, 0.0, 0.0);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Diorama>()
        .add_systems(Update, (keep, receive, turn).chain());
}

/// The diorama of the template whose page is shown.
#[derive(Resource, Default)]
pub(crate) struct Diorama {
    shown: Option<Shown>,
}

/// What the page shows of its town.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Seen {
    /// The parts are coming.
    Waiting,
    /// No realm is running to build it.
    NoRealm,
    /// It could not be built, saying why.
    Failed(String),
    /// Built, and drawn by this camera.
    Ready(Entity),
}

/// A template's town, as far as it has come.
struct Shown {
    template: String,
    agents: u32,
    /// The part asked for and its ticket.
    asked: (u32, Ticket),
    seen: Seen,
    stage: Option<Built>,
    /// Everything spawned for it.
    root: Option<Entity>,
    /// How far it has turned, and the view's width over its height.
    turned: f32,
    aspect: f32,
}

/// The scene of a town, once its stage has come.
struct Built {
    stage: Stage,
    looks: Vec<Handle<Material>>,
    camera: Entity,
}

impl Diorama {
    /// What the page of `template` shows of its town.
    pub(crate) fn seen(&self, template: &str) -> Seen {
        match &self.shown {
            Some(shown) if shown.template == template => {
                shown.seen.clone()
            }
            _ => Seen::Waiting,
        }
    }
}

/// The diorama's methods on [`World`].
pub(crate) trait DioramaWorldExt {
    /// Asks the realm for the town of `template` unless it is the
    /// one shown, letting go of any other.
    fn show_town(&mut self, template: &Template);

    /// Takes away the town shown and everything of its scene.
    fn let_go(&mut self);
}

impl DioramaWorldExt for World {
    fn show_town(&mut self, template: &Template) {
        let (name, agents) =
            (template.name, template.agents.len() as u32);
        if self
            .resource::<Diorama>()
            .shown
            .as_ref()
            .is_some_and(|shown| shown.template == name)
        {
            return;
        }
        self.let_go();
        let mut shown = Shown {
            template: name.to_owned(),
            agents,
            asked: (0, 0),
            seen: Seen::Waiting,
            stage: None,
            root: None,
            turned: 0.0,
            aspect: ASPECT,
        };
        shown.ask(self, 0);
        self.resource_mut::<Diorama>().shown = Some(shown);
    }

    fn let_go(&mut self) {
        let Some(shown) = self.resource_mut::<Diorama>().shown.take()
        else {
            return;
        };
        let spawned = [
            shown.root,
            shown.stage.as_ref().map(|built| built.camera),
        ];
        for entity in spawned.into_iter().flatten() {
            if let Ok(entity) = self.get_entity_mut(entity) {
                entity.despawn();
            }
        }
        self.resource_mut::<Animate>().0 = false;
    }
}

impl Shown {
    /// Asks the realm for part `part`.
    fn ask(&mut self, world: &mut World, part: u32) {
        let ask = Ask {
            template: self.template.clone(),
            agents: self.agents,
            part,
        };
        let ticket = match encode(&ask) {
            Ok(bytes) => world.call(SERVICE, &bytes),
            Err(why) => {
                self.seen = Seen::Failed(why.to_string());
                return;
            }
        };
        if ticket == 0 {
            self.seen =
                Seen::Failed("the realm could not be asked".into());
        }
        self.asked = (part, ticket);
    }

    /// Builds `part` in the scene, its stage first.
    fn build(
        &mut self,
        commands: &mut Commands,
        assets: &mut SceneAssets,
        part: Part,
    ) -> Result<(), String> {
        let root = *self.root.get_or_insert_with(|| {
            commands.spawn(Transform::default()).id()
        });
        if let Some(stage) = part.stage {
            self.stage =
                Some(Built::of(commands, assets, root, stage)?);
        }
        let built = self
            .stage
            .as_ref()
            .ok_or("the first part had no stage")?;
        for piece in part.pieces {
            let look = built
                .looks
                .get(piece.look as usize)
                .ok_or("a piece of no look")?
                .clone();
            let shadows =
                built.stage.looks[piece.look as usize].shadows;
            let mesh =
                assets.add(Mesh::build(&MeshData::from(piece))?);
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(look),
                Shadows {
                    cast: shadows,
                    receive: true,
                },
                ChildOf(root),
            ));
        }
        Ok(())
    }

    /// The place of the camera, `turned` about the town.
    fn eye(&self) -> Option<Transform> {
        let stage = &self.stage.as_ref()?.stage;
        Some(stage.sight(self.aspect, self.turned))
    }
}

impl Built {
    /// The looks, lights and camera of `stage`, the lights under
    /// `root`.
    fn of(
        commands: &mut Commands,
        assets: &mut SceneAssets,
        root: Entity,
        stage: Stage,
    ) -> Result<Self, String> {
        let rgba = |[r, g, b]: [f32; 3]| (r, g, b, 1.0);
        let looks = stage
            .looks
            .iter()
            .map(|look| {
                Material::make(Pbr {
                    emissive: rgba(look.emissive),
                    roughness: look.roughness,
                    metallic: look.metallic,
                    ..Pbr::colored(rgba(look.base))
                })
                .map(|made| assets.add(made))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for sun in [&stage.sun, &stage.fill] {
            let Sun {
                color,
                lux,
                toward,
                shadows,
            } = *sun;
            commands.spawn((
                Transform::default()
                    .looking_to(Vec3::from(toward), Vec3::Y),
                SceneLight(Light::Directional(DirectionalLight {
                    color: rgba(color),
                    illuminance: lux,
                    shadows,
                })),
                ChildOf(root),
            ));
        }
        let (ambient, nits) = stage.ambient;
        let camera = commands
            .spawn((
                SceneCamera::viewport(CameraSettings {
                    projection: Projection::Perspective((
                        FOV,
                        0.5,
                        (stage.outer + stage.height) * 12.0,
                    )),
                    clear: Some(CLEAR),
                    tonemapping: Tonemapping::Neutral,
                    exposure: stage.exposure,
                    ambient: (rgba(ambient), nits),
                    ..CameraSettings::default()
                }),
                stage.sight(ASPECT, 0.0),
            ))
            .id();
        Ok(Self {
            stage,
            looks,
            camera,
        })
    }
}

impl From<Piece> for MeshData {
    fn from(piece: Piece) -> Self {
        Self {
            normals: piece.normals(),
            colors: piece.colors(),
            positions: piece.positions,
            uvs: Vec::new(),
            indices: piece.indices,
        }
    }
}

impl Stage {
    /// The camera `turned` about the town, framing the whole plinth
    /// in a view `aspect` times as wide as it is tall.
    fn sight(&self, aspect: f32, turned: f32) -> Transform {
        let (tilt_sin, tilt_cos) = TILT.sin_cos();
        let (turn_sin, turn_cos) = turned.sin_cos();
        let back = Vec3::new(
            turn_sin * tilt_cos,
            tilt_sin,
            turn_cos * tilt_cos,
        );
        let right = Vec3::Y.cross(back).normalize();
        let up = back.cross(right);
        let half = (FOV * 0.5).tan() * (1.0 - ROOM);
        let rims = [
            (self.outer, -self.depth),
            (self.radius, 0.0),
            (self.reach, self.height),
        ];
        let points = rims
            .iter()
            .flat_map(|&(radius, y)| {
                (0..RIM).map(move |k| {
                    let round = k as f32 / RIM as f32 * TAU;
                    Vec3::new(
                        round.cos() * radius,
                        y,
                        round.sin() * radius,
                    )
                })
            })
            .collect::<Vec<_>>();
        // The nearest the camera may come with every point in view,
        // its middle moved up or down until the plinth is in
        // the middle of the view.
        let mut middle = Vec3::ZERO;
        let mut far = 0.0;
        for _ in 0..3 {
            far = points
                .iter()
                .map(|p| {
                    let p = *p - middle;
                    let (x, y, z) =
                        (p.dot(right), p.dot(up), p.dot(back));
                    (x.abs() / (half * aspect) + z)
                        .max(y.abs() / half + z)
                })
                .fold(0.0f32, f32::max);
            let (low, high) = points
                .iter()
                .map(|p| {
                    let p = *p - middle;
                    p.dot(up) / (far - p.dot(back))
                })
                .fold((f32::MAX, f32::MIN), |(low, high), y| {
                    (low.min(y), high.max(y))
                });
            middle += up * (low + high) * 0.5 * far;
        }
        Transform::from_translation(middle + back * far)
            .looking_at(middle, Vec3::Y)
    }
}

/// Lets go of the town once its page is no longer open.
fn keep(world: &mut World) {
    let Some(template) = world
        .resource::<Diorama>()
        .shown
        .as_ref()
        .map(|shown| shown.template.clone())
    else {
        return;
    };
    let open = world.resource::<StallShown>().0 == Stall::Hamlets
        && world.resource::<Shelf<Template>>().open.as_deref()
            == Some(template.as_str());
    if !open {
        world.let_go();
    }
}

/// Builds each part as it comes, and asks for the next.
fn receive(
    world: &mut World,
    mut replies: Local<MessageCursor<Replied>>,
) {
    let replied = replies
        .read(world.resource::<Messages<Replied>>())
        .cloned()
        .collect::<Vec<_>>();
    for Replied { ticket, reply } in replied {
        let Some(mut shown) =
            world.resource_mut::<Diorama>().shown.take()
        else {
            return;
        };
        if shown.asked.1 == ticket {
            came(world, &mut shown, reply);
        }
        world.resource_mut::<Diorama>().shown = Some(shown);
    }
}

/// Takes in the part `shown` asked for.
fn came(
    world: &mut World,
    shown: &mut Shown,
    reply: Result<Reply, Failure>,
) {
    let part = match reply {
        Ok(Reply::Answered(bytes)) => {
            decode::<Part>(&bytes).map_err(|why| {
                format!("the realm answered oddly: {why}")
            })
        }
        Ok(_) => Err("the realm answered oddly".to_owned()),
        Err(Failure::NoImplementer) => {
            shown.seen = Seen::NoRealm;
            return;
        }
        Err(why) => Err(why.to_string()),
    };
    let built = part.and_then(|part| {
        let (at, of) = (part.part, part.parts);
        if at != shown.asked.0 {
            return Err(format!(
                "the realm sent part {at} for {}",
                shown.asked.0
            ));
        }
        world.resource_scope(
            |world, mut assets: Mut<SceneAssets>| {
                let mut commands = world.commands();
                shown.build(&mut commands, &mut assets, part)
            },
        )?;
        world.flush();
        Ok(at + 1 < of)
    });
    match built {
        Ok(true) => shown.ask(world, shown.asked.0 + 1),
        Ok(false) => {
            if let Some(built) = &shown.stage {
                shown.seen = Seen::Ready(built.camera);
                world.resource_mut::<Animate>().0 = true;
            }
        }
        Err(why) => {
            warn(&format!("the town of {}: {why}", shown.template));
            shown.seen = Seen::Failed(why);
        }
    }
}

/// Turns the camera about the town while it is seen, and follows the
/// view's size.
fn turn(
    mut diorama: ResMut<Diorama>,
    mut resized: MessageReader<SceneResize>,
    mut animate: ResMut<Animate>,
    time: Res<Time>,
    mut cameras: Query<&mut Transform, With<SceneCamera>>,
) {
    let Some(shown) = &mut diorama.shown else {
        return;
    };
    let Seen::Ready(camera) = shown.seen else {
        return;
    };
    for size in resized.read().filter(|size| size.view == camera) {
        // A view with no room is behind another tab, or not laid out.
        let room = size.width > 0.0 && size.height > 0.0;
        if room {
            shown.aspect = size.width / size.height;
        }
        if animate.0 != room {
            animate.0 = room;
        }
    }
    if !animate.0 {
        return;
    }
    shown.turned =
        (shown.turned + time.delta_secs() * TAU / TURN) % TAU;
    if let (Some(eye), Ok(mut at)) =
        (shown.eye(), cameras.get_mut(camera))
    {
        *at = eye;
    }
}
