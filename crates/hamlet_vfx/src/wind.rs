//! The wind: one way across the whole world, and a strength that
//! rolls downwind in bands. Everything that moves in it reads the
//! same [`WindSettings::strength`], or `wind.wesl` on the GPU.
//!
//! It is made visible now and then: a pale line is drawn on the air,
//! runs along a ribbon with one curl in it and is gone. Each is a
//! pure function of [`Now`].

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, Face, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::load_shader_library;
use bevy::window::RequestRedraw;

use crate::{Now, Tints, VfxSystems};

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "wind.wesl");
    app.init_resource::<WindSettings>()
        .init_resource::<WindChoice>()
        .add_systems(Startup, prepare)
        .add_systems(Update, blow.in_set(VfxSystems));
}

/// The wind, and its lines.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindSettings {
    /// The least and most seconds between one gust and the next,
    /// with every particle the world may have.
    pub gap: (f32, f32),
    /// The way the wind blows, in degrees from north, and how far
    /// it veers to either side.
    pub heading: f32,
    pub veer: f32,
    /// Breadth in metres of the bands of strong wind that roll
    /// downwind, with no turbulence and with all of it, and their
    /// speed in metres a second.
    pub breadth: (f32, f32),
    pub speed: f32,
    /// The share of its full strength the wind keeps between gusts.
    pub calm: f32,
    /// The seconds from one lull of the whole wind to the next: it
    /// rises and falls everywhere at once, over the bands, between
    /// the least and the most of [`WindChoice`].
    pub swell: f32,
    /// Length and width of a gust's ribbon, and its height over the
    /// ground at its lowest and highest.
    pub length: f32,
    pub width: f32,
    pub height: (f32, f32),
    /// Seconds a line takes to run its ribbon, and the share of the
    /// ribbon it covers at a time.
    pub time: f32,
    pub dash: f32,
    /// Size of the curl, as a share of the ribbon's length.
    pub curl: f32,
    /// Brightness of a line by day, and the share of that it keeps
    /// by night.
    pub light: f32,
    pub night: f32,
}

/// How the chief has the wind blow, set on the Sight page and so
/// kept with the view.
#[derive(
    Resource,
    Clone,
    Copy,
    PartialEq,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct WindChoice {
    /// How hard the whole wind blows at the depth of a lull and at
    /// the height of a swell, as shares of the strength everything
    /// that sways is tuned to. It scales everything at once: the
    /// gusts and calms rolling across the ground keep their full
    /// range at any strength.
    pub least: f32,
    pub most: f32,
    /// From 0 to 1, how small its bands are: the more, the more
    /// plants side by side bend each as far as its own band blows.
    pub turbulence: f32,
}

impl WindChoice {
    /// The least and the most either end of the strength is set to.
    pub const STRENGTH: (f32, f32) = (0.0, 2.0);
}

impl WindSettings {
    /// The way the wind blows at `now`, in radians from north. It
    /// veers slowly from side to side.
    pub fn blowing(&self, now: f32) -> f32 {
        (self.heading + self.veer * 0.5 * (now * 0.11).sin())
            .to_radians()
    }

    /// How hard the wind blows at `at` on the ground at `now`: the
    /// gust or calm passing there, times how hard the whole wind
    /// blows as `choice` has it. The same as `strength` in
    /// `wind.wesl`.
    pub fn strength(
        &self,
        choice: WindChoice,
        at: Vec2,
        now: f32,
    ) -> f32 {
        self.uniform(choice).strength(at, now)
    }

    /// What the shaders are told of the wind, blowing as `choice`
    /// has it, from which they find it at any time.
    pub fn uniform(&self, choice: WindChoice) -> WindUniform {
        // Turbulence narrows the bands evenly on a log scale, so each
        // step of it halves them as often.
        let (calm, wild) =
            (self.breadth.0.max(0.1), self.breadth.1.max(0.1));
        let breadth = calm
            * (wild / calm).powf(choice.turbulence.clamp(0.0, 1.0));
        let across = 1.0 / breadth;
        // The bands roll along the wind's mean heading: by its
        // veering one, scaled by the time, they would swing ever
        // faster sideways.
        let mean = self.heading.to_radians();
        WindUniform {
            flow: Vec4::new(
                mean,
                (self.veer * 0.5).to_radians(),
                0.0,
                self.calm,
            ),
            scale: Vec4::new(
                across,
                self.speed * across,
                mean.sin(),
                mean.cos(),
            ),
            swell: Vec4::new(
                TAU / self.swell.max(1.0),
                choice.least,
                choice.most.max(choice.least),
                0.0,
            ),
        }
    }
}

/// Mirrors `Wind` in `wind.wesl`; the fields are explained there.
#[derive(ShaderType, Reflect, Debug, Clone, Default, PartialEq)]
pub struct WindUniform {
    pub flow: Vec4,
    pub scale: Vec4,
    pub swell: Vec4,
}

impl WindUniform {
    fn strength(&self, at: Vec2, now: f32) -> f32 {
        let rolled = Vec2::new(self.scale.z, self.scale.w)
            * (now * self.scale.y);
        let band = 0.5 + 0.5 * fbm(at * self.scale.x - rolled);
        let gust = smoothstep(0.3, 0.75, band);
        (self.flow.w + (1.0 - self.flow.w) * gust) * self.overall(now)
    }

    /// How hard the whole wind blows at `now`, from the least it is
    /// let blow at the depth of a lull to the most at the height of
    /// a swell. `overall` in `wind.wesl`.
    fn overall(&self, now: f32) -> f32 {
        let t = now * self.swell.x;
        let rise = 0.5 + 0.25 * (t.sin() + (t * 1.618 + 1.3).sin());
        self.swell.y
            + (self.swell.z - self.swell.y)
                * smoothstep(0.0, 1.0, rise)
    }
}

/// The seed of the wind's noise. `SEED` in `wind.wesl`.
const SEED: u32 = 7;

/// `scramble` in `noise.wesl`.
fn scramble(x: u32) -> u32 {
    let state =
        x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state)
        .wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// `gradient` in `noise.wesl`: one of sixteen directions.
fn gradient(x: i32, z: i32, seed: u32) -> Vec2 {
    let mut h = scramble(0x51ed_270b ^ seed);
    h = scramble(h ^ x as u32);
    h = scramble(h ^ z as u32);
    Vec2::from_angle((h & 15) as f32 * std::f32::consts::FRAC_PI_8)
}

/// `perlin` in `noise.wesl`, in about -1..1.
fn perlin(p: Vec2, seed: u32) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let (x, z) = (cell.x as i32, cell.y as i32);
    let fade = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = gradient(x, z, seed).dot(f);
    let b = gradient(x + 1, z, seed).dot(f - Vec2::X);
    let c = gradient(x, z + 1, seed).dot(f - Vec2::Y);
    let d = gradient(x + 1, z + 1, seed).dot(f - Vec2::ONE);
    a.lerp(b, fade.x).lerp(c.lerp(d, fade.x), fade.y) * 1.41
}

/// `fbm` in `noise.wesl` of two octaves, with the wind's seed.
fn fbm(p: Vec2) -> f32 {
    (perlin(p, SEED) + 0.5 * perlin(p * 2.03, SEED + 1)) / 1.5
}

fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The turn that lays something made along x down the wind that
/// blows toward `heading`.
pub(crate) fn downwind(heading: f32) -> Quat {
    // Along x; a heading of 0 blows north.
    Quat::from_rotation_y(heading - std::f32::consts::FRAC_PI_2)
}

/// The ribbon every gust is drawn with, and the dash that runs it.
#[derive(Resource)]
pub struct Breeze {
    ribbon: Handle<Mesh>,
    dash: Handle<Image>,
}

/// One gust: when it began, and its own material, whose dash moves.
#[derive(Component)]
struct Gust {
    began: f32,
    light: f32,
    line: Handle<StandardMaterial>,
}

/// Points along the ribbon, as shares of its length.
const POINTS: usize = 56;

/// The place of the ribbon at `t` of its way, for a ribbon one long
/// that runs along x. It rises a little, makes one curl in the plane
/// it travels in, and wanders to the side.
fn path(t: f32, curl: f32) -> Vec3 {
    let turn = TAU * ((t - 0.38) / 0.34).clamp(0.0, 1.0);
    let eased = turn - 0.5 * (turn * 2.0).sin() * 0.25;
    Vec3::new(
        t - curl * eased.sin(),
        curl * (1.0 - eased.cos()) + 0.05 * t,
        0.035 * (TAU * 0.8 * t).sin(),
    )
}

fn ribbon(set: &WindSettings) -> Mesh {
    let half = set.width / set.length.max(1e-3) * 0.5;
    let (mut at, mut uv) = (Vec::new(), Vec::new());
    for i in 0..=POINTS {
        let t = i as f32 / POINTS as f32;
        // It comes to a point at both ends.
        let taper = (t * (1.0 - t) * 4.0).sqrt();
        let p = path(t, set.curl);
        for side in [-1.0, 1.0] {
            at.push([p.x, p.y, p.z + side * half * taper]);
            uv.push([t, 0.5 + 0.5 * side]);
        }
    }
    let indices = (0..POINTS as u32)
        .flat_map(|i| {
            let a = i * 2;
            [a, a + 1, a + 2, a + 1, a + 3, a + 2]
        })
        .collect::<Vec<_>>();
    let normals = vec![[0.0, 1.0, 0.0]; at.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, at)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(Indices::U32(indices))
}

/// A soft white dash, clear at both ends and at both edges, so that
/// the ribbon past it draws nothing.
fn dash() -> Image {
    const W: usize = 64;
    const H: usize = 8;
    let mut data = Vec::with_capacity(W * H * 4);
    for y in 0..H {
        let across = (y as f32 + 0.5) / H as f32;
        let edge = (1.0 - (across * 2.0 - 1.0).abs()).powf(0.8);
        for x in 0..W {
            let along = x as f32 / (W - 1) as f32;
            // Faint behind, brightest near the front.
            let body = along.powf(1.6) * (1.0 - along).powf(0.35);
            let shown = (body * 2.2 * edge).clamp(0.0, 1.0);
            data.extend([255, 255, 255, (shown * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn prepare(
    mut commands: Commands,
    set: Res<WindSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    images: Option<ResMut<Assets<Image>>>,
) {
    // A world with nothing to draw on has no pictures.
    let Some(mut images) = images else {
        return;
    };
    commands.insert_resource(Breeze {
        ribbon: meshes.add(ribbon(&set)),
        dash: images.add(dash()),
    });
}

/// The place of the dash on a ribbon `u` of the way through its gust.
fn slide(set: &WindSettings, u: f32) -> Affine2 {
    let dash = set.dash.clamp(0.05, 1.0);
    // From wholly before the ribbon to wholly past it.
    let front = u * (1.0 + dash);
    Affine2::from_scale_angle_translation(
        Vec2::new(1.0 / dash, 1.0),
        0.0,
        Vec2::new(-(front - dash) / dash, 0.0),
    )
}

/// Sets off a gust that begins at `began` from `at`, blowing toward
/// `yaw`, `size` times its usual length and `light` times as bright.
pub fn gust(
    commands: &mut Commands,
    breeze: &Breeze,
    materials: &mut Assets<StandardMaterial>,
    set: &WindSettings,
    began: f32,
    at: Vec3,
    yaw: f32,
    size: f32,
    light: f32,
) {
    let line = materials.add(StandardMaterial {
        base_color: Color::NONE,
        base_color_texture: Some(breeze.dash.clone()),
        uv_transform: slide(set, 0.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None::<Face>,
        ..default()
    });
    commands.spawn((
        Gust {
            began,
            light,
            line: line.clone(),
        },
        Mesh3d(breeze.ribbon.clone()),
        MeshMaterial3d(line),
        Transform::from_translation(at)
            .with_rotation(downwind(yaw))
            .with_scale(Vec3::splat(set.length * size)),
        NotShadowCaster,
    ));
}

/// Runs each gust's line along its ribbon, and clears it away at
/// its end.
fn blow(
    mut commands: Commands,
    now: Res<Now>,
    set: Res<WindSettings>,
    tints: Res<Tints>,
    gusts: Query<(Entity, &Gust)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if gusts.is_empty() {
        return;
    }
    redraw.write(RequestRedraw);
    for (entity, gust) in &gusts {
        let u = (now.0 - gust.began) / set.time.max(1e-3);
        if u >= 1.0 {
            materials.remove(&gust.line);
            commands.entity(entity).try_despawn();
            continue;
        }
        if u < 0.0 {
            continue;
        }
        let Some(mut line) = materials.get_mut(&gust.line) else {
            continue;
        };
        // It comes out of nothing and goes back to it.
        let shown = (u * 6.0).min(1.0) * ((1.0 - u) * 4.0).min(1.0);
        line.base_color = Color::LinearRgba(
            LinearRgba::from_vec3(tints.wind)
                .with_alpha(set.light * gust.light * shown),
        );
        line.uv_transform = slide(&set, u);
    }
}
