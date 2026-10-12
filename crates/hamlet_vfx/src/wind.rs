//! Wind made visible: now and then a pale line is drawn on the air,
//! runs along a ribbon with one curl in it and is gone. Each is a
//! pure function of [`Now`].

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, Face, TextureDimension, TextureFormat,
};
use bevy::window::RequestRedraw;

use crate::{Now, Tints, VfxSystems};

/// The wind's lines.
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

impl WindSettings {
    /// The way the wind blows at `now`, in radians from north. It
    /// veers slowly from side to side.
    pub(crate) fn blowing(&self, now: f32) -> f32 {
        (self.heading + self.veer * 0.5 * (now * 0.11).sin())
            .to_radians()
    }
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

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<WindSettings>()
        .add_systems(Startup, prepare)
        .add_systems(Update, blow.in_set(VfxSystems));
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
