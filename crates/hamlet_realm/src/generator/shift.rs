//! A house coming into the world and going out of it
//! (docs/robots.md). Only transforms and visibility move:
//! the meshes are the ones the background tasks built.

#[cfg(test)]
mod tests;

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use bevy::window::RequestRedraw;

use super::House;
use super::sync::Drawn;
use super::work::{Passage, Robot};
use crate::Clock;
use crate::keeper::Act;

/// The coming in and going out of a house.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
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
    pub(crate) fn time_in(&self, props: usize) -> f32 {
        let last = props.saturating_sub(1) as f32 * self.prop_gap;
        self.rise.max(self.props_after + last + self.prop)
    }
}

/// The way of a house's change: in or out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Way {
    In,
    Out,
}

impl Way {
    /// The parts of what comes and of what it takes the place of.
    pub(super) fn parts(self) -> (Part, Part) {
        match self {
            Self::In => (Part::Rise, Part::Clear),
            Self::Out => (Part::Rest, Part::Fall),
        }
    }
}

/// The part one entity plays while a house comes in or goes out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Part {
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
    fn comes(self) -> bool {
        matches!(self, Self::Rise | Self::Rest)
    }

    /// Seconds it stays out of sight after the change begins.
    fn waits(self, set: &HouseMotion) -> f32 {
        match self {
            Self::Rest => set.rest_after,
            _ => 0.0,
        }
    }

    /// Seconds from the change's start to this part's end, with
    /// `props` things popping up in the yard.
    fn length(self, set: &HouseMotion, props: usize) -> f32 {
        match self {
            Self::Rise => set.time_in(props),
            Self::Clear => set.clear,
            Self::Fall => set.shudder + set.fall,
            Self::Rest => set.rest_after + set.rise,
        }
    }

    /// Its place `t` seconds into the change, having stood at `from`
    /// when the change began.
    fn pose(
        self,
        set: &HouseMotion,
        t: f32,
        from: Transform,
    ) -> Transform {
        match self {
            Self::Rise | Self::Rest => {
                let u = (t - self.waits(set)) / set.rise;
                from.with_scale(from.scale * stood(set, u))
            }
            Self::Clear => {
                let u = (t / set.clear).clamp(0.0, 1.0);
                from.with_scale(from.scale * flat(1.0 - u * u, 0.0))
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
                Transform {
                    translation: from.translation,
                    rotation: from.rotation * lean,
                    scale: from.scale
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

/// An entity's part in a house coming in or going out. One that
/// goes is despawned at the end; one that comes is left as it
/// stood.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Shift {
    part: Part,
    /// True when the house took the place of something on its plot.
    replaces: bool,
    /// Seconds it waits before it begins.
    wait: f32,
    /// Clock time it began and where the entity stood then. None
    /// until it has.
    began: Option<(f32, Transform)>,
}

impl Shift {
    pub(super) fn new(part: Part, replaces: bool) -> Self {
        Self {
            part,
            replaces,
            wait: 0.0,
            began: None,
        }
    }

    /// The same, begun `wait` seconds late.
    pub(super) fn after(mut self, wait: f32) -> Self {
        self.wait = wait;
        self
    }

    pub(super) fn part(&self) -> Part {
        self.part
    }

    /// The clock time it began and where it stood then, once it
    /// has.
    pub(super) fn began(&self) -> Option<(f32, Transform)> {
        self.began
    }
}

/// A thing that stands about a house, as a child of it.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Prop {
    /// Its place at rest, in the house's frame.
    rest: Transform,
    /// True when it outlasts the house: it stands while the house
    /// comes or goes around it.
    stays: bool,
}

impl Prop {
    /// A thing of the work yard. It pops up after its house.
    pub(super) fn yard(rest: Transform) -> Self {
        Self { rest, stays: false }
    }

    /// The garden tree. It stands by the ruin as by the house.
    pub(super) fn tree(rest: Transform) -> Self {
        Self { rest, stays: true }
    }
}

/// On a robot that waits, unseen, for its house to stand.
#[derive(Component)]
pub(super) struct Folded;

fn show(seen: &mut Mut<Visibility>, want: Visibility) {
    if **seen != want {
        **seen = want;
    }
}

/// Plays every house that is coming in or going out.
pub(super) fn shift_houses(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<HouseMotion>,
    drawn: Res<Drawn>,
    mut houses: Query<
        (
            Entity,
            &House,
            &mut Shift,
            &mut Transform,
            &mut Visibility,
            Option<&Children>,
        ),
        (Without<Prop>, Without<Robot>),
    >,
    mut props: Query<
        (&Prop, &mut Transform, &mut Visibility),
        Without<Robot>,
    >,
    leaving: Query<(&Robot, &Passage)>,
    mut robots: Query<&mut Visibility, (With<Robot>, With<Folded>)>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if houses.is_empty() {
        return;
    }
    redraw.write(RequestRedraw);
    let now = clock.t;
    for (entity, house, mut shift, mut at, mut seen, children) in
        &mut houses
    {
        let part = shift.part;
        let (since, from) = match shift.began {
            Some(began) => began,
            None => {
                // A house stands until its robot has taken its
                // leave.
                let held = matches!(part, Part::Fall | Part::Rest)
                    && !clock.frozen
                    && leaving.iter().any(|(robot, passage)| {
                        robot.agent == house.agent && passage.leaves()
                    });
                if held {
                    continue;
                }
                let begins =
                    if clock.frozen { now } else { now + shift.wait };
                shift.began = Some((begins, *at));
                (begins, *at)
            }
        };
        let t = now - since;
        if t < 0.0 {
            continue;
        }
        let pops = |prop: &Prop| !(prop.stays && shift.replaces);
        let popping = children
            .into_iter()
            .flatten()
            .filter(|child| {
                props.get(**child).is_ok_and(|(prop, ..)| pops(prop))
            })
            .count();
        // A still shows the moment after.
        let over = clock.frozen || t >= part.length(&set, popping);

        if over && !part.comes() {
            commands.entity(entity).try_despawn();
            continue;
        }
        if over {
            *at = from;
            show(&mut seen, Visibility::Inherited);
            // Its next coming or going is charmed afresh.
            commands
                .entity(entity)
                .try_remove::<(Shift, super::magic::Charmed)>();
        } else {
            *at = part.pose(&set, t, from);
            if t >= part.waits(&set) {
                show(&mut seen, Visibility::Inherited);
            }
        }

        let mut order = 0;
        for child in children.into_iter().flatten() {
            let Ok((prop, mut local, mut seen)) =
                props.get_mut(*child)
            else {
                continue;
            };
            let rest = prop.rest;
            match (part.comes(), pops(prop)) {
                // The one of the house that comes is in sight.
                (false, false) => show(&mut seen, Visibility::Hidden),
                (false, true) => {}
                // Held as it stands, whatever the house under it
                // does.
                (true, false) => {
                    let scale = at.scale / from.scale;
                    local.translation = rest.translation / scale;
                    local.scale = rest.scale / scale;
                    show(
                        &mut seen,
                        if over {
                            Visibility::Inherited
                        } else {
                            Visibility::Visible
                        },
                    );
                }
                (true, true) => {
                    if part != Part::Rise {
                        continue;
                    }
                    let u = (t
                        - set.props_after
                        - order as f32 * set.prop_gap)
                        / set.prop;
                    order += 1;
                    local.scale = if over {
                        rest.scale
                    } else {
                        rest.scale
                            * spring(u, set.overshoot).max(1e-3)
                    };
                }
            }
        }

        // The robot unfolds on the porch of a house that stands.
        if part == Part::Rise
            && (over || t >= set.props_after)
            && let Some(&robot) = drawn.robots.get(&house.agent)
            && let Ok(mut seen) = robots.get_mut(robot)
        {
            *seen = Visibility::Inherited;
            commands
                .entity(robot)
                .try_remove::<Folded>()
                .try_insert(Passage::new(Act::Appear));
        }
    }
}
