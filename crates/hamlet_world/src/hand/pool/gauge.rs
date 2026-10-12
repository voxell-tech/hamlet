//! The meshes and the lights of the meters.

use bevy::prelude::*;

use crate::Stock;
use crate::hand::PlaySettings;
use crate::hand::toys::lit;
use crate::meshes::Mb;
use crate::palette::Palette;

/// The meshes and lights of a hamlet's meter.
#[derive(Resource)]
pub(in crate::hand) struct Gauge {
    pub(super) meter: Handle<Mesh>,
    pub(super) level: Handle<Mesh>,
    pub(super) pace: Handle<Mesh>,
    /// The light of a meter by how much is left, in [`SHADES`]
    /// steps from nothing to all of it.
    glow: Vec<Handle<StandardMaterial>>,
    /// The colour of each of those lights.
    tints: Vec<Color>,
    pub(super) matte: Handle<StandardMaterial>,
    pub(super) brass: Handle<StandardMaterial>,
}

/// Number of colours a meter's light passes through as it sinks.
const SHADES: usize = 24;

/// The colour of a meter's light with `left` of its budget, from 0
/// to 1: red when empty, through orange and yellow, to green when
/// full. `stops` are those four, evenly apart.
pub(in crate::hand) fn shade(stops: &[Vec3; 4], left: f32) -> Vec3 {
    let along = left.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let from = (along as usize).min(stops.len() - 2);
    stops[from].lerp(stops[from + 1], along - from as f32)
}

impl Gauge {
    fn step(left: f32) -> usize {
        (left.clamp(0.0, 1.0) * (SHADES - 1) as f32).round() as usize
    }

    /// The light of a meter with `left` of its budget.
    pub(super) fn light(
        &self,
        left: f32,
    ) -> Handle<StandardMaterial> {
        self.glow[Self::step(left)].clone()
    }

    /// The colour that light throws on what is about it.
    pub(super) fn tint(&self, left: f32) -> Color {
        self.tints[Self::step(left)]
    }
}

/// The height above the ground at which the light of a hamlet's meter
/// begins, and its height when the budget is whole.
pub(super) const COLUMN: (f32, f32) = (0.42, 2.9);

/// A round bar from `low` to `high`, `radius` wide.
fn drum(
    mb: &mut Mb,
    low: f32,
    high: f32,
    radius: f32,
    sides: usize,
    colour: Vec3,
) {
    mb.lathe(
        Vec3::Y * low,
        &[
            (0.0, 0.0),
            (radius, 0.0),
            (radius, high - low),
            (0.0, high - low),
        ],
        sides,
        |_, _| colour,
    );
}

/// The stand of a hamlet's meter, round so that it reads from every
/// side: a stone foot, a dark core the light stands about, three
/// brass rods with a ring at every tenth, and a cap.
fn meter(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    let (foot, tall) = COLUMN;
    let top = foot + tall;
    drum(&mut mb, 0.0, foot * 0.55, 0.62, 12, pal.limestone_shade);
    drum(&mut mb, foot * 0.55, foot, 0.48, 12, pal.limestone);
    drum(&mut mb, foot, top, 0.12, 8, pal.timber_dark * 0.45);
    for rod in 0..3 {
        let around = rod as f32 * std::f32::consts::TAU / 3.0 + 0.5;
        mb.rbox(
            Vec3::new(
                around.cos() * 0.4,
                foot + tall * 0.5,
                around.sin() * 0.4,
            ),
            Vec3::new(0.028, tall * 0.5, 0.028),
            0.01,
            Quat::from_rotation_y(-around),
            pal.brass,
            0.1,
        );
    }
    // A ring for each tenth, the middle one heavier.
    for tenth in 1..10 {
        let thick = if tenth == 5 { 0.03 } else { 0.014 };
        let at = foot + tall * tenth as f32 / 10.0;
        mb.lathe(
            Vec3::Y * (at - thick),
            &[
                (0.37, 0.0),
                (0.43, 0.0),
                (0.43, thick * 2.0),
                (0.37, thick * 2.0),
                (0.37, 0.0),
            ],
            12,
            |_, _| pal.brass * 1.1,
        );
    }
    drum(&mut mb, top, top + 0.12, 0.5, 12, pal.brass);
    mb.lathe(
        Vec3::Y * (top + 0.12),
        &[(0.0, 0.0), (0.3, 0.0), (0.12, 0.22), (0.0, 0.3)],
        10,
        |_, _| pal.brass * 1.1,
    );
    mb.build()
}

/// The light of a meter at its fullest, standing on the ground it
/// is scaled from.
fn level() -> Mesh {
    let mut mb = Mb::new();
    drum(&mut mb, 0.0, COLUMN.1, 0.3, 14, Vec3::ONE);
    mb.build()
}

/// The pointer that shows where the light would stand if the budget
/// were spent evenly: a heavy brass ring about the rods.
fn pace(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    mb.lathe(
        Vec3::Y * -0.05,
        &[
            (0.44, 0.0),
            (0.56, 0.0),
            (0.56, 0.1),
            (0.44, 0.1),
            (0.44, 0.0),
        ],
        14,
        |_, _| pal.brass * 1.35,
    );
    mb.build()
}

pub(in crate::hand) fn prepare(
    mut commands: Commands,
    pal: Res<Palette>,
    stock: Res<Stock>,
    set: Res<PlaySettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let shades = (0..SHADES).map(|step| {
        shade(&pal.meter, step as f32 / (SHADES - 1) as f32)
    });
    commands.insert_resource(Gauge {
        meter: meshes.add(meter(&pal)),
        level: meshes.add(level()),
        pace: meshes.add(pace(&pal)),
        glow: shades
            .clone()
            .map(|light| materials.add(lit(light, set.meter_glow)))
            .collect(),
        tints: shades
            .map(|light| Color::linear_rgb(light.x, light.y, light.z))
            .collect(),
        matte: stock.matte.clone(),
        brass: stock.brass.clone(),
    });
}
