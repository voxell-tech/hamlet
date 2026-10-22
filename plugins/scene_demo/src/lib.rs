//! A lit scene in a panel, to show what a plugin can draw: shapes
//! under a sun, a mesh of its own with a colour at each vertex, a
//! ring in a shader of its own, a pool of light laid as a decal, and
//! a field of cubes drawn as copies of one, lifted in waves by
//! compute. Dragging turns the camera about the middle, the wheel
//! brings it nearer, the shape under the pointer lights up and a
//! click names what it hit. Where the camera is stays across a
//! reload.

use std::f32::consts::{FRAC_PI_2, TAU};

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    AlphaMode, Bloom, Buffer, CameraSettings, Compute, Decal,
    DirectionalLight, Fog, FogFalloff, Light, Material, Mesh,
    MeshData, MouseButton, Pbr, Rgba, Shaded, Shader, ShaderKind,
    Shape, Texture, viewport,
};
use hamlet_engine_bevy::ui::{
    FrameProps, Space, Tone, column, label, resource, row,
};
use serde::{Deserialize, Serialize};

/// The keys of what the plugin names.
const CAMERA: u32 = 1;
const SUN: u32 = 2;
const GROUND: u32 = 3;
const POOL: u32 = 4;
const SPHERE: u32 = 11;
const RING: u32 = 12;
const PYRAMID: u32 = 13;
const FIELD: u32 = 100;

/// Cubes along each side of the field.
const SIDE: u32 = 20;
/// Invocations of a workgroup of `waves.wgsl`.
const WORKGROUP: u32 = 64;
/// Pixels along each side of the pool of light's picture.
const POOL_SIDE: u32 = 64;

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
    fn build(&self, app: &mut App) {
        app.init_resource::<Said>()
            .init_saved_resource::<Orbit>()
            .add_panel("scene", "Scene", panel)
            .panel_padding(Space::None)
            .add_systems(Startup, setup)
            .add_systems(Update, (light_up, name_clicks, turn));
    }
}

hamlet_engine_bevy::export!(SceneDemo);

/// The line over the view.
#[derive(Resource, Default)]
struct Said(String);

/// The camera's place about the middle.
#[derive(Resource, Clone, Copy, Serialize, Deserialize)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    distance: f32,
    dragging: bool,
}

/// A shape the pointer can light up, and the material it has unlit.
#[derive(Component)]
struct Lit {
    name: &'static str,
    material: Handle<Material>,
}

/// The field of cubes.
#[derive(Component)]
struct Field;

/// The materials the pointer changes, and the compute that lifts the
/// field, kept so it runs.
#[derive(Resource)]
struct Looks {
    glowing: Handle<Material>,
    field: Handle<Material>,
    _waves: Handle<Compute>,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: 0.6,
            pitch: 0.45,
            distance: 18.0,
            dragging: false,
        }
    }
}

impl Orbit {
    fn transform(self) -> Transform {
        let (yaw_sin, yaw_cos) = self.yaw.sin_cos();
        let (pitch_sin, pitch_cos) = self.pitch.sin_cos();
        let eye = Vec3::new(
            self.distance * pitch_cos * yaw_sin,
            self.distance * pitch_sin,
            self.distance * pitch_cos * yaw_cos,
        );
        Transform::from_translation(eye)
            .looking_at(Vec3::ZERO, Vec3::Y)
    }
}

/// The line over the view, padded on its own, over a view that
/// reaches the panel's edges.
fn panel(camera: Query<Entity, With<SceneCamera>>) -> AnyView {
    let label =
        row((label(resource::<Said, _>(|said| said.0.clone()))
            .tone(Tone::Dim),))
        .padding(Space::Gap);
    match camera.single() {
        Ok(camera) => column((label, viewport(camera)))
            .space(Space::None)
            .boxed(),
        Err(_) => column((label,)).space(Space::None).boxed(),
    }
}

fn setup(
    mut commands: Commands,
    mut assets: ResMut<SceneAssets>,
    orbit: Res<Orbit>,
    mut said: ResMut<Said>,
) {
    said.0 = match build(&mut commands, &mut assets, *orbit) {
        Ok(()) => "Drag to turn, wheel to zoom".to_owned(),
        Err(why) => {
            hamlet_engine_bevy::error(&why);
            format!("The scene cannot be drawn: {why}")
        }
    };
}

fn build(
    commands: &mut Commands,
    assets: &mut SceneAssets,
    orbit: Orbit,
) -> Result<(), String> {
    let edges = assets.add(Shader::compile(
        ShaderKind::Pass,
        include_str!("edges.wgsl"),
    )?);
    commands.spawn((
        SceneKey(CAMERA),
        SceneCamera::viewport(settings()),
        orbit.transform(),
        PostPass {
            shader: edges,
            params: vec![0.35, 0.75, 0.5, 0.0],
        },
    ));
    commands.spawn((
        SceneKey(SUN),
        Transform::from_xyz(4.0, 10.0, 6.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        SceneLight(Light::Directional(DirectionalLight {
            color: WHITE,
            illuminance: 8_000.0,
            shadows: true,
        })),
    ));
    commands.spawn((
        SceneKey(GROUND),
        Mesh3d(assets.add(Mesh::shape(Shape::Plane((40.0, 40.0)))?)),
        MeshMaterial3d(
            assets.add(Material::make(Pbr::colored(SOIL))?),
        ),
    ));

    let opaque = Shaded {
        alpha_mode: AlphaMode::Opaque,
        double_sided: false,
        lit: true,
    };
    let ring_shader = assets.add(Shader::compile(
        ShaderKind::Material,
        include_str!("ring.wgsl"),
    )?);
    let shapes = [
        (
            SPHERE,
            "the sphere",
            Vec3::new(-3.0, 1.0, 0.0),
            assets.add(Mesh::shape(Shape::Sphere(1.0))?),
            assets.add(Material::make(Pbr::colored(SPHERE_COLOR))?),
        ),
        (
            RING,
            "the ring",
            Vec3::new(0.0, 1.2, -3.0),
            assets.add(Mesh::shape(Shape::Torus((0.3, 1.0)))?),
            assets.add_shaded_material(
                &ring_shader,
                opaque,
                &params(&[RING_LOW, RING_HIGH]),
                None,
                None,
            )?,
        ),
        (
            PYRAMID,
            "the pyramid",
            Vec3::new(3.0, 0.0, 0.0),
            assets.add(Mesh::build(&pyramid())?),
            assets.add(Material::make(Pbr::colored(WHITE))?),
        ),
    ];
    for (key, name, at, mesh, material) in shapes {
        commands.spawn((
            SceneKey(key),
            Transform::from_translation(at),
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Pickable {
                hover: true,
                click: true,
            },
            Lit { name, material },
        ));
    }
    let glowing = assets.add(Material::make(Pbr {
        emissive: (2.0, 1.6, 0.6, 1.0),
        ..Pbr::colored(GLOW)
    })?);

    let picture = assets.add(Texture::make(
        POOL_SIDE,
        POOL_SIDE,
        &pool_pixels(),
    )?);
    commands.spawn((
        SceneKey(POOL),
        Transform::from_xyz(-3.0, 0.0, 0.0)
            .with_scale(Vec3::new(4.0, 1.0, 4.0)),
        ForwardDecal,
        MeshMaterial3d(assets.add_decal_material(
            Decal {
                base_color: GLOW,
                emissive: (0.0, 0.0, 0.0, 1.0),
                unlit: true,
                fog: false,
                depth_fade: 1.0,
            },
            &picture,
        )?),
    ));

    let count = SIDE * SIDE;
    let lifts = assets.add(Buffer::make(count * 16)?);
    let field_shader = assets.add(Shader::compile(
        ShaderKind::Material,
        include_str!("field.wgsl"),
    )?);
    let field = assets.add_shaded_material(
        &field_shader,
        opaque,
        &field_params(None),
        None,
        Some(&lifts),
    )?;
    commands.spawn((
        SceneKey(FIELD),
        Instances {
            mesh: assets
                .add(Mesh::shape(Shape::Cuboid((0.4, 0.4, 0.4)))?),
            material: field.clone(),
            copies: field_places(),
        },
        Pickable {
            hover: true,
            click: true,
        },
        Field,
    ));

    let waves_shader = assets.add(Shader::compile(
        ShaderKind::Compute,
        include_str!("waves.wgsl"),
    )?);
    let waves = assets.add_compute(&waves_shader, &lifts, None)?;
    let run = assets.get(&waves).ok_or("the waves are gone")?;
    run.set_params(&[count as f32, SIDE as f32, 0.4, 0.0])?;
    run.dispatch(count.div_ceil(WORKGROUP), 1, 1, true)?;

    commands.insert_resource(Looks {
        glowing,
        field,
        _waves: waves,
    });
    Ok(())
}

/// Lights the shape or cube under the pointer up, and puts the one
/// it left back.
fn light_up(
    mut commands: Commands,
    mut over: MessageReader<SceneOver>,
    mut out: MessageReader<SceneOut>,
    lit: Query<&Lit>,
    field: Query<(), With<Field>>,
    looks: Option<Res<Looks>>,
    assets: Res<SceneAssets>,
) {
    let Some(looks) = looks else {
        return;
    };
    let changes =
        out.read().map(|out| (out.entity, out.index, false)).chain(
            over.read()
                .map(|over| (over.0.entity, over.0.index, true)),
        );
    for (entity, index, on) in changes {
        if field.contains(entity) {
            let _ = assets.set_params(
                &looks.field,
                &field_params(on.then_some(index)),
            );
        } else if let Ok(lit) = lit.get(entity) {
            let material =
                if on { &looks.glowing } else { &lit.material };
            commands
                .entity(entity)
                .insert(MeshMaterial3d(material.clone()));
        }
    }
}

fn name_clicks(
    mut clicks: MessageReader<SceneClick>,
    lit: Query<&Lit>,
    field: Query<(), With<Field>>,
    mut said: ResMut<Said>,
) {
    for click in clicks.read() {
        let hit = click.0;
        said.0 = if field.contains(hit.entity) {
            format!("Clicked cube {}", hit.index)
        } else {
            let name = lit
                .get(hit.entity)
                .map_or("something", |lit| lit.name);
            format!("Clicked {name}")
        };
    }
}

/// Turns the camera about the middle as the pointer drags, and brings
/// it nearer as the wheel turns.
fn turn(
    mut presses: MessageReader<ScenePress>,
    mut pointers: MessageReader<ScenePointer>,
    mut wheels: MessageReader<SceneWheel>,
    mut orbit: ResMut<Orbit>,
    mut camera: Query<&mut Transform, With<SceneCamera>>,
) {
    let before = (orbit.yaw, orbit.pitch, orbit.distance);
    for press in presses.read() {
        if press.button == MouseButton::Primary {
            orbit.dragging = press.down;
        }
    }
    for pointer in pointers.read() {
        if orbit.dragging {
            orbit.yaw = (orbit.yaw - pointer.dx * 0.01) % TAU;
            orbit.pitch = (orbit.pitch + pointer.dy * 0.01)
                .clamp(0.05, FRAC_PI_2 - 0.05);
        }
    }
    for wheel in wheels.read() {
        orbit.distance =
            (orbit.distance * (1.0 - wheel.y * 0.1)).clamp(4.0, 60.0);
    }
    if before != (orbit.yaw, orbit.pitch, orbit.distance)
        && let Ok(mut camera) = camera.single_mut()
    {
        *camera = orbit.transform();
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
            Transform::from_xyz(
                (x - middle) * 0.6,
                0.2,
                (z - middle) * 0.6 + 8.0,
            )
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

/// The picture of the pool of light: white, fading from its middle to
/// nothing at its edge.
fn pool_pixels() -> Vec<u8> {
    let half = POOL_SIDE as f32 / 2.0;
    (0..POOL_SIDE * POOL_SIDE)
        .flat_map(|at| {
            let x = (at % POOL_SIDE) as f32 + 0.5 - half;
            let y = (at / POOL_SIDE) as f32 + 0.5 - half;
            let out = ((x * x + y * y).sqrt() / half).min(1.0);
            let alpha = (1.0 - out).powi(2) * 255.0;
            [255, 255, 255, alpha as u8]
        })
        .collect()
}

/// A full colour at `turn` of the way round the wheel.
fn hue(turn: f32) -> (f32, f32, f32) {
    let channel = |offset: f32| {
        let at = (turn + offset).fract() * 6.0;
        (2.0 - (at - 2.0).abs()).clamp(0.0, 1.0)
    };
    (channel(0.0), channel(2.0 / 3.0), channel(1.0 / 3.0))
}
