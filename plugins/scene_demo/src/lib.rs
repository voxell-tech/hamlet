//! A lit scene in a panel, to show what a plugin can draw: shapes
//! under a sun, a mesh of its own with a colour at each vertex, a
//! ring in a shader of its own, and a field of cubes drawn as copies
//! of one, lifted in waves by compute. Dragging turns the camera
//! about the middle, the wheel brings it nearer, the shape under the
//! pointer lights up and a click names what it hit.

use std::f32::consts::{FRAC_PI_2, TAU};

use hamlet_engine_sdk::scene::gpu::{
    Buffer, Compute, Shaded, Shader, ShaderKind, set_params,
    set_pass, shaded_material,
};
use hamlet_engine_sdk::scene::{
    AlphaMode, Bloom, Camera, CameraSettings, DirectionalLight,
    Entity, Fog, FogFalloff, Instances, Light, Material, Mesh,
    MeshData, MouseButton, Pbr, Rgba, SceneInput, Shape, Target,
    Transform, copies, viewport,
};
use hamlet_engine_sdk::ui::{
    AnyView, Tone, ViewExt, World, column, label, resource,
};
use hamlet_engine_sdk::{Entry, Plugin};

/// The keys of what the plugin watches and names.
const CAMERA: u32 = 1;
const SUN: u32 = 2;
const GROUND: u32 = 3;
const SPHERE: u32 = 11;
const RING: u32 = 12;
const PYRAMID: u32 = 13;
const FIELD: u32 = 100;

/// Cubes along each side of the field.
const SIDE: u32 = 20;
/// Invocations of a workgroup of `waves.wgsl`.
const WORKGROUP: u32 = 64;

const SKY: Rgba = (0.02, 0.025, 0.04, 1.0);
const SOIL: Rgba = (0.08, 0.1, 0.09, 1.0);
const SPHERE_COLOR: Rgba = (0.85, 0.45, 0.2, 1.0);
const GLOW: Rgba = (1.0, 0.85, 0.4, 1.0);
const RING_LOW: Rgba = (0.1, 0.4, 0.9, 1.0);
const RING_HIGH: Rgba = (0.2, 0.95, 0.8, 1.0);
const FIELD_LOW: Rgba = (0.15, 0.2, 0.35, 1.0);
const FIELD_HIGH: Rgba = (0.5, 0.8, 1.0, 1.0);
const WHITE: Rgba = (1.0, 1.0, 1.0, 1.0);

#[derive(Default)]
struct SceneDemo;

impl Plugin for SceneDemo {
    fn declare(&self, entry: &mut Entry) {
        entry.panel("scene", "Scene");
    }

    fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
        if world.get::<Demo>().is_none() {
            let said = match Demo::build() {
                Ok(demo) => {
                    world.insert(demo);
                    "Drag to turn, wheel to zoom".to_owned()
                }
                Err(why) => {
                    hamlet_engine_sdk::error(&why);
                    format!("The scene cannot be drawn: {why}")
                }
            };
            world.insert(Said(said));
        }
        column((
            label(resource::<Said, _>(|said| said.0.clone()))
                .tone(Tone::Dim),
            viewport::<Demo>(|demo| Some(&demo.camera)),
        ))
        .boxed()
    }

    fn scene(&mut self, input: SceneInput, world: &mut World) {
        if world.get::<Demo>().is_none() {
            return;
        }
        if let Some(said) = world.resource_mut::<Demo>().answer(input)
        {
            world.resource_or_init::<Said>().0 = said;
        }
    }
}

hamlet_engine_sdk::export!(SceneDemo);

/// The line over the view.
#[derive(Default)]
struct Said(String);

/// A shape the pointer can light up.
struct Lit {
    key: u32,
    name: &'static str,
    entity: Entity,
    mesh: Mesh,
    material: Material,
}

/// The camera's place about the middle.
#[derive(Clone, Copy)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    distance: f32,
    dragging: bool,
}

impl Orbit {
    fn transform(self) -> Transform {
        let (yaw_sin, yaw_cos) = self.yaw.sin_cos();
        let (pitch_sin, pitch_cos) = self.pitch.sin_cos();
        let eye = (
            self.distance * pitch_cos * yaw_sin,
            self.distance * pitch_sin,
            self.distance * pitch_cos * yaw_cos,
        );
        Transform::looking_at(eye, (0.0, 0.0, 0.0))
    }
}

/// Everything the scene holds, kept so it stays.
struct Demo {
    at: Entity,
    camera: Camera,
    orbit: Orbit,
    lit: Vec<Lit>,
    glowing: Material,
    field_material: Material,
    _held: Held,
}

/// What is kept only so it is not dropped: the sun, the ground, the
/// field, the shaders, the field's lifts and their compute.
type Held = (Entity, Entity, Instances, Vec<Shader>, Buffer, Compute);

impl Demo {
    fn build() -> Result<Self, String> {
        let orbit = Orbit {
            yaw: 0.6,
            pitch: 0.45,
            distance: 18.0,
            dragging: false,
        };
        let at = Entity::make(None, CAMERA)?;
        at.set_transform(orbit.transform());
        let camera = Camera::make(&at, settings(), Target::Viewport)?;

        let sun = Entity::make(None, SUN)?;
        sun.set_transform(Transform::looking_at(
            (4.0, 10.0, 6.0),
            (0.0, 0.0, 0.0),
        ));
        sun.set_light(Some(Light::Directional(DirectionalLight {
            color: WHITE,
            illuminance: 8_000.0,
            shadows: true,
        })))?;

        let ground = Entity::make(None, GROUND)?;
        ground.draw(
            &Mesh::shape(Shape::Plane((40.0, 40.0)))?,
            &Material::make(Pbr::colored(SOIL))?,
        );

        let ring_shader = Shader::compile(
            ShaderKind::Material,
            include_str!("ring.wgsl"),
        )?;
        let ring_material = shaded_material(
            &ring_shader,
            Shaded {
                alpha_mode: AlphaMode::Opaque,
                double_sided: false,
                lit: true,
            },
            &params(&[RING_LOW, RING_HIGH]),
            None,
            None,
        )?;
        let lit = vec![
            Lit::make(
                SPHERE,
                "the sphere",
                (-3.0, 1.0, 0.0),
                Mesh::shape(Shape::Sphere(1.0))?,
                Material::make(Pbr::colored(SPHERE_COLOR))?,
            )?,
            Lit::make(
                RING,
                "the ring",
                (0.0, 1.2, -3.0),
                Mesh::shape(Shape::Torus((0.3, 1.0)))?,
                ring_material,
            )?,
            Lit::make(
                PYRAMID,
                "the pyramid",
                (3.0, 0.0, 0.0),
                Mesh::build(&pyramid())?,
                Material::make(Pbr::colored(WHITE))?,
            )?,
        ];
        let glowing = Material::make(Pbr {
            emissive: (2.0, 1.6, 0.6, 1.0),
            ..Pbr::colored(GLOW)
        })?;

        let count = SIDE * SIDE;
        let lifts = Buffer::make(count * 16)?;
        let field_shader = Shader::compile(
            ShaderKind::Material,
            include_str!("field.wgsl"),
        )?;
        let field_material = shaded_material(
            &field_shader,
            Shaded {
                alpha_mode: AlphaMode::Opaque,
                double_sided: false,
                lit: true,
            },
            &field_params(None),
            None,
            Some(&lifts),
        )?;
        let cube = Mesh::shape(Shape::Cuboid((0.4, 0.4, 0.4)))?;
        let field =
            Instances::make(None, &cube, &field_material, FIELD)?;
        field.set(&copies(&field_places()))?;
        field.watch(true, true);

        let waves_shader = Shader::compile(
            ShaderKind::Compute,
            include_str!("waves.wgsl"),
        )?;
        let waves = Compute::make(&waves_shader, &lifts, None)?;
        waves.set_params(&[count as f32, SIDE as f32, 0.4, 0.0])?;
        waves.dispatch(count.div_ceil(WORKGROUP), 1, 1, true)?;

        let edges_shader = Shader::compile(
            ShaderKind::Pass,
            include_str!("edges.wgsl"),
        )?;
        set_pass(
            &camera,
            Some(&edges_shader),
            &[0.35, 0.75, 0.5, 0.0],
        )?;

        Ok(Self {
            at,
            camera,
            orbit,
            lit,
            glowing,
            field_material,
            _held: (
                sun,
                ground,
                field,
                vec![
                    ring_shader,
                    field_shader,
                    waves_shader,
                    edges_shader,
                ],
                lifts,
                waves,
            ),
        })
    }

    /// Answers what the person did: what the line over the view says
    /// now, if it changed.
    fn answer(&mut self, input: SceneInput) -> Option<String> {
        match input {
            SceneInput::Over(hit) => {
                self.light(hit.key, hit.index, true);
                None
            }
            SceneInput::Out((key, index)) => {
                self.light(key, index, false);
                None
            }
            SceneInput::Click(hit) => Some(match hit.key {
                FIELD => format!("Clicked cube {}", hit.index),
                key => {
                    let name = self
                        .lit
                        .iter()
                        .find(|lit| lit.key == key)
                        .map_or("something", |lit| lit.name);
                    format!("Clicked {name}")
                }
            }),
            SceneInput::Pressed(press) => {
                if press.button == MouseButton::Primary {
                    self.orbit.dragging = press.down;
                }
                None
            }
            SceneInput::Moved(pointer) if self.orbit.dragging => {
                self.orbit.yaw =
                    (self.orbit.yaw - pointer.dx * 0.01) % TAU;
                self.orbit.pitch = (self.orbit.pitch
                    + pointer.dy * 0.01)
                    .clamp(0.05, FRAC_PI_2 - 0.05);
                self.at.set_transform(self.orbit.transform());
                None
            }
            SceneInput::Turned(wheel) => {
                self.orbit.distance = (self.orbit.distance
                    * (1.0 - wheel.y * 0.1))
                    .clamp(4.0, 60.0);
                self.at.set_transform(self.orbit.transform());
                None
            }
            _ => None,
        }
    }

    /// Lights the shape or cube of `key` and `index` up, or puts it
    /// back.
    fn light(&self, key: u32, index: u32, on: bool) {
        if key == FIELD {
            let at = on.then_some(index);
            let _ =
                set_params(&self.field_material, &field_params(at));
            return;
        }
        if let Some(lit) = self.lit.iter().find(|lit| lit.key == key)
        {
            let material =
                if on { &self.glowing } else { &lit.material };
            lit.entity.draw(&lit.mesh, material);
        }
    }
}

impl Lit {
    fn make(
        key: u32,
        name: &'static str,
        at: (f32, f32, f32),
        mesh: Mesh,
        material: Material,
    ) -> Result<Self, String> {
        let entity = Entity::make(None, key)?;
        entity.set_transform(Transform::at(at));
        entity.draw(&mesh, &material);
        entity.watch(true, true);
        Ok(Self {
            key,
            name,
            entity,
            mesh,
            material,
        })
    }
}

fn settings() -> CameraSettings {
    CameraSettings {
        clear: Some(SKY),
        fog: Some(Fog {
            color: SKY,
            falloff: FogFalloff::Linear((20.0, 60.0)),
        }),
        bloom: Some(Bloom {
            intensity: 0.2,
            low_frequency_boost: 0.6,
            threshold: 1.0,
        }),
        ..CameraSettings::default()
    }
}

/// The floats of `colors`, four each.
fn params(colors: &[Rgba]) -> Vec<f32> {
    colors
        .iter()
        .flat_map(|&(r, g, b, a)| [r, g, b, a])
        .collect()
}

/// The field's params, the cube `lit` lit up.
fn field_params(lit: Option<u32>) -> Vec<f32> {
    let mut floats = params(&[FIELD_LOW, FIELD_HIGH]);
    let at = lit.map_or(-1.0, |index| index as f32);
    floats.extend([at, 0.0, 0.0, 0.0]);
    floats.extend(params(&[GLOW]));
    floats
}

/// A cube's place in the field, row by row.
fn field_places() -> Vec<Transform> {
    let middle = (SIDE as f32 - 1.0) / 2.0;
    (0..SIDE * SIDE)
        .map(|at| {
            let (x, z) = ((at % SIDE) as f32, (at / SIDE) as f32);
            Transform::at((
                (x - middle) * 0.6,
                0.2,
                (z - middle) * 0.6 + 8.0,
            ))
        })
        .collect()
}

/// A six-sided pyramid, its tip white and its foot going round the
/// colours.
fn pyramid() -> MeshData {
    let sides = 6;
    let mut positions = vec![0.0, 2.0, 0.0, 0.0, 0.0, 0.0];
    let mut colors = vec![1.0, 1.0, 1.0, 1.0, 0.2, 0.2, 0.2, 1.0];
    for side in 0..sides {
        let turn = side as f32 / sides as f32 * TAU;
        positions.extend([turn.cos(), 0.0, turn.sin()]);
        let (r, g, b) = hue(side as f32 / sides as f32);
        colors.extend([r, g, b, 1.0]);
    }
    let mut indices = Vec::new();
    for side in 0..sides {
        let (this, next) = (2 + side, 2 + (side + 1) % sides);
        indices.extend([0, next, this, 1, this, next]);
    }
    MeshData {
        positions,
        normals: Vec::new(),
        uvs: Vec::new(),
        colors,
        indices,
    }
}

/// A full colour at `turn` of the way round the wheel.
fn hue(turn: f32) -> (f32, f32, f32) {
    let channel = |offset: f32| {
        let at = (turn + offset).fract() * 6.0;
        (2.0 - (at - 2.0).abs()).clamp(0.0, 1.0)
    };
    (channel(0.0), channel(2.0 / 3.0), channel(1.0 / 3.0))
}
