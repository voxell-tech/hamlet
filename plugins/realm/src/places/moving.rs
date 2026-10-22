//! A town picked up and set down somewhere else
//! (docs/endless-world.md). Setting it down changes nothing here: it
//! is told to the town's hamlet as a chief's command, and the realm
//! moves the town when the roster says it moved.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Material, Mesh, Pbr, Shape};

use crate::build::frame::Frame;
use crate::build::stock::rgba;
use crate::build::upload::Draw;
use crate::society::{Place, Society, TownId};
use crate::world::land::{Land, Unfit};
use crate::world::layout::plot;
use crate::world::palette::Palette;

/// The key of the footprint's entities.
const FOOTPRINT: u32 = 6;

/// A place in a chief's hand: how the footprint that shows where it
/// would stand is drawn. Its words stand over its middle, so those of
/// the words beside the pointer are not read.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
pub struct CarrySettings {
    /// Height of the footprint over the ground.
    pub lift: f32,
    /// Strength of its glow, over its colour.
    pub glow: f32,
    /// The lights its rim is made of: how many, and how wide each
    /// is.
    pub beads: u32,
    pub bead: f32,
    /// The width of the ring that marks a common, as a share of the
    /// ring's radius.
    pub band: f32,
    /// Radius and height of the light over its middle.
    pub beam: (f32, f32),
    /// Thickness of the mark where a house would stand.
    pub pad: f32,
}

/// A town in a chief's hand.
#[derive(Clone, Debug, PartialEq)]
pub struct Carry {
    pub town: TownId,
    /// The hamlet's name on disk.
    pub hamlet: String,
    /// The ground under the pointer, while the pointer is over any.
    pub to: Option<Vec2>,
    /// `Ok` when it could be set down there.
    pub fits: Result<(), Unfit>,
}

/// A part of the footprint.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    /// One of the lights about the round of ground the town holds.
    Rim(u32),
    /// The town's common.
    Common,
    /// A light that stands over its middle.
    Beam,
    /// The ground one house of the town takes.
    House(u32),
}

/// The town in a chief's hand, and the command that set one down,
/// until it is sent: the hamlet and the command.
#[derive(Resource, Default)]
pub struct Carrying {
    pub carry: Option<Carry>,
    pub set_down: Option<(String, String)>,
}

/// The footprint of a carried town, where it would stand.
#[derive(Resource)]
pub struct Footprint {
    root: Entity,
    /// Each part, the mesh it draws, and whether it is lit as
    /// fitting while it is shown.
    marks: Vec<(Mark, Entity, usize, Option<bool>)>,
    meshes: [Handle<Mesh>; 3],
    fits: Handle<Material>,
    unfit: Handle<Material>,
    shown: bool,
    said: Option<&'static str>,
}

/// The point the ray from `eye` along `way` meets the ground at.
fn ground_under(land: &Land, eye: Vec3, way: Vec3) -> Option<Vec2> {
    let down = -way.y;
    if down < 0.02 {
        return None;
    }
    // From level ground, a few steps toward the ground as it lies.
    let mut far = (eye.y - land.set.land.site_y) / down;
    for _ in 0..4 {
        let p = eye + way * far;
        far = (eye.y - land.height(p.xz())) / down;
    }
    (far > 0.0).then(|| (eye + way * far).xz())
}

impl Carry {
    /// Follows the ray from the eye with the town, and finds whether
    /// it fits where it meets the ground.
    pub fn reach(
        &mut self,
        land: &Land,
        society: &Society,
        ray: Option<(Vec3, Vec3)>,
    ) {
        self.to =
            ray.and_then(|(eye, way)| ground_under(land, eye, way));
        self.fits = match self.to {
            Some(to) => {
                society.fits(land, Place::Town(self.town), to)
            }
            None => Err(Unfit::Crowded),
        };
    }

    /// The chief's command that sets it down where it is held.
    pub fn command(&self) -> Option<String> {
        let to = self.to.filter(|_| self.fits.is_ok())?;
        Some(format!(
            "/move-hamlet {} {:.1} {:.1}",
            self.hamlet, to.x, to.y
        ))
    }
}

impl Footprint {
    pub fn make(
        land: &Land,
        pal: &Palette,
        set: &CarrySettings,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let light = |colour: Vec3| Pbr {
            emissive: rgba(colour * set.glow),
            ..Pbr::colored((0.0, 0.0, 0.0, 1.0))
        };
        let fits = draw.material(light(pal.place_fits))?;
        let unfit = draw.material(light(pal.place_unfit))?;
        let pad = Mesh::shape(Shape::Cylinder((1.0, set.pad)))?;
        let band = Mesh::shape(Shape::Torus((
            set.band * 0.5,
            1.0 - set.band * 0.5,
        )))?;
        let beam = Mesh::shape(Shape::Cylinder(set.beam))?;
        let meshes =
            [pad, band, beam].map(|mesh| draw.assets.add(mesh));
        let root = draw.spawn(None, FOOTPRINT);
        draw.show(root, false);
        let marks = (0..set.beads)
            .map(Mark::Rim)
            .chain([Mark::Common, Mark::Beam])
            .chain((0..land.set.town.plots).map(Mark::House))
            .map(|mark| {
                let entity = draw.spawn(Some(root), FOOTPRINT);
                let mesh = match mark {
                    Mark::Rim(_) | Mark::House(_) => 0,
                    Mark::Common => 1,
                    Mark::Beam => 2,
                };
                draw.show(entity, false);
                draw.shadows(entity, false, false);
                (mark, entity, mesh, None)
            })
            .collect::<Vec<_>>();
        Ok(Self {
            root,
            marks,
            meshes,
            fits,
            unfit,
            shown: false,
            said: None,
        })
    }

    /// Draws the footprint where `carry` would stand: its rim, a
    /// light over its middle, its common and the ground of each
    /// of its houses, with a few words on what a click would do.
    /// Hidden with nothing in hand.
    pub fn show(
        &mut self,
        carry: Option<&Carry>,
        land: &Land,
        society: &Society,
        set: &CarrySettings,
        draw: &mut Draw,
    ) {
        let shown = carry.and_then(|carry| {
            let (_, radius) =
                society.stands(land, Place::Town(carry.town))?;
            let town = society.town(carry.town)?;
            Some((carry, carry.to?, radius, town))
        });
        let Some((carry, to, radius, town)) = shown else {
            if self.shown {
                draw.show(self.root, false);
                self.shown = false;
            }
            return;
        };
        if !self.shown {
            draw.show(self.root, true);
            self.shown = true;
        }
        let level = land.set.land.site_y;
        // Over the ground as it lies now and as it would be levelled.
        let over = |p: Vec2| land.surface(p).max(level) + set.lift;
        let middle = Vec3::new(to.x, over(to), to.y);
        draw.place(self.root, Frame::at(middle));
        // The plots as they would lie there.
        let mut there = land.clone();
        there.move_site(town.site, to);
        for (mark, entity, mesh, lit) in &mut self.marks {
            let mut good = carry.fits.is_ok();
            let place = match *mark {
                // Each lies on the ground under it, so the rim shows
                // on a slope too.
                Mark::Rim(i) => {
                    let turn = i as f32 / set.beads as f32 * TAU;
                    let out = Vec2::from_angle(turn) * radius;
                    Some(
                        Frame::at(Vec3::new(
                            out.x,
                            over(to + out) - middle.y,
                            out.y,
                        ))
                        .with_size(
                            Vec3::new(set.bead, 1.0, set.bead),
                        ),
                    )
                }
                Mark::Beam => {
                    Some(Frame::at(Vec3::Y * set.beam.1 * 0.5))
                }
                Mark::Common => {
                    let r = land.set.town.plaza_radius;
                    Some(
                        Frame::IDENTITY
                            .with_size(Vec3::new(r, 1.0, r)),
                    )
                }
                Mark::House(k) => society
                    .agents
                    .iter()
                    .any(|a| {
                        a.town == town.id && a.plot == k && !a.cleared
                    })
                    .then(|| {
                        let spot = plot(&there, town.site, k);
                        good &= spot.usable;
                        let r = land.set.town.house_radius;
                        Frame::at(Vec3::new(
                            spot.at.x - to.x,
                            over(spot.at) - middle.y,
                            spot.at.y - to.y,
                        ))
                        .with_size(Vec3::new(r, 1.0, r))
                    }),
            };
            match place {
                Some(place) => {
                    draw.place(*entity, place);
                    if *lit != Some(good) {
                        let light = if good {
                            &self.fits
                        } else {
                            &self.unfit
                        };
                        draw.show(*entity, true);
                        draw.draw(
                            *entity,
                            &self.meshes[*mesh],
                            light,
                        );
                        *lit = Some(good);
                    }
                }
                None => {
                    if lit.is_some() {
                        draw.show(*entity, false);
                        *lit = None;
                    }
                }
            }
        }
        let says = match carry.fits {
            Ok(()) => "Click to set down. Right-click to put back",
            Err(Unfit::Water) => "Not in the water",
            Err(Unfit::Steep) => "Too steep here",
            Err(Unfit::Crowded) => "Too close to another place",
        };
        if self.said != Some(says) {
            if let Some((_, beam, ..)) = self
                .marks
                .iter()
                .find(|(mark, ..)| *mark == Mark::Beam)
            {
                draw.commands
                    .entity(*beam)
                    .insert(SceneLabel(says.to_owned()));
            }
            self.said = Some(says);
        }
    }
}
