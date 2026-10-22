//! A house coming into the realm and going out of it
//! (docs/robots.md): standing up, shuddering and falling. Only
//! transforms and visibility move; the meshes stay as built.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec3};

use crate::build::frame::Frame;

/// The coming in and going out of a house.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HouseMotion {
    /// Seconds a house, or a ruin, takes to stand up.
    pub rise: f32,
    /// About how far past its height it springs before it settles,
    /// as a share of the height.
    pub overshoot: f32,
    /// Extra width while it lies flat, as a share of its width.
    pub squash: f32,
    /// Seconds into the rise at which the first thing in the yard
    /// pops up and the robot appears.
    pub props_after: f32,
    /// Seconds between one thing in the yard and the next.
    pub prop_gap: f32,
    /// Seconds a thing in the yard takes to pop up.
    pub prop: f32,
    /// Seconds a ruin takes to sink under a house that rises.
    pub clear: f32,
    /// Seconds a house shudders before it falls.
    pub shudder: f32,
    /// The furthest it tips while it shudders, in radians.
    pub tilt: f32,
    /// Shakes per second.
    pub shake: f32,
    /// Seconds it takes to fall.
    pub fall: f32,
    /// Seconds after the shudder begins at which what it rests as
    /// starts to stand up.
    pub rest_after: f32,
    /// A town that is set down somewhere else: seconds before its
    /// first house stands up there, and from one house to the next.
    pub move_after: f32,
    pub move_gap: f32,
}

impl HouseMotion {
    /// Seconds from the first move of a house coming in to the last.
    pub fn time_in(&self, props: usize) -> f32 {
        let last = props.saturating_sub(1) as f32 * self.prop_gap;
        self.rise.max(self.props_after + last + self.prop)
    }

    /// The scale of yard thing `order`, `t` seconds into its house's
    /// rise.
    pub fn popped(&self, t: f32, order: usize) -> f32 {
        let u = (t - self.props_after - order as f32 * self.prop_gap)
            / self.prop;
        spring(u, self.overshoot).max(1e-3)
    }
}

/// The way of a house's change: in or out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Way {
    In,
    Out,
}

impl Way {
    /// The parts of what comes and of what it takes the place of.
    pub fn parts(self) -> (Part, Part) {
        match self {
            Self::In => (Part::Rise, Part::Clear),
            Self::Out => (Part::Rest, Part::Fall),
        }
    }
}

/// The part one house plays while a house comes in or goes out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// A house stands up out of the ground, and its yard after it.
    Rise,
    /// The thing that stood on the plot sinks under the house that
    /// rises.
    Clear,
    /// A house shudders and falls flat.
    Fall,
    /// The remains of a fallen house stand up in its place.
    Rest,
}

impl Part {
    /// True when it is there when the change is over.
    pub fn comes(self) -> bool {
        matches!(self, Self::Rise | Self::Rest)
    }

    /// Seconds it stays out of sight after the change begins.
    pub fn waits(self, set: &HouseMotion) -> f32 {
        match self {
            Self::Rest => set.rest_after,
            _ => 0.0,
        }
    }

    /// Seconds from the change's start to this part's end, with
    /// `props` things popping up in the yard.
    pub fn length(self, set: &HouseMotion, props: usize) -> f32 {
        match self {
            Self::Rise => set.time_in(props),
            Self::Clear => set.clear,
            Self::Fall => set.shudder + set.fall,
            Self::Rest => set.rest_after + set.rise,
        }
    }

    /// Its place `t` seconds into the change, having stood at `from`
    /// when the change began.
    pub fn pose(
        self,
        set: &HouseMotion,
        t: f32,
        from: Frame,
    ) -> Frame {
        match self {
            Self::Rise | Self::Rest => {
                let u = (t - self.waits(set)) / set.rise;
                from.with_size(from.size * stood(set, u))
            }
            Self::Clear => {
                let u = (t / set.clear).clamp(0.0, 1.0);
                from.with_size(from.size * flat(1.0 - u * u, 0.0))
            }
            Self::Fall => {
                let grows = (t / set.shudder).clamp(0.0, 1.0);
                let down =
                    ((t - set.shudder) / set.fall).clamp(0.0, 1.0);
                let tip = set.tilt * grows * (1.0 - down);
                let beat = t * set.shake * TAU;
                let lean = Quat::from_rotation_z(tip * beat.sin())
                    * Quat::from_rotation_x(
                        tip * (beat * 1.3 + 1.0).cos(),
                    );
                Frame {
                    at: from.at,
                    turn: from.turn * lean,
                    size: from.size
                        * flat(1.0 - down * down, set.squash * down),
                }
            }
        }
    }
}

/// From 0 to 1 as `u` does, past 1 by about `over` on the way and
/// back.
fn spring(u: f32, over: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    // Three quarters of a swing, so that it comes down onto 1.
    let swing = 1.5;
    let damp = -over.clamp(1e-3, 0.99).ln() * swing;
    1.0 - (-damp * u).exp() * (swing * PI * u).cos()
}

/// A scale of `height`, widened by `wider`. Never quite flat, so
/// that the normals stay finite.
fn flat(height: f32, wider: f32) -> Vec3 {
    Vec3::new(1.0 + wider, height.max(0.01), 1.0 + wider)
}

/// The scale of something standing up, `u` of the way there: wide
/// while it is low, narrow while it is past its height.
fn stood(set: &HouseMotion, u: f32) -> Vec3 {
    let height = spring(u, set.overshoot);
    flat(height, set.squash * (1.0 - height))
}

/// A house's part in a change, as it plays.
#[derive(Clone, Copy, Debug)]
pub struct Shift {
    pub part: Part,
    /// True when the house took the place of something on its plot.
    pub replaces: bool,
    /// Seconds it waits before it begins.
    pub wait: f32,
    /// Clock time it began and where the house stood then. None
    /// until it has.
    pub began: Option<(f32, Frame)>,
}

impl Shift {
    pub fn new(part: Part, replaces: bool) -> Self {
        Self {
            part,
            replaces,
            wait: 0.0,
            began: None,
        }
    }

    /// The same, begun `wait` seconds late.
    pub fn after(mut self, wait: f32) -> Self {
        self.wait = wait;
        self
    }
}
