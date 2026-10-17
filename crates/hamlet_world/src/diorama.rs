//! Dioramas: a hamlet drawn small and apart from the world, to be
//! shown in a panel before it is bought. Its common, houses, yards
//! and robots are the generator's own, stood on a round plinth on a
//! render layer no other camera sees, and drawn by a camera of their
//! own into a picture. One stands at a time, and only while a
//! picture is asked for.

use std::f32::consts::TAU;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::{ColorGrading, ColorGradingGlobal};
use bevy::window::RequestRedraw;

use crate::generator::house::{self, TownStyle};
use crate::generator::land::Land;
use crate::generator::layout::{self, HALL_PLOT};
use crate::generator::society::AgentId;
use crate::generator::{common, flora, yard};
use crate::keeper::{self, KState, Who};
use crate::meshes::{Mb, Rng, noise2};
use crate::palette::Palette;
use crate::sky::Looks;
use crate::{Stock, WorldSystems};

/// The render layer dioramas are drawn on.
const LAYER: usize = 9;

/// Where a diorama stands: under the world, past the far end of the
/// world camera's sight.
const AWAY: Vec3 = Vec3::new(0.0, -9000.0, 0.0);

/// The pixels of a still, and of the turning picture. Both are wider
/// than tall by the same share.
const STILL: UVec2 = UVec2::new(384, 240);
const TURNING: UVec2 = UVec2::new(1024, 640);

/// Frames a still is drawn for before it is kept, so that its meshes
/// have reached the GPU and its robots taken their poses.
const STILL_FRAMES: u32 = 3;

/// Radians the turning diorama turns in a second.
const TURN_RATE: f32 = TAU / 50.0;

/// The way a still is seen from, turned from +Z, where the yards
/// face.
const FRONT: f32 = 0.55;

/// The camera's height over the plinth as an angle, its field of
/// view, and its distance in radii of the plinth. A still is seen
/// closer, as it is drawn small.
const TILT: f32 = 0.6;
const FOV: f32 = 0.42;
const DISTANCE: f32 = 3.5;
const STILL_DISTANCE: f32 = 2.9;

/// The robots are drawn larger than in the world, so that each agent
/// reads beside its house.
const ROBOT_SCALE: f32 = 1.5;

/// Grass left about the outermost house, the depth of the soil under
/// it and of the wooden base under that, and how far the base stands
/// out.
const MARGIN: f32 = 3.5;
const SOIL: f32 = 1.1;
const BASE: f32 = 0.8;
const LIP: f32 = 0.45;

/// Trees on the grass about the houses.
const TREES: usize = 11;

/// The first id the houses and robots of dioramas take their looks
/// from. Those of the world count up from naught.
const FIRST_ID: u32 = 1 << 20;

pub fn plugin(app: &mut App) {
    app.init_resource::<Dioramas>()
        .init_resource::<Stand>()
        .add_systems(
            Update,
            (
                keep.run_if(wanted),
                lay.run_if(any_with_component::<Unlaid>),
            )
                .chain()
                .run_if(resource_exists::<Land>)
                .before(WorldSystems),
        );
}

/// A town to be drawn small: the seed its looks come from, and the
/// number of its agents with the mayor.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Model {
    pub seed: u64,
    pub agents: u32,
}

/// The pictures of dioramas asked for.
#[derive(Resource, Default)]
pub struct Dioramas {
    /// The model drawn turning into the picture of [`turning`],
    /// while one is.
    pub turning: Option<Model>,
    /// A still of each model asked for, and whether it is drawn.
    stills: Vec<(Model, Handle<Image>, bool)>,
    live: Option<Handle<Image>>,
}

/// The still of `model`, drawn soon if it is not yet. None where no
/// diorama is drawn.
pub fn still(
    world: &mut World,
    model: Model,
) -> Option<Handle<Image>> {
    let kept = world
        .get_resource::<Dioramas>()?
        .stills
        .iter()
        .find(|(of, ..)| *of == model)
        .map(|(_, image, _)| image.clone());
    if kept.is_some() {
        return kept;
    }
    let image =
        world.resource_mut::<Assets<Image>>().add(picture(STILL));
    world.resource_mut::<Dioramas>().stills.push((
        model,
        image.clone(),
        false,
    ));
    Some(image)
}

/// The picture the turning diorama is drawn into. None where no
/// diorama is drawn.
pub fn turning(world: &mut World) -> Option<Handle<Image>> {
    if let Some(live) = &world.get_resource::<Dioramas>()?.live {
        return Some(live.clone());
    }
    let image =
        world.resource_mut::<Assets<Image>>().add(picture(TURNING));
    world.resource_mut::<Dioramas>().live = Some(image.clone());
    Some(image)
}

fn picture(size: UVec2) -> Image {
    Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Bgra8UnormSrgb,
        None,
    )
}

/// The diorama standing, and the camera that draws it.
#[derive(Resource, Default)]
struct Stand {
    /// The model standing, its root and the radius of its plinth.
    up: Option<(Model, Entity, f32)>,
    /// Frames it has been drawn for.
    frames: u32,
    /// How far the turning diorama has turned.
    turned: f32,
    eye: Option<Entity>,
    parts: Option<Parts>,
}

/// The meshes every diorama shares.
struct Parts {
    desk: Handle<Mesh>,
    engine: Handle<Mesh>,
    wheel: Handle<Mesh>,
    trees: Vec<Handle<Mesh>>,
}

impl Parts {
    fn new(land: &Land, meshes: &mut Assets<Mesh>) -> Self {
        let (pal, shape) = (&land.pal, &land.set.yard);
        Parts {
            desk: meshes.add(yard::desk(pal, shape).build()),
            engine: meshes.add(yard::engine(pal, shape).build()),
            wheel: meshes.add(yard::wheel(pal).build()),
            trees: (0..flora::KINDS)
                .map(|kind| meshes.add(flora::mesh(pal, kind)))
                .collect(),
        }
    }
}

/// The root of a diorama whose parts are not yet on its layer.
#[derive(Component)]
struct Unlaid;

/// The camera dioramas are drawn by.
#[derive(Component)]
struct Eye;

/// True while a picture is wanted, or a diorama stands that is not.
fn wanted(dioramas: Res<Dioramas>, stand: Res<Stand>) -> bool {
    stand.up.is_some()
        || dioramas.turning.is_some()
        || dioramas.stills.iter().any(|(.., drawn)| !drawn)
}

/// Stands up the diorama a picture is wanted of, the stills first,
/// and points the camera at it. Takes it away once none is wanted.
#[allow(clippy::too_many_arguments)]
fn keep(
    mut commands: Commands,
    mut dioramas: ResMut<Dioramas>,
    mut stand: ResMut<Stand>,
    land: Res<Land>,
    stock: Res<Stock>,
    kit: Res<keeper::Kit>,
    looks: Res<Looks>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut eyes: Query<
        (
            &mut Camera,
            &mut RenderTarget,
            &mut Projection,
            &mut Transform,
        ),
        With<Eye>,
    >,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let next = dioramas.stills.iter().position(|(.., drawn)| !drawn);
    let wanted = match (next, dioramas.turning, &dioramas.live) {
        (Some(at), ..) => {
            let (model, image, _) = &dioramas.stills[at];
            Some((*model, image.clone()))
        }
        (None, Some(model), Some(live)) => {
            Some((model, live.clone()))
        }
        _ => None,
    };
    let Some((model, image)) = wanted else {
        if let Some((_, root, _)) = stand.up {
            commands.entity(root).despawn();
            stand.up = None;
        }
        for (mut camera, ..) in &mut eyes {
            if camera.is_active {
                camera.is_active = false;
            }
        }
        return;
    };
    redraw.write(RequestRedraw);
    let stand = &mut *stand;
    if stand.eye.is_none() {
        stand.eye = Some(spawn_eye(&mut commands, &looks));
    }
    if stand.up.is_none_or(|(up, ..)| up != model) {
        if let Some((_, root, _)) = stand.up {
            commands.entity(root).despawn();
        }
        let parts = stand
            .parts
            .get_or_insert_with(|| Parts::new(&land, &mut meshes));
        let kept = Kept {
            land: &land,
            stock: &stock,
            kit: &kit,
            parts,
        };
        let (root, radius) = kept.raise(
            &mut commands,
            &mut meshes,
            &mut materials,
            model,
        );
        stand.up = Some((model, root, radius));
        stand.frames = 0;
    }
    stand.frames += 1;
    let (turn, distance) = match next {
        Some(at) => {
            if stand.frames > STILL_FRAMES {
                dioramas.stills[at].2 = true;
            }
            (FRONT, STILL_DISTANCE)
        }
        None => {
            stand.turned =
                (stand.turned + TURN_RATE * time.delta_secs()) % TAU;
            (FRONT + stand.turned, DISTANCE)
        }
    };
    let (
        Some((_, _, radius)),
        Ok((mut camera, mut target, mut projection, mut at)),
    ) = (stand.up, eyes.single_mut())
    else {
        return;
    };
    let aimed = matches!(
        &*target,
        RenderTarget::Image(at) if at.handle == image
    );
    if !aimed {
        *target = RenderTarget::Image(ImageRenderTarget {
            handle: image,
            scale_factor: 1.0,
        });
        // Bevy measures a camera's picture again only when its
        // projection or its target's asset changes.
        projection.set_changed();
    }
    if !camera.is_active {
        camera.is_active = true;
    }
    *at = sight(radius * distance, turn);
}

/// The place of the camera `far` from the middle of a diorama,
/// turned `turn` about it.
fn sight(far: f32, turn: f32) -> Transform {
    let middle = AWAY - Vec3::Y * 1.5;
    let from = Vec3::new(
        turn.sin() * TILT.cos(),
        TILT.sin(),
        turn.cos() * TILT.cos(),
    );
    Transform::from_translation(middle + from * far)
        .looking_at(middle, Vec3::Y)
}

/// The camera of dioramas and its lights, as on a clear day.
fn spawn_eye(commands: &mut Commands, looks: &Looks) -> Entity {
    let day = &looks.day;
    let layer = RenderLayers::layer(LAYER);
    commands.spawn((
        DirectionalLight {
            color: day.sun,
            illuminance: day.sun_lux,
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 1.0,
            maximum_distance: 300.0,
            first_cascade_far_bound: 300.0,
            ..default()
        }
        .build(),
        Transform::default().looking_to(day.sun_dir, Vec3::Y),
        layer.clone(),
    ));
    // From the other side and above, as the world's own fill.
    let fill =
        Vec3::new(-day.sun_dir.x, -0.5, -day.sun_dir.z * 0.4 - 0.5);
    commands.spawn((
        DirectionalLight {
            color: day.fill,
            illuminance: day.fill_lux,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default().looking_to(fill, Vec3::Y),
        layer.clone(),
    ));
    commands
        .spawn((
            Eye,
            Camera3d::default(),
            Camera {
                // Before the world's camera and the window's.
                order: -2,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: FOV,
                near: 1.0,
                far: 600.0,
                ..default()
            }),
            Msaa::Sample4,
            Tonemapping::TonyMcMapface,
            AmbientLight {
                color: day.ambient,
                brightness: day.ambient_nits,
                affects_lightmapped_meshes: true,
            },
            ColorGrading {
                global: ColorGradingGlobal {
                    exposure: day.exposure,
                    post_saturation: day.saturation,
                    ..default()
                },
                ..default()
            },
            layer,
        ))
        .id()
}

/// Puts every part of a new diorama on the dioramas' layer.
fn lay(
    mut commands: Commands,
    roots: Query<Entity, With<Unlaid>>,
    children: Query<&Children>,
) {
    let layer = RenderLayers::layer(LAYER);
    for root in &roots {
        commands
            .entity(root)
            .remove::<Unlaid>()
            .insert(layer.clone());
        for part in children.iter_descendants(root) {
            commands.entity(part).try_insert(layer.clone());
        }
    }
}

/// What a diorama is built from.
struct Kept<'a> {
    land: &'a Land,
    stock: &'a Stock,
    kit: &'a keeper::Kit,
    parts: &'a Parts,
}

impl Kept<'_> {
    /// Stands `model` up: its plinth, common, houses, yards, trees
    /// and robots, laid out as its town would be. Returns its
    /// root and the radius of its plinth.
    fn raise(
        &self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        model: Model,
    ) -> (Entity, f32) {
        let (land, stock, parts) =
            (self.land, self.stock, self.parts);
        let (pal, town) = (&land.pal, &land.set.town);
        let site = (model.seed as usize % pal.towns.len())
            .min(land.sites.len().saturating_sub(1));
        let style = TownStyle::of(land, site);
        let place = &land.sites[site];
        // The town in its own frame, with its middle at the origin.
        let unturn = Vec2::from_angle(-place.turn);
        let local = |p: Vec2| unturn.rotate(p - place.at);
        let root = commands
            .spawn((
                Transform::from_translation(AWAY),
                Visibility::Visible,
                Unlaid,
            ))
            .id();
        let matte = || MeshMaterial3d(stock.matte.clone());
        commands.spawn((
            Mesh3d(
                meshes.add(common::mesh(land, site, &style).build()),
            ),
            matte(),
            ChildOf(root),
        ));

        let mut paths = Mb::new();
        let mut homes = Vec::new();
        let first = FIRST_ID + (model.seed % 4096) as u32 * 64;
        for k in 0..model.agents.max(1) {
            let hall = k == HALL_PLOT;
            let id = AgentId(first + k);
            let spot = layout::plot(land, site, k);
            let at = local(spot.at);
            let facing = unturn.rotate(spot.facing());
            let yaw = facing.x.atan2(facing.y);
            let turned = Quat::from_rotation_y(yaw);
            let yard = house::yard_of(land, id, hall, yaw, 0);
            let built =
                house::house(land, id, &style, hall, hall, 0, &yard);
            let home = commands
                .spawn((
                    Mesh3d(meshes.add(built.body.build())),
                    matte(),
                    Transform::from_xyz(at.x, 0.0, at.y)
                        .with_rotation(turned),
                    ChildOf(root),
                ))
                .id();
            commands.spawn((
                Mesh3d(meshes.add(built.panes.build())),
                MeshMaterial3d(stock.windows.clone()),
                NotShadowCaster,
                ChildOf(home),
            ));
            commands.spawn((
                Mesh3d(parts.desk.clone()),
                matte(),
                yard.desk_at(),
                ChildOf(home),
            ));
            let engine = commands
                .spawn((
                    Mesh3d(parts.engine.clone()),
                    matte(),
                    yard.engine_at(),
                    ChildOf(home),
                ))
                .id();
            commands.spawn((
                Mesh3d(parts.wheel.clone()),
                matte(),
                yard.wheel(0.0),
                ChildOf(engine),
            ));
            if !hall
                && let Some((at, kind, scale)) =
                    house::garden_tree(land, id, &yard)
            {
                commands.spawn((
                    Mesh3d(parts.trees[kind].clone()),
                    matte(),
                    Transform::from_translation(at)
                        .with_scale(Vec3::splat(scale)),
                    ChildOf(home),
                ));
            }

            // The robot waits on its porch, on the paving where that
            // reaches.
            let porch = local(spot.porch());
            let lift = if porch.length() < town.plaza_radius - 0.1 {
                town.plaza_lift
            } else {
                0.0
            };
            let who = if hall {
                Who::leader()
            } else {
                Who::citizen(id.0 * 7 + 3)
            };
            let robot = keeper::spawn_as(
                commands,
                self.kit,
                materials,
                who,
                Transform::from_xyz(porch.x, lift, porch.y)
                    .with_rotation(turned)
                    .with_scale(Vec3::splat(ROBOT_SCALE)),
                KState::Rest,
            );
            commands.entity(robot).insert(ChildOf(root));
            if !hall {
                path(
                    &mut paths,
                    pal,
                    local(spot.door()),
                    town.plaza_radius,
                );
            }
            homes.push(at);
        }
        commands.spawn((
            Mesh3d(meshes.add(paths.build())),
            matte(),
            NotShadowCaster,
            ChildOf(root),
        ));

        let reach = homes
            .iter()
            .map(|at| at.length() + town.house_radius)
            .fold(town.plaza_radius, f32::max);
        let radius = reach + MARGIN;
        commands.spawn((
            Mesh3d(
                meshes.add(plinth(pal, radius, model.seed).build()),
            ),
            matte(),
            ChildOf(root),
        ));
        let mut brass = Mb::new();
        brass.torus(
            Vec3::Y * -(SOIL + BASE * 0.3),
            Vec3::Y,
            radius + LIP,
            0.06,
            96,
            pal.brass,
        );
        commands.spawn((
            Mesh3d(meshes.add(brass.build())),
            MeshMaterial3d(stock.brass.clone()),
            ChildOf(root),
        ));

        // Trees on the grass about the town, off the houses.
        let mut rng = Rng::seeded(&format!("diorama {}", model.seed));
        let mut planted = 0;
        for _ in 0..TREES * 8 {
            if planted == TREES {
                break;
            }
            let a = rng.f() * TAU;
            let r = rng.range(reach - 1.0, radius - 1.2);
            let at = Vec2::from_angle(a) * r;
            if homes.iter().any(|home| home.distance(at) < 4.6) {
                continue;
            }
            let kind = (rng.f() * flora::KINDS as f32) as usize;
            commands.spawn((
                Mesh3d(
                    parts.trees[kind.min(flora::KINDS - 1)].clone(),
                ),
                matte(),
                Transform::from_xyz(at.x, 0.0, at.y)
                    .with_rotation(Quat::from_rotation_y(
                        rng.f() * TAU,
                    ))
                    .with_scale(Vec3::splat(rng.range(0.65, 1.05))),
                ChildOf(root),
            ));
            planted += 1;
        }
        (root, radius)
    }
}

/// A trodden path from a door to the edge of the common.
fn path(mb: &mut Mb, pal: &Palette, door: Vec2, common: f32) {
    let towards = -door.normalize_or_zero();
    let length = (door.length() - common + 0.3).max(0.0);
    let side = towards.perp() * 0.5;
    let steps = (length / 0.8).ceil().max(1.0) as usize;
    for i in 0..steps {
        let (a, b) = (
            door + towards * (length * i as f32 / steps as f32),
            door + towards * (length * (i + 1) as f32 / steps as f32),
        );
        let shade = noise2(a.x * 0.7, a.y * 0.7);
        let colour = pal.dirt.lerp(pal.dirt_shade, shade * 0.5);
        let lift = |p: Vec2| Vec3::new(p.x, 0.025, p.y);
        mb.face(
            [
                lift(a - side),
                lift(a + side),
                lift(b + side),
                lift(b - side),
            ],
            Vec3::Y,
            colour,
        );
    }
}

/// The plinth a diorama stands on: grass on top, the soil under it,
/// and a wooden base that stands out a little.
fn plinth(pal: &Palette, radius: f32, seed: u64) -> Mb {
    const SEGMENTS: usize = 96;
    const RINGS: usize = 10;
    let mut mb = Mb::new();
    let salt = (seed % 97) as f32 * 3.1;
    let grass = |p: Vec2| {
        let n = noise2(p.x * 0.16 + salt, p.y * 0.16);
        let fine = noise2(p.x * 0.7 - salt, p.y * 0.7);
        pal.grass_shade
            .lerp(pal.grass, 0.45 + 0.5 * n)
            .lerp(pal.grass_sun, 0.25 * fine)
    };
    let middle = mb.v(Vec3::ZERO, Vec3::Y, grass(Vec2::ZERO));
    let mut last = vec![middle; SEGMENTS];
    for ring in 1..=RINGS {
        let r = radius * ring as f32 / RINGS as f32;
        let now = (0..SEGMENTS)
            .map(|s| {
                let p = Vec2::from_angle(
                    s as f32 / SEGMENTS as f32 * TAU,
                ) * r;
                mb.v(Vec3::new(p.x, 0.0, p.y), Vec3::Y, grass(p))
            })
            .collect::<Vec<_>>();
        for s in 0..SEGMENTS {
            let n = (s + 1) % SEGMENTS;
            if ring == 1 {
                mb.tri(middle, now[s], now[n]);
            } else {
                mb.quad(last[s], last[n], now[n], now[s]);
            }
        }
        last = now;
    }
    let base = |_, _| pal.timber_dark;
    let soil = [pal.dirt_shade, pal.mud, pal.dirt, pal.dirt_shade];
    mb.lathe(
        Vec3::ZERO,
        &[
            (radius, -SOIL),
            (radius, -SOIL * 0.66),
            (radius, -SOIL * 0.33),
            (radius, -0.08),
            (radius, 0.0),
        ],
        SEGMENTS,
        |ring, _| match ring {
            3 => pal.grass_shade,
            ring => soil[ring % soil.len()],
        },
    );
    let out = radius + LIP;
    mb.lathe(
        Vec3::ZERO,
        &[(out, -SOIL), (radius, -SOIL)],
        SEGMENTS,
        base,
    );
    mb.lathe(
        Vec3::ZERO,
        &[(out, -SOIL - BASE), (out, -SOIL)],
        SEGMENTS,
        base,
    );
    mb.lathe(
        Vec3::ZERO,
        &[(0.0, -SOIL - BASE), (out, -SOIL - BASE)],
        SEGMENTS,
        base,
    );
    mb
}
