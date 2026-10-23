//! The things of the realm a pointer can hit, named by the keys of
//! the entities that draw them, the one under it and the one chosen,
//! each with a ring of light on the ground about it.

use glam::{Vec3, Vec3Swizzles};
use hamlet_engine_bevy::api::agents::{Focus, Selected};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Modifiers, MouseButton, Shader};

use crate::bridge::Bridge;
use crate::build::mark::{Mark, Shape};
use crate::build::upload::Draw;
use crate::folk::keeper::Floor;
use crate::society::{AgentId, Society, TownId};

/// The colour of a ring's light, and how bright the ring under the
/// pointer and the ring of the thing chosen burn.
const CANDLE: Vec3 = Vec3::new(1.0, 0.64, 0.25);
const HOVER_GLOW: f32 = 1.7;
const CHOSEN_GLOW: f32 = 6.5;

const ALSO_GLOW: f32 = 4.0;

/// Keys of the rings' entities.
const HOVER_RING: u32 = 4;
const CHOSEN_RING: u32 = 5;
const ALSO_RING: u32 = 8;

/// The room each thing takes for the pointer, its ring and its name.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickSettings {
    /// The box a pointer can hit for a robot.
    pub robot_box: Vec3,
    /// Radius and height of the room a house takes.
    pub house: (f32, f32),
    /// Radius and height of the room a common takes.
    pub common: (f32, f32),
    /// Height of a ring above the ground.
    pub ring_lift: f32,
    /// Width of a ring's line, whatever the size of what it is
    /// about.
    pub ring_width: f32,
    /// Distance past its ring to which the ground under a thing that
    /// moves is looked up, so that it is not looked up again every
    /// frame.
    pub ring_slack: f32,
    /// Camera distance beyond which names are not shown.
    pub signs_far: f32,
}

/// The realm's methods on [`Modifiers`].
pub trait ModifiersExt {
    /// True when Shift is held, which makes a click a second choice.
    fn shift(&self) -> bool;
}

impl ModifiersExt for Modifiers {
    fn shift(&self) -> bool {
        *self & Modifiers::SHIFT != Modifiers::empty()
    }
}

/// A thing in the realm that can be pointed at.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Thing {
    /// An agent's robot.
    Robot(AgentId),
    /// An agent's house, or what is left of it.
    House(AgentId),
    /// A hamlet's common.
    Common(TownId),
    /// The chief's mascot.
    Mascot,
    /// The chief's place, the Hearth.
    Hearth,
    /// The library every agent shares.
    Library,
    /// The workbench on a hamlet's common, which stands for its
    /// folder.
    Workbench(TownId),
    /// The clockwork on a hamlet's workbench, which sets its mode.
    Clockwork(TownId),
}

/// The bits of a key that say which kind of thing it is.
const KIND: u32 = 28;

impl Thing {
    /// The key of the entities that draw it. Nothing has the key 0.
    pub fn key(self) -> u32 {
        let (kind, id) = match self {
            Self::Robot(agent) => (1, agent.0),
            Self::House(agent) => (2, agent.0),
            Self::Common(town) => (3, town.0),
            Self::Mascot => (4, 0),
            Self::Hearth => (5, 0),
            Self::Library => (6, 0),
            Self::Workbench(town) => (7, town.0),
            Self::Clockwork(town) => (8, town.0),
        };
        (kind << KIND) | (id & ((1 << KIND) - 1))
    }

    /// The thing drawn by the entities of `key`.
    pub fn of(key: u32) -> Option<Self> {
        let id = key & ((1 << KIND) - 1);
        Some(match key >> KIND {
            1 => Self::Robot(AgentId(id)),
            2 => Self::House(AgentId(id)),
            3 => Self::Common(TownId(id)),
            4 => Self::Mascot,
            5 => Self::Hearth,
            6 => Self::Library,
            7 => Self::Workbench(TownId(id)),
            8 => Self::Clockwork(TownId(id)),
            _ => return None,
        })
    }

    /// The thing a click on it chooses: a hamlet's common for the
    /// workbench and the clockwork on it.
    pub fn chosen(self) -> Self {
        match self {
            Self::Workbench(town) | Self::Clockwork(town) => {
                Self::Common(town)
            }
            other => other,
        }
    }

    /// What the window's focus selects for it: an agent for its robot
    /// or house, a hamlet for its common, the Hearth for the chief's
    /// place and mascot, and the library for itself.
    pub fn selects(
        self,
        society: &Society,
        bridge: &Bridge,
    ) -> Option<Selected> {
        match self {
            Self::Robot(agent) | Self::House(agent) => bridge
                .named(agent)
                .map(|(name, _)| Selected::Agent(name.clone())),
            Self::Common(town)
            | Self::Workbench(town)
            | Self::Clockwork(town) => {
                society.town(town)?;
                bridge
                    .hamlet(town)
                    .map(|hamlet| Selected::Hamlet(hamlet.to_owned()))
            }
            Self::Mascot | Self::Hearth => Some(Selected::Hearth),
            Self::Library => Some(Selected::Library),
        }
    }

    /// The thing the focus's selection is drawn as.
    fn selected(
        selected: &Selected,
        bridge: &Bridge,
    ) -> Option<Self> {
        match selected {
            Selected::Agent(name) => {
                bridge.find(name).map(Self::Robot)
            }
            Selected::Hamlet(hamlet) => {
                bridge.town(hamlet).map(Self::Common)
            }
            Selected::Hearth => Some(Self::Hearth),
            Selected::Library => Some(Self::Library),
            Selected::Nothing => None,
        }
    }
}

/// A ring of light laid on the ground.
struct Ring {
    mark: Mark,
    /// Where it lies and its radius, while it is shown.
    shown: Option<(Vec3, f32)>,
}

impl Ring {
    fn make(
        key: u32,
        glow_by: f32,
        shader: &Handle<Shader>,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let mut mark = Mark::make(key, shader, Shape::RING, draw)?;
        mark.shine(CANDLE * glow_by, 1.0, draw);
        Ok(Self { mark, shown: None })
    }

    /// Lays it at `at` with `radius` on `floor`, or takes it away.
    fn lay(
        &mut self,
        at: Option<(Vec3, f32)>,
        floor: &Floor,
        set: &PickSettings,
        draw: &mut Draw,
    ) {
        if let Some((at, radius)) = at {
            let reach = radius * 1.1;
            if !self.mark.holds(at.xz(), reach) {
                // A ring that moves with what it is about is given
                // more ground than it covers, so that its ground is
                // not looked up again every frame.
                let moving = self.shown.is_some_and(|(was, of)| {
                    of == radius && was != at
                });
                let slack = if moving { set.ring_slack } else { 0.0 };
                self.mark.settle(at, reach + slack, floor, draw);
            }
            if self.shown != Some((at, radius)) {
                self.mark.lay(at, radius, 0.0, draw);
                self.mark
                    .shape(Shape::band(radius, set.ring_width), draw);
            }
        }
        self.mark.hand_over(draw);
        self.mark.show(at.is_some(), draw);
        self.shown = at;
    }
}

/// The thing under the pointer and the one chosen, and their rings.
#[derive(Resource)]
pub struct Picking {
    hovered: Option<Thing>,
    /// The robot whose box the pointer is over.
    robot: Option<Thing>,
    selected: Option<Thing>,
    /// A second thing, chosen with Shift held, for what takes two.
    also: Option<Thing>,
    /// The left button let go over the view without moving, until a
    /// thing says it was clicked.
    loose: bool,
    /// The click of a release that was taken for something else, to
    /// be passed over until the left button next goes down.
    passed: bool,
    /// The right button let go over the view without moving, until a
    /// thing says it was clicked.
    asking: bool,
    /// What the person chose, to tell the window's focus: a thing,
    /// or nothing for the bare ground.
    chosen: Option<Option<Thing>>,
    /// What the person asked about with the right button: a thing,
    /// or nothing for the bare ground.
    asked: Option<Option<Thing>>,
    rings: [Ring; 3],
}

impl Picking {
    /// Nothing pointed at or chosen, its rings drawn by `shader`.
    pub fn make(
        shader: &Handle<Shader>,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        Ok(Self {
            hovered: None,
            robot: None,
            selected: None,
            also: None,
            loose: false,
            passed: false,
            asking: false,
            chosen: None,
            asked: None,
            rings: [
                Ring::make(HOVER_RING, HOVER_GLOW, shader, draw)?,
                Ring::make(CHOSEN_RING, CHOSEN_GLOW, shader, draw)?,
                Ring::make(ALSO_RING, ALSO_GLOW, shader, draw)?,
            ],
        })
    }

    /// The thing under the pointer: a robot whose box the pointer is
    /// over, whatever stands in front of it, or else the thing the
    /// engine finds.
    pub fn hovered(&self) -> Option<Thing> {
        self.robot.or(self.hovered)
    }

    pub fn selected(&self) -> Option<Thing> {
        self.selected
    }

    pub fn also(&self) -> Option<Thing> {
        self.also
    }

    /// The agent of the robot whose box the pointer is over.
    pub fn hovered_robot(&self) -> Option<AgentId> {
        match self.robot {
            Some(Thing::Robot(agent)) => Some(agent),
            _ => None,
        }
    }

    /// Takes the robot of `agent` as the one under the pointer.
    pub fn aim(&mut self, agent: Option<AgentId>) {
        self.robot = agent.map(Thing::Robot);
    }

    /// Takes what the person chose since it was last taken.
    pub fn take_choice(&mut self) -> Option<Option<Thing>> {
        if std::mem::take(&mut self.loose) {
            // A click the engine found nothing under may still be on
            // a robot's box.
            if self.robot.is_some() {
                self.selected = self.robot;
            }
            self.also = None;
            self.chosen = Some(self.robot);
        }
        self.chosen.take()
    }

    /// Takes what the person asked about since it was last taken.
    pub fn take_ask(&mut self) -> Option<Option<Thing>> {
        if std::mem::take(&mut self.asking) {
            self.asked = Some(self.robot);
        }
        self.asked.take()
    }

    /// Answers the pointer over the view. `still` is true while the
    /// button has not moved far enough since it went down to be a
    /// drag. `thing` is the thing an entity draws.
    pub fn input(
        &mut self,
        input: &SceneInput,
        still: bool,
        thing: impl Fn(Entity) -> Option<Thing>,
    ) {
        match input {
            SceneInput::Over(hit) => self.hovered = thing(hit.entity),
            SceneInput::Out(out) => {
                if self.hovered == thing(out.entity) {
                    self.hovered = None;
                }
            }
            SceneInput::Press(press) => match press.button {
                MouseButton::Primary if press.down => {
                    self.passed = false;
                }
                MouseButton::Primary => self.loose = still,
                MouseButton::Secondary if press.down => {
                    self.asking = false;
                }
                MouseButton::Secondary => self.asking = still,
                MouseButton::Middle => {}
            },
            SceneInput::Click(hit)
                if self.passed
                    && hit.button == MouseButton::Primary =>
            {
                self.passed = false
            }
            SceneInput::Click(hit) => {
                // A robot behind what took the click is the one
                // meant.
                let thing = self.robot.or_else(|| thing(hit.entity));
                match hit.button {
                    MouseButton::Primary => {
                        self.loose = false;
                        if still && let Some(thing) = thing {
                            self.choose(thing, hit.modifiers);
                        }
                    }
                    MouseButton::Secondary => {
                        self.asking = false;
                        if still {
                            self.asked =
                                Some(thing.map(Thing::chosen));
                        }
                    }
                    MouseButton::Middle => {}
                }
            }
            _ => {}
        }
    }

    /// Chooses `thing` for a click with `modifiers` held: the
    /// selection, or with Shift the second choice, which the same
    /// click lets go again. The clockwork is the town's to turn
    /// unless Shift is held.
    fn choose(&mut self, thing: Thing, modifiers: Modifiers) {
        let shift = modifiers.shift();
        if matches!(thing, Thing::Clockwork(_)) && !shift {
            return;
        }
        let thing = thing.chosen();
        if shift {
            let free = self.also != Some(thing)
                && self.selected != Some(thing);
            self.also = free.then_some(thing);
        } else {
            self.selected = Some(thing);
            self.also = None;
            self.chosen = Some(Some(thing));
        }
    }

    /// Passes over the click of the left button's last release.
    pub fn pass_click(&mut self) {
        self.passed = true;
    }

    /// Follows the selection of the window's focus, made here or
    /// anywhere else: the thing it selects anew, if it does.
    pub fn follow(
        &mut self,
        focus: &Focus,
        bridge: &Bridge,
    ) -> Option<Thing> {
        let now = Thing::selected(&focus.selected, bridge);
        // A house or a robot stands for the same agent, and the
        // mascot for the chief's place.
        let same = match (self.selected, now) {
            (Some(Thing::House(a)), Some(Thing::Robot(b))) => a == b,
            (Some(Thing::Mascot), Some(Thing::Hearth)) => true,
            (was, now) => was == now,
        };
        if same {
            return None;
        }
        self.selected = now;
        self.also = None;
        now
    }

    /// Lets go of the selection and the second choice.
    pub fn clear(&mut self) {
        self.selected = None;
        self.also = None;
    }

    /// Lays the rings on `floor` about the thing under the pointer,
    /// the one chosen and the second choice, each given as its
    /// ground and its radius.
    pub fn lay(
        &mut self,
        [hovered, selected, also]: [Option<(Vec3, f32)>; 3],
        floor: &Floor,
        set: &PickSettings,
        draw: &mut Draw,
    ) {
        let hovered = hovered.filter(|_| {
            self.hovered() != self.selected
                && self.hovered() != self.also
        });
        let also = also.filter(|_| self.also != self.selected);
        let [hover, chosen, second] = &mut self.rings;
        hover.lay(hovered, floor, set, draw);
        chosen.lay(selected, floor, set, draw);
        second.lay(also, floor, set, draw);
    }
}
