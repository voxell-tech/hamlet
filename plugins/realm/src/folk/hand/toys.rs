//! The small things a chief's hand leaves in the realm: a heart or a
//! spark over a robot.

use glam::{Quat, Vec3};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Material, Mesh, Pbr, Shape};

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::stock::rgba;
use crate::build::upload::{Draw, Unsent};
use crate::folk::hand::{HandFrame, PlaySettings};
use crate::world::palette::Palette;

/// The kind of a toy, which says how it looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    /// Rises over a robot at rest and fades.
    Heart,
    /// Rises over a robot at work, or one cheered, and fades.
    Spark,
}

/// One toy, when it began and where it goes.
struct Toy {
    entity: Entity,
    began: f32,
    from: Vec3,
    to: Vec3,
    own: f32,
}

/// The meshes and lights of the toys, made once they are first
/// tossed, and the toys in the realm.
#[derive(Resource, Default)]
pub struct Toys {
    heart: Option<Handle<Mesh>>,
    spark: Option<Handle<Mesh>>,
    warm: Option<Handle<Material>>,
    cool: Option<Handle<Material>>,
    /// Toys tossed and yet to be put in the realm.
    tossed: Vec<(Kind, f32, (Vec3, Vec3), f32)>,
    toys: Vec<Toy>,
}

/// A light that glows in its own colour.
fn lit(colour: Vec3, strength: f32) -> Pbr {
    Pbr {
        emissive: rgba(colour * strength),
        roughness: 0.6,
        ..Pbr::colored(rgba(colour))
    }
}

fn heart() -> Mb {
    let mut mb = Mb::new();
    let white = |_: Vec3| Vec3::ONE;
    for side in [-1.0f32, 1.0] {
        mb.blob(
            Vec3::new(side * 0.42, 0.5, 0.0),
            Vec3::new(0.52, 0.5, 0.36),
            5,
            0.0,
            0.0,
            white,
        );
    }
    mb.lathe(
        Vec3::new(0.0, -0.75, 0.0),
        &[(0.0, 0.0), (0.5, 0.62), (0.86, 1.2), (0.0, 1.3)],
        10,
        |_, _| Vec3::ONE,
    );
    mb
}

impl Toys {
    /// Sets a toy of `kind` going at clock time `began`, from `from`
    /// to `to`. `own` tells toys of one moment apart.
    pub(super) fn toss(
        &mut self,
        kind: Kind,
        began: f32,
        way: (Vec3, Vec3),
        own: f32,
    ) {
        self.tossed.push((kind, began, way, own));
    }

    /// Makes the meshes and lights once a toy is tossed, as the call
    /// has room for them.
    pub(super) fn hand_over(
        &mut self,
        pal: &Palette,
        draw: &mut Draw,
    ) {
        if self.tossed.is_empty() || self.heart.is_some() {
            return;
        }
        let made = (|| {
            let spark = Mesh::shape(Shape::Sphere(0.6))?;
            Ok::<_, String>((
                draw.assets.add(spark),
                draw.material(lit(pal.wax, 1.6))?,
                draw.material(lit(pal.rune, 3.0))?,
            ))
        })();
        match (made, draw.mesh(heart().data())) {
            (Ok((spark, warm, cool)), Ok(heart)) => {
                self.heart = Some(heart);
                self.spark = Some(spark);
                self.warm = Some(warm);
                self.cool = Some(cool);
            }
            (_, Err(Unsent::Full(_))) => {}
            (Err(why), _) | (_, Err(Unsent::Refused(why))) => {
                hamlet_engine_bevy::warn(&format!("a toy: {why}"));
                self.tossed.clear();
            }
        }
    }

    /// Plays every toy and clears each away at its end.
    pub(super) fn play(
        &mut self,
        frame: &HandFrame,
        draw: &mut Draw,
    ) {
        if let (Some(heart), Some(spark), Some(warm), Some(cool)) =
            (&self.heart, &self.spark, &self.warm, &self.cool)
        {
            for (kind, began, (from, to), own) in
                self.tossed.drain(..)
            {
                let (mesh, fill) = match kind {
                    Kind::Heart => (heart, warm),
                    Kind::Spark => (spark, cool),
                };
                let entity = draw.spawn(None, 0);
                draw.draw(entity, mesh, fill);
                draw.shadows(entity, false, true);
                draw.place(
                    entity,
                    Frame::at(from).with_size(Vec3::ZERO),
                );
                self.toys.push(Toy {
                    entity,
                    began,
                    from,
                    to,
                    own,
                });
            }
        }
        self.toys.retain(|toy| {
            match placed(toy, frame.now - toy.began, frame.set) {
                Some(at) => {
                    draw.place(toy.entity, at);
                    true
                }
                None => {
                    draw.despawn(toy.entity);
                    false
                }
            }
        });
    }
}

/// The place and size of a toy `t` seconds after it began. None once
/// it is over.
fn placed(toy: &Toy, t: f32, set: &PlaySettings) -> Option<Frame> {
    let u = t / set.heart_time;
    if u >= 1.0 {
        return None;
    }
    // It pops out, drifts up with a sway and thins away.
    let pop = (u * 5.0).clamp(0.0, 1.0);
    let size = (1.0 - (1.0 - pop).powi(3))
        * (1.0 - ((u - 0.6) / 0.4).clamp(0.0, 1.0).powi(2));
    let sway = Vec3::new((t * 5.0 + toy.own).sin(), 0.0, 0.0) * 0.06;
    let at = toy.from.lerp(toy.to, 1.0 - (1.0 - u).powi(2));
    Some(
        Frame::at(at + sway)
            .with_turn(Quat::from_rotation_y(
                (t * 2.6 + toy.own).sin() * 0.6,
            ))
            .with_size(Vec3::splat(set.heart_size * size)),
    )
}
