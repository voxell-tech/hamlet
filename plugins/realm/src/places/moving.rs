//! A place picked up and set down somewhere else
//! (docs/endless-world.md): a town, the chief's Hearth or the
//! library. Setting down changes nothing here: it is told as a
//! [`SetDown`], and a town is moved when the roster says it moved.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    Material, Mesh, Shape, StandardMaterial,
};

use crate::build::frame::Frame;
use crate::build::stock::rgba;
use crate::build::upload::Draw;
use crate::society::{Place, Society};
use crate::world::land::{Land, Unfit};
use crate::world::layout::plot;
use crate::world::palette::Palette;

/// The key of the footprint's entities.
const FOOTPRINT: u32 = 6;

/// A place in a chief's hand: how the footprint that shows where it
/// would stand is drawn. Its words stand over its middle.
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

/// A place in a chief's hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carry {
    pub place: Place,
    /// The ground under the pointer, while the pointer is over any.
    pub to: Option<Vec2>,
    /// `Ok` when it could be set down there.
    pub fits: Result<(), Unfit>,
}

/// The place in a chief's hand, and where one was set down, until
/// the realm's frame tells it.
#[derive(Resource, Default)]
pub struct Carrying {
    pub carry: Option<Carry>,
    pub set_down: Option<SetDown>,
}

/// A chief set a place down at `to`, where it fits, written early in
/// the realm's frame. A town is moved by its hamlet's command, the
/// Hearth and the library by whoever draws them.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct SetDown {
    pub place: Place,
    pub to: Vec2,
}

/// A part of the footprint.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    /// One of the lights about the round of ground the place holds.
    Rim(u32),
    /// A town's common.
    Common,
    /// A light that stands over its middle.
    Beam,
    /// The ground one house of a town takes.
    House(u32),
}

/// The footprint of a carried place, where it would stand.
#[derive(Resource)]
pub struct Footprint {
    root: Entity,
    /// Each part, the mesh it draws, and whether it is lit as
    /// fitting while it is shown.
    marks: Vec<(Mark, Entity, usize, Option<bool>)>,
    /// The few words on what a click would do, over its middle.
    words: Entity,
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
    /// `place` in hand, not yet over the ground.
    pub fn new(place: Place) -> Self {
        Self {
            place,
            to: None,
            fits: Err(Unfit::Crowded),
        }
    }

    /// Follows the ray from the eye with the place, and finds
    /// whether it fits where it meets the ground.
    pub fn reach(
        &mut self,
        land: &Land,
        society: &Society,
        ray: Option<(Vec3, Vec3)>,
    ) {
        self.to =
            ray.and_then(|(eye, way)| ground_under(land, eye, way));
        self.fits = match self.to {
            Some(to) => society.fits(land, self.place, to),
            None => Err(Unfit::Crowded),
        };
    }

    /// Where it would be set down: where it is held, if it fits
    /// there.
    pub fn set_down(&self) -> Option<SetDown> {
        let to = self.to.filter(|_| self.fits.is_ok())?;
        Some(SetDown {
            place: self.place,
            to,
        })
    }
}

impl Footprint {
    pub fn make(
        land: &Land,
        pal: &Palette,
        set: &CarrySettings,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let light = |colour: Vec3| StandardMaterial {
            emissive: rgba(colour * set.glow),
            ..StandardMaterial::colored((0.0, 0.0, 0.0, 1.0))
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
        let words = draw.spawn(Some(root), FOOTPRINT);
        draw.place(words, Frame::at(Vec3::Y * set.beam.1));
        Ok(Self {
            root,
            marks,
            words,
            meshes,
            fits,
            unfit,
            shown: false,
            said: None,
        })
    }

    /// Draws the footprint where `carry` would stand: its rim, a
    /// light over its middle, and for a town its common and the
    /// ground of each of its houses, with a few words on what a click
    /// would do. With the pointer off the ground, the words alone
    /// stand over the place where it is. Hidden with nothing in
    /// hand.
    pub fn show(
        &mut self,
        carry: Option<&Carry>,
        land: &Land,
        society: &Society,
        set: &CarrySettings,
        draw: &mut Draw,
    ) {
        let shown = carry.and_then(|carry| {
            let (at, radius) = society.stands(land, carry.place)?;
            Some((carry, at, radius))
        });
        let Some((carry, at, radius)) = shown else {
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
        let to = carry.to.unwrap_or(at);
        let middle = Vec3::new(to.x, over(to), to.y);
        draw.place(self.root, Frame::at(middle));
        let town = match carry.place {
            Place::Town(id) => society.town(id),
            Place::Chief | Place::Library => None,
        };
        // The plots as they would lie there.
        let there = town.map(|town| {
            let mut there = land.clone();
            there.move_site(town.site, to);
            (there, town)
        });
        for (mark, entity, mesh, lit) in &mut self.marks {
            let mut good = carry.fits.is_ok();
            let place = match *mark {
                _ if carry.to.is_none() => None,
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
                Mark::Common => town.map(|_| {
                    let r = land.set.town.plaza_radius;
                    Frame::IDENTITY.with_size(Vec3::new(r, 1.0, r))
                }),
                Mark::House(k) => there
                    .as_ref()
                    .filter(|(_, town)| {
                        society.agents.iter().any(|a| {
                            a.town == town.id
                                && a.plot == k
                                && !a.cleared
                        })
                    })
                    .map(|(there, town)| {
                        let spot = plot(there, town.site, k);
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
        let says = match (carry.to, carry.fits) {
            (None, _) => "Point at the ground",
            (_, Ok(())) => {
                "Click to set down. Right-click to put back"
            }
            (_, Err(Unfit::Water)) => "Not in the water",
            (_, Err(Unfit::Steep)) => "Too steep here",
            (_, Err(Unfit::Crowded)) => "Too close to another place",
        };
        if self.said != Some(says) {
            draw.commands
                .entity(self.words)
                .insert(SceneLabel(says.to_owned()));
            self.said = Some(says);
        }
    }
}
