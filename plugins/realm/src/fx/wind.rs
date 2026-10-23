//! The wind made visible: now and then a pale line is drawn on the
//! air over the ground the camera looks at, runs along a ribbon with
//! one curl in it and is gone.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Quat, Vec2, Vec3};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    AlphaMode, Material, Mesh, Shaded, Shader, ShaderKind,
};

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::upload::Draw;
use crate::fx::{chance, ready};
use crate::realm::Clock;
use crate::tuning::{VfxTuning, WorldTuning};
use crate::view::camera::Camera;
use crate::world::land::Land;
use crate::world::palette::Palette;
use crate::world::sky::{Quality, Sky};

/// Points along the ribbon, as shares of its length.
const POINTS: usize = 56;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Gusts>()
        .add_systems(Update, blow.run_if(ready));
}

/// The wind's lines.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindSettings {
    /// The least and most seconds between one gust and the next,
    /// with every particle the realm may have.
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

/// The gusts in the air, the time the next is due and the count of
/// gusts so far, and the ribbon and shader every gust is drawn with
/// once they are made.
#[derive(Resource, Default)]
struct Gusts {
    breeze: Option<Breeze>,
    playing: Vec<Gust>,
    due: f32,
    count: u32,
}

struct Breeze {
    ribbon: Handle<Mesh>,
    shader: Handle<Shader>,
}

/// One gust: when it began, how bright it is, and its own material,
/// whose dash moves.
struct Gust {
    began: f32,
    light: f32,
    entity: Entity,
    line: Handle<Material>,
}

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

fn ribbon(set: &WindSettings) -> Mb {
    let half = set.width / set.length.max(1e-3) * 0.5;
    let mut mb = Mb::new();
    for i in 0..=POINTS {
        let t = i as f32 / POINTS as f32;
        // It comes to a point at both ends.
        let taper = (t * (1.0 - t) * 4.0).sqrt();
        let p = path(t, set.curl);
        for side in [-1.0, 1.0] {
            mb.vuv(
                p + Vec3::Z * side * half * taper,
                Vec3::Y,
                Vec3::ONE,
                Vec2::new(t, 0.5 + 0.5 * side),
            );
        }
    }
    for i in 0..POINTS as u32 {
        let a = i * 2;
        mb.quad(a, a + 1, a + 3, a + 2);
    }
    mb
}

impl Breeze {
    fn make(
        set: &WindSettings,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        Ok(Self {
            ribbon: draw
                .assets
                .add(Mesh::build(&ribbon(set).data())?),
            shader: draw.assets.add(Shader::compile(
                ShaderKind::Material,
                include_str!("wind.wgsl"),
            )?),
        })
    }
}

/// The params of a gust's line, `u` of the way through it.
fn params(
    set: &WindSettings,
    colour: Vec3,
    light: f32,
    u: f32,
) -> [f32; 8] {
    let dash = set.dash.clamp(0.05, 1.0);
    // It comes out of nothing and goes back to it.
    let shown = (u * 6.0).min(1.0) * ((1.0 - u) * 4.0).min(1.0);
    [
        colour.x,
        colour.y,
        colour.z,
        set.light * light * shown,
        // From wholly before the ribbon to wholly past it.
        u * (1.0 + dash),
        dash,
        0.0,
        0.0,
    ]
}

/// Sets off a gust when one is due, runs each gust's line along its
/// ribbon, and clears it away at its end. A realm with no particles
/// has no wind to see.
fn blow(
    tuning: Res<WorldTuning>,
    vfx: Res<VfxTuning>,
    pal: Res<Palette>,
    quality: Res<Quality>,
    clock: Res<Clock>,
    camera: Res<Camera>,
    land: Res<Land>,
    sky: Res<Sky>,
    mut gusts: ResMut<Gusts>,
    mut draw: Draw,
) {
    let set = &vfx.wind;
    let gusts = &mut *gusts;
    let now = clock.now;
    gusts.playing.retain(|gust| {
        let u = (now - gust.began) / set.time.max(1e-3);
        if u >= 1.0 {
            draw.despawn(gust.entity);
            return false;
        }
        let params = params(set, pal.wind, gust.light, u.max(0.0));
        if let Err(why) = draw.assets.set_params(&gust.line, &params)
        {
            hamlet_engine_bevy::warn(&format!("wind: {why}"));
        }
        true
    });
    let density = quality.density();
    if density <= 0.0 || now < gusts.due {
        return;
    }
    let breeze = match &gusts.breeze {
        Some(breeze) => breeze,
        None => match Breeze::make(set, &mut draw) {
            Ok(breeze) => gusts.breeze.insert(breeze),
            Err(why) => {
                hamlet_engine_bevy::warn(&format!("wind: {why}"));
                gusts.due = f32::INFINITY;
                return;
            }
        },
    };
    let own = gusts.count as f32 * 7.31;
    let roll = |salt: f32| chance(own + salt);
    gusts.count += 1;
    gusts.due = now
        + (set.gap.0 + (set.gap.1 - set.gap.0) * roll(0.1)) / density;

    // Somewhere in what the camera sees, larger from further off so
    // that it still shows.
    let (view, _) = camera.views();
    let size = (view.dist / 70.0).clamp(0.7, 7.0);
    let reach = view.dist * 0.55;
    let over = view.focus
        + Vec2::new(roll(0.2) - 0.5, roll(0.3) - 0.5) * 2.0 * reach;
    let height =
        set.height.0 + (set.height.1 - set.height.0) * roll(0.4);
    let at = land.on_ground(over) + Vec3::Y * height * size;
    let yaw = (set.heading + set.veer * (roll(0.5) * 2.0 - 1.0))
        .to_radians();
    let size = size * (0.8 + 0.4 * roll(0.6));
    let light =
        set.night + (1.0 - set.night) * tuning.sky.daylight(sky.hour);
    let line = match draw.assets.add_shaded_material(
        &breeze.shader,
        Shaded {
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            lit: false,
        },
        &params(set, pal.wind, light, 0.0),
        None,
        None,
    ) {
        Ok(line) => line,
        Err(why) => {
            hamlet_engine_bevy::warn(&format!("wind: {why}"));
            return;
        }
    };
    let entity = draw.spawn(None, 0);
    // Made along x; a heading of 0 blows north.
    draw.place(
        entity,
        Frame::at(at)
            .with_turn(Quat::from_rotation_y(yaw - FRAC_PI_2))
            .with_size(Vec3::splat(set.length * size)),
    );
    draw.draw(entity, &breeze.ribbon, &line);
    draw.shadows(entity, false, false);
    gusts.playing.push(Gust {
        began: now,
        light,
        entity,
        line,
    });
}
