//! Small magic drawn as meshes: a ring of runes that opens on the
//! ground, embers that rise, and motes that fly from one place to
//! another. Each is a pure function of [`Now`].

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::Face;
use bevy::window::RequestRedraw;

use crate::mark::{Lay, MarkMaterial, Marks, Shape, lie};
use crate::{Now, Tints, VfxSystems};

/// The world's small magic.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MagicSettings {
    /// The reach of a rune circle and of the embers' ground, and how
    /// far over the ground both lie.
    pub radius: f32,
    pub lift: f32,
    /// Seconds a rune circle lasts, and how bright it is.
    pub rune_time: f32,
    pub rune_light: f32,
    /// Number of embers a falling house gives off, the seconds over
    /// which they set out, and the seconds each lasts.
    pub embers: u32,
    pub ember_spread: f32,
    pub ember_time: f32,
    /// An ember's climb, its stray, its size and its brightness.
    pub ember_rise: f32,
    pub ember_sway: f32,
    pub ember_size: f32,
    pub ember_light: f32,
    /// The motes of each house of a town that is set down somewhere
    /// else: how many, the seconds they are on their way, and how
    /// high their arc is for the distance.
    pub move_motes: u32,
    pub move_time: f32,
    pub move_arc: f32,
    /// The motes of a house that is merged away: how many, the
    /// seconds they take to cross, how high they arc and how far
    /// they spread on the way, how high over the other house they
    /// land, and their size.
    pub motes: u32,
    pub flight_time: f32,
    pub flight_arc: f32,
    pub flight_spread: f32,
    pub flight_height: f32,
    pub mote_size: f32,
    /// The light over a thinking agent's house: how high, how big,
    /// how bright, how far and how fast it bobs, and the seconds it
    /// takes to grow.
    pub musing_height: f32,
    pub musing_size: f32,
    pub musing_light: f32,
    pub musing_bob: f32,
    pub musing_rate: f32,
    pub musing_in: f32,
}

/// The meshes and the shared lights of the magic.
#[derive(Resource)]
pub struct Wand {
    mote: Handle<Mesh>,
    ember: Handle<StandardMaterial>,
    musing: Handle<StandardMaterial>,
}

impl Wand {
    /// The round mesh every mote and ember is drawn with.
    pub fn mote(&self) -> Handle<Mesh> {
        self.mote.clone()
    }

    /// The light of a thought, which a flying mote also has.
    pub fn musing(&self) -> Handle<StandardMaterial> {
        self.musing.clone()
    }
}

/// One ring of a rune circle, when it began and which way it turns.
#[derive(Component)]
struct Rune {
    began: f32,
    turn: f32,
    at: Vec3,
    fill: Handle<MarkMaterial>,
}

/// An ember, when it rose, from where, and a number of its own.
#[derive(Component, Clone, Copy)]
pub struct Ember {
    pub began: f32,
    pub from: Vec3,
    pub own: f32,
}

/// A mote on its way from one place to another.
#[derive(Component, Clone, Copy)]
pub struct Flight {
    pub began: f32,
    pub from: Vec3,
    pub to: Vec3,
    /// A number of its own.
    pub own: f32,
    /// Seconds on its way, and how high its arc is.
    pub time: f32,
    pub arc: f32,
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<MagicSettings>()
        .add_systems(Startup, prepare)
        .add_systems(Update, cast.in_set(VfxSystems));
}

/// A colour that adds its light to what is behind it.
pub fn glow(colour: Vec3, strength: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::from_vec3(
            colour * strength,
        )),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None::<Face>,
        fog_enabled: false,
        ..default()
    }
}

fn prepare(
    mut commands: Commands,
    tints: Res<Tints>,
    set: Res<MagicSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(Wand {
        mote: meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
        ember: materials.add(glow(tints.ember, set.ember_light)),
        musing: materials.add(glow(tints.rune, set.musing_light)),
    });
}

/// A circle of runes that opens at `began` on the ground `lay`
/// tells of, at `ground`.
pub fn circle(
    commands: &mut Commands,
    marks: &Marks,
    materials: &mut Assets<MarkMaterial>,
    light: Vec3,
    ground: Vec3,
    lay: &Lay,
    began: f32,
) {
    for (shape, turn) in [(Shape::RING, 1.0), (Shape::HEX, -1.6)] {
        let fill = materials.add(marks.shine(shape, light, lay));
        commands.spawn((
            Rune {
                began,
                turn,
                at: ground,
                fill: fill.clone(),
            },
            marks.mark(fill),
            lie(ground, 0.0, 0.0),
        ));
    }
}

/// Sends a mote on its way.
pub fn fly(commands: &mut Commands, wand: &Wand, flight: Flight) {
    commands.spawn((
        flight,
        Mesh3d(wand.mote.clone()),
        MeshMaterial3d(wand.musing.clone()),
        Transform::from_translation(flight.from)
            .with_scale(Vec3::ZERO),
        NotShadowCaster,
    ));
}

/// Raises an ember drawn as a mesh, for a world with no particles.
pub fn rise(commands: &mut Commands, wand: &Wand, ember: Ember) {
    commands.spawn((
        ember,
        Mesh3d(wand.mote.clone()),
        MeshMaterial3d(wand.ember.clone()),
        Transform::from_translation(ember.from)
            .with_scale(Vec3::ZERO),
        NotShadowCaster,
    ));
}

/// A number from 0 to 1 that depends only on `n`.
pub fn chance(n: f32) -> f32 {
    (n * 12.9898 + 4.1414)
        .sin()
        .mul_add(43_758.547, 0.0)
        .fract()
        .abs()
}

/// Plays the runes, the embers and the motes, and clears each away
/// at its end.
fn cast(
    mut commands: Commands,
    now: Res<Now>,
    set: Res<MagicSettings>,
    tints: Res<Tints>,
    mut runes: Query<
        (Entity, &Rune, &mut Transform),
        (Without<Ember>, Without<Flight>),
    >,
    mut embers: Query<
        (Entity, &Ember, &mut Transform),
        (Without<Rune>, Without<Flight>),
    >,
    mut flights: Query<
        (Entity, &Flight, &mut Transform),
        (Without<Rune>, Without<Ember>),
    >,
    mut materials: ResMut<Assets<MarkMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if runes.is_empty() && embers.is_empty() && flights.is_empty() {
        return;
    }
    redraw.write(RequestRedraw);
    for (entity, rune, mut at) in &mut runes {
        let u = (now.0 - rune.began) / set.rune_time;
        if !(0.0..1.0).contains(&u) {
            if u >= 1.0 {
                // The heights it lay on go with it.
                if let Some(fill) = materials.remove(&rune.fill) {
                    images.remove(fill.heights());
                }
                commands.entity(entity).try_despawn();
            }
            continue;
        }
        // It opens fast, holds, and is gone as the house settles.
        let open = 1.0 - (1.0 - (u * 3.0).min(1.0)).powi(3);
        let fade = (1.0 - u).powf(1.5) * (u * 8.0).min(1.0);
        *at = lie(
            rune.at,
            set.radius * (0.55 + 0.45 * open),
            u * rune.turn,
        );
        if let Some(mut fill) = materials.get_mut(&rune.fill) {
            fill.light(tints.rune * set.rune_light * fade);
        }
    }
    for (entity, flight, mut at) in &mut flights {
        // Each sets out a little after the last and arrives with
        // the rest.
        let wait = chance(flight.own + 0.25) * flight.time * 0.35;
        let u = (now.0 - flight.began - wait) / (flight.time - wait);
        if u >= 1.0 {
            commands.entity(entity).try_despawn();
            continue;
        }
        if u < 0.0 {
            continue;
        }
        let along = u * u * (3.0 - 2.0 * u);
        let arc = (u * std::f32::consts::PI).sin();
        let side = Vec3::new(
            (flight.own * 3.1).sin(),
            0.0,
            (flight.own * 2.3).cos(),
        ) * set.flight_spread;
        at.translation = flight.from.lerp(flight.to, along)
            + Vec3::Y * flight.arc * arc
            + side * arc;
        at.scale = Vec3::splat(set.mote_size * arc.sqrt());
    }
    for (entity, ember, mut at) in &mut embers {
        // Each sets out a little after the last.
        let wait = chance(ember.own + 0.25) * set.ember_spread;
        let u = (now.0 - ember.began - wait) / set.ember_time;
        if u >= 1.0 {
            commands.entity(entity).try_despawn();
            continue;
        }
        if u < 0.0 {
            continue;
        }
        let sway = Vec3::new(
            (u * 5.0 + ember.own).sin(),
            0.0,
            (u * 4.0 + ember.own * 1.7).cos(),
        ) * set.ember_sway;
        let up = Vec3::Y
            * set.ember_rise
            * (0.6 + 0.4 * chance(ember.own + 0.75))
            * (1.0 - (1.0 - u).powi(2));
        at.translation = ember.from + up + sway * u;
        // It flares as it leaves the ground and dies on the way up.
        let size = (u * 6.0).min(1.0) * (1.0 - u);
        at.scale = Vec3::splat(set.ember_size * size);
    }
}
