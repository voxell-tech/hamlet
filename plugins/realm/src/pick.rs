//! The things of the realm a pointer can hit, named by the keys of
//! the entities that draw them, the one under it and the one chosen,
//! each with a ring of light on the ground about it.

use std::f32::consts::TAU;

use glam::{Quat, Vec3};
use hamlet_engine_sdk::api::agents::{Focus, Selected};
use hamlet_engine_sdk::scene::{
    Entity, Material, MouseButton, SceneInput,
};

use crate::bridge::Bridge;
use crate::frame::Frame;
use crate::mesh::Mb;
use crate::society::{AgentId, Society, TownId};
use crate::stock::glow;
use crate::upload::{Unsent, Upload};

/// The colour of a ring's light, and how bright the ring under the
/// pointer and the ring of the thing chosen burn.
const CANDLE: Vec3 = Vec3::new(1.0, 0.64, 0.25);
const HOVER_GLOW: f32 = 1.7;
const CHOSEN_GLOW: f32 = 6.5;

/// Keys of the rings' entities.
const HOVER_RING: u32 = 4;
const CHOSEN_RING: u32 = 5;

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
    /// Distance past its ring to which the ground under a thing that
    /// moves is looked up, so that it is not looked up again every
    /// frame.
    pub ring_slack: f32,
    /// Camera distance beyond which names are not shown.
    pub signs_far: f32,
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
            _ => return None,
        })
    }

    /// What the window's focus selects for it: an agent for its robot
    /// or house, a hamlet for its common, and the Hearth for the
    /// chief's place and mascot.
    pub fn selects(
        self,
        society: &Society,
        bridge: &Bridge,
    ) -> Option<Selected> {
        match self {
            Self::Robot(agent) | Self::House(agent) => bridge
                .named(agent)
                .map(|(name, _)| Selected::Agent(name.clone())),
            Self::Common(town) => {
                society.town(town)?;
                bridge
                    .hamlet(town)
                    .map(|hamlet| Selected::Hamlet(hamlet.to_owned()))
            }
            Self::Mascot | Self::Hearth => Some(Selected::Hearth),
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
            Selected::Nothing | Selected::Library => None,
        }
    }
}

/// A ring of light lying on the ground.
struct Ring {
    entity: Entity,
    /// Where it lies and its radius, while it is shown.
    shown: Option<(Vec3, f32)>,
    material: Material,
}

impl Ring {
    fn make(key: u32, glow_by: f32) -> Result<Self, String> {
        let entity = Entity::make(None, key)?;
        let material = Material::make(glow(CANDLE, glow_by))?;
        entity.set_visible(false);
        entity.set_shadows(false, false);
        Ok(Self {
            entity,
            shown: None,
            material,
        })
    }

    /// Lays it at `at` with `radius`, or takes it away.
    fn lay(&mut self, at: Option<(Vec3, f32)>) {
        if self.shown == at {
            return;
        }
        if let Some((at, radius)) = at {
            self.entity.set_transform(
                Frame::at(at).with_size(Vec3::splat(radius)).scene(),
            );
        }
        if self.shown.is_some() != at.is_some() {
            self.entity.set_visible(at.is_some());
        }
        self.shown = at;
    }
}

/// A flat band of radius 1, its inner edge at 0.86.
fn band() -> Mb {
    let mut mb = Mb::new();
    let segs = 48;
    let ring = (0..segs)
        .map(|i| {
            let turn =
                Quat::from_rotation_y(i as f32 / segs as f32 * TAU);
            (
                mb.v(turn * Vec3::X * 0.86, Vec3::Y, Vec3::ONE),
                mb.v(turn * Vec3::X, Vec3::Y, Vec3::ONE),
            )
        })
        .collect::<Vec<_>>();
    for i in 0..segs {
        let (a, b) = (ring[i], ring[(i + 1) % segs]);
        mb.quad(a.0, a.1, b.1, b.0);
    }
    mb
}

/// The thing under the pointer and the one chosen, and their rings.
pub struct Picking {
    hovered: Option<Thing>,
    selected: Option<Thing>,
    /// The left button let go over the view without moving, until a
    /// thing says it was clicked.
    loose: bool,
    /// What the person chose, to tell the window's focus: a thing,
    /// or nothing for the bare ground.
    chosen: Option<Option<Thing>>,
    rings: [Ring; 2],
    /// The band each ring draws, once it is made.
    band: Option<hamlet_engine_sdk::scene::Mesh>,
}

impl Picking {
    pub fn make() -> Result<Self, String> {
        Ok(Self {
            hovered: None,
            selected: None,
            loose: false,
            chosen: None,
            rings: [
                Ring::make(HOVER_RING, HOVER_GLOW)?,
                Ring::make(CHOSEN_RING, CHOSEN_GLOW)?,
            ],
            band: None,
        })
    }

    pub fn hovered(&self) -> Option<Thing> {
        self.hovered
    }

    pub fn selected(&self) -> Option<Thing> {
        self.selected
    }

    /// Takes what the person chose since it was last taken.
    pub fn take_choice(&mut self) -> Option<Option<Thing>> {
        if std::mem::take(&mut self.loose) {
            self.chosen = Some(None);
        }
        self.chosen.take()
    }

    /// Answers the pointer over the view. `still` is true while the
    /// left button has not moved far enough since it went down to be
    /// a drag.
    pub fn input(&mut self, input: &SceneInput, still: bool) {
        match input {
            SceneInput::Over(hit) => {
                self.hovered = Thing::of(hit.key)
            }
            SceneInput::Out((key, _)) => {
                if self.hovered == Thing::of(*key) {
                    self.hovered = None;
                }
            }
            SceneInput::Pressed(press)
                if press.button == MouseButton::Primary
                    && !press.down
                    && still =>
            {
                self.loose = true;
            }
            SceneInput::Click(hit) => {
                self.loose = false;
                if let Some(thing) = Thing::of(hit.key) {
                    self.selected = Some(thing);
                    self.chosen = Some(Some(thing));
                }
            }
            _ => {}
        }
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
        now
    }

    /// Lays the rings about the thing under the pointer and the one
    /// chosen, each given as its ground and its radius.
    pub fn lay(
        &mut self,
        hovered: Option<(Vec3, f32)>,
        selected: Option<(Vec3, f32)>,
        upload: &mut Upload,
    ) {
        if self.band.is_none() {
            match band().upload(upload) {
                Ok(mesh) => {
                    for ring in &self.rings {
                        ring.entity.draw(&mesh, &ring.material);
                    }
                    self.band = Some(mesh);
                }
                Err(Unsent::Full(_)) => return,
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_sdk::warn(&format!("rings: {why}"));
                    return;
                }
            }
        }
        let hovered =
            hovered.filter(|_| self.hovered != self.selected);
        let [hover, chosen] = &mut self.rings;
        hover.lay(hovered);
        chosen.lay(selected);
    }
}
