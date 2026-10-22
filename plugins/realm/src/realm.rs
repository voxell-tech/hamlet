//! The realm as one: the land and the society drawn from the roster,
//! and each part of the scene kept in step with them, a little each
//! frame within what the engine allows.

use std::time::Instant;

use glam::{Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_sdk::api::agents::{
    Focus, Recipient, Roster, Selected,
};
use hamlet_engine_sdk::scene::{
    Camera as Lens, Entity, MouseButton, SceneInput,
};

use crate::bridge::Bridge;
use crate::camera::Camera;
use crate::chief::Chief;
use crate::ground::Ground;
use crate::house::Remains;
use crate::keeper::Robots;
use crate::land::{Land, Unfit};
use crate::layout::plot;
use crate::moving::{Carry, Footprint};
use crate::palette::Palette;
use crate::pick::{Picking, Thing};
use crate::road::Roads;
use crate::signs::{Signs, Words};
use crate::sky::{Hour, Lights, Sky};
use crate::society::{AgentId, Society};
use crate::stock::Stock;
use crate::town::Towns;
use crate::tuning::WorldTuning;
use crate::upload::{Pace, Upload};

/// The land is grown from this seed, as the realm has always been.
const SEED: u64 = 7;

/// The first hamlet, the home itself, whose mayor the window's own
/// chat is with.
const HOME: &str = "home";

/// Keys of the lights' entities.
const SUN: u32 = 2;
const FILL: u32 = 3;

/// What every part of the scene reads as it is kept in step.
pub struct Ctx<'a> {
    pub tuning: &'a WorldTuning,
    pub pal: &'a Palette,
    pub land: &'a Land,
    pub society: &'a Society,
    pub stock: &'a Stock,
    pub sky: &'a Sky,
    /// Seconds since the realm began.
    pub clock: f32,
    /// Where the camera stands.
    pub eye: Vec3,
    /// The agent the chief talks to, and where its robot stands.
    pub beside: Option<(AgentId, Vec3)>,
}

/// The realm and its scene.
pub struct Realm {
    tuning: WorldTuning,
    pal: Palette,
    land: Land,
    society: Society,
    bridge: Bridge,
    stock: Stock,
    camera: Camera,
    lights: Lights,
    sky: Sky,
    /// The chief's clock, seconds east of UTC, and the hour shown.
    offset: i32,
    hour: Hour,
    /// Who the chief talks to.
    with: Option<Recipient>,
    ground: Ground,
    towns: Towns,
    roads: Roads,
    robots: Robots,
    chief: Chief,
    pub pick: Picking,
    /// The thing the camera stays on.
    following: Option<Thing>,
    /// The town in hand, where it would stand, and the command that
    /// set one down, until it is sent.
    carrying: Option<Carry>,
    footprint: Footprint,
    set_down: Option<(String, String)>,
    signs: Signs,
    /// The name the chief at this window signs in with.
    chief_name: String,
    pace: Pace,
    clock: f32,
}

impl Realm {
    /// The realm with no hamlets yet, its camera drawing into the
    /// panel's viewport.
    /// Only what is cheap is made here: every mesh is built and
    /// handed over as the realm ticks, within each frame's share.
    pub fn make() -> Result<Self, String> {
        let began = Instant::now();
        let (tuning, pal) = WorldTuning::read()?;
        let mut land =
            Land::new(SEED, tuning.generator.clone(), pal.clone());
        let mut society = Society::default();
        let sky =
            Sky::at(&tuning.looks, &tuning.sky, Sky::hour_at(0));
        let stock = Stock::make(&pal)?;
        let camera = Camera::make(&tuning.camera, &sky.look)?;
        let lights = Lights::make(
            Entity::make(None, SUN)?,
            Entity::make(None, FILL)?,
        );
        lights.show(&sky.look);
        stock.light_windows(&pal, sky.look.candle);
        // A chief takes its ground before any town is founded.
        let chief =
            Chief::make(&tuning, &pal, &mut land, &mut society)?;
        let ground = Ground::make(&pal)?;
        let towns = Towns::make(&pal)?;
        let roads = Roads::make(&pal)?;
        let robots = Robots::make(&pal)?;
        let footprint = Footprint::make(&land, &pal, &tuning.carry)?;
        let mut pace = Pace::default();
        pace.spend(began);
        Ok(Self {
            tuning,
            pal,
            land,
            society,
            bridge: Bridge::default(),
            stock,
            camera,
            lights,
            sky,
            offset: 0,
            hour: Hour::Clock,
            with: None,
            ground,
            towns,
            roads,
            robots,
            chief,
            pick: Picking::make()?,
            following: None,
            carrying: None,
            footprint,
            set_down: None,
            signs: Signs::make()?,
            chief_name: String::new(),
            pace,
            clock: 0.0,
        })
    }

    /// The camera the panel shows.
    pub fn lens(&self) -> &Lens {
        self.camera.lens()
    }

    /// What the person chose since it was last asked, as the window's
    /// focus selects it. None while nothing was chosen, or what was
    /// is nothing the focus holds.
    pub fn choice(&mut self) -> Option<Selected> {
        match self.pick.take_choice()? {
            None => Some(Selected::Nothing),
            Some(thing) => thing.selects(&self.society, &self.bridge),
        }
    }

    /// Brings the society in line with the roster.
    pub fn follow(&mut self, roster: &Roster) {
        let moments = self.bridge.follow(
            roster,
            &mut self.land,
            &mut self.society,
        );
        self.robots.moments(&moments);
        self.camera.frame(
            &self.tuning.camera,
            &self.land,
            &self.society,
        );
        self.signs.put_up(self.sign_words());
    }

    /// Follows the window's focus: the chief's clock and what is
    /// selected.
    pub fn focus(&mut self, focus: &Focus) {
        self.offset = focus.offset;
        self.with = Some(focus.with.clone());
        if self.chief_name != focus.chief {
            self.chief_name.clone_from(&focus.chief);
            self.signs.put_up(self.sign_words());
        }
        // A hamlet or the Hearth chosen elsewhere is brought into
        // view.
        if let Some(Thing::Common(_) | Thing::Hearth) =
            self.pick.follow(focus, &self.bridge)
        {
            self.focus_on_selected(false);
        }
    }

    /// Takes the town of `hamlet` in hand, to be set down with a
    /// click on the ground.
    pub fn carry(&mut self, hamlet: &str) {
        self.carrying = self.bridge.town(hamlet).map(|town| Carry {
            town,
            hamlet: hamlet.to_owned(),
            to: None,
            fits: Err(Unfit::Crowded),
        });
    }

    /// The command that sets a carried town down, once it is set
    /// down: the hamlet and the command.
    pub fn set_down(&mut self) -> Option<(String, String)> {
        self.set_down.take()
    }

    /// The agent of the society `with` is drawn as.
    fn talking_to(&self, with: &Recipient) -> Option<AgentId> {
        match with {
            Recipient::Agent(name) => self.bridge.find(name),
            Recipient::Mayor(hamlet) => {
                let town = self.bridge.town(hamlet)?;
                self.society.town(town).map(|town| town.leader)
            }
        }
    }

    /// Answers what the person did over the view.
    pub fn input(&mut self, input: &SceneInput) {
        self.camera.input(&self.tuning.camera, input);
        let still =
            self.camera.travel <= self.tuning.camera.click_slop;
        let Some(carry) = &mut self.carrying else {
            self.pick.input(input, still);
            return;
        };
        // A town in hand takes the clicks: the left button sets it
        // down where it fits, the right puts it back. The pointer is
        // still followed.
        let SceneInput::Pressed(press) = input else {
            if let SceneInput::Over(_) | SceneInput::Out(_) = input {
                self.pick.input(input, still);
            }
            return;
        };
        match (press.button, press.down) {
            (MouseButton::Primary, false) if still => {
                carry.reach(
                    &self.land,
                    &self.society,
                    self.camera.ray(&self.tuning.camera),
                );
                if let Some(command) = carry.command() {
                    self.set_down =
                        Some((carry.hamlet.clone(), command));
                    self.carrying = None;
                    self.pick.pass_click();
                }
            }
            (MouseButton::Secondary, false) if still => {
                self.carrying = None;
            }
            _ => {}
        }
    }

    /// A frame passed, `dt` seconds after the last: the sky, the
    /// camera, and each part of the scene a step further.
    pub fn tick(&mut self, dt: f32) {
        let began = Instant::now();
        self.clock += dt;
        let hour = self.hour.now(&self.tuning.sky, self.offset);
        let sky = Sky::at(&self.tuning.looks, &self.tuning.sky, hour);
        if sky.look != self.sky.look {
            self.lights.show(&sky.look);
            if sky.look.candle != self.sky.look.candle {
                self.stock.light_windows(&self.pal, sky.look.candle);
            }
            self.sky = sky;
        }
        // The thing followed, until the ground is slid by hand.
        if self.camera.following.is_none() {
            self.following = None;
        }
        if let Some(thing) = self.following {
            self.camera.following = self.place(thing);
        }
        self.camera.tick(
            &self.tuning.camera,
            &self.land,
            &self.sky.look,
            dt,
        );
        let mut upload = Upload::new(&self.pace);
        let beside = self
            .with
            .as_ref()
            .and_then(|with| self.talking_to(with))
            .and_then(|agent| Some((agent, self.robots.at(agent)?)));
        let ctx = Ctx {
            tuning: &self.tuning,
            pal: &self.pal,
            land: &self.land,
            society: &self.society,
            stock: &self.stock,
            sky: &self.sky,
            clock: self.clock,
            eye: self.camera.eye(&self.land),
            beside,
        };
        let (view, ahead) = self.camera.views();
        let detail = &self.tuning.detail;
        let sights = [Some(view), ahead]
            .into_iter()
            .flatten()
            .map(|view| {
                view.sight(&self.land, &self.tuning.camera, detail)
            })
            .collect::<Vec<_>>();
        self.ground.update(&ctx, &sights, &mut upload);
        self.towns.update(&ctx, &mut upload);
        self.roads.update(&ctx, &mut upload);
        self.robots.update(&ctx, dt, &mut upload);
        self.chief.update(&ctx, dt, &mut upload);
        let rings = [self.pick.hovered(), self.pick.selected()]
            .map(|thing| thing.and_then(|thing| self.ring(thing)));
        self.pick.lay(rings[0], rings[1], &mut upload);
        let ray = self.camera.ray(&self.tuning.camera);
        if let Some(carry) = &mut self.carrying {
            // A town that is gone is in nobody's hand.
            if self.society.town(carry.town).is_none() {
                self.carrying = None;
            } else {
                carry.reach(&self.land, &self.society, ray);
            }
        }
        self.footprint.show(
            self.carrying.as_ref(),
            &self.land,
            &self.society,
            &self.tuning.carry,
        );
        let tag = self.tag();
        self.signs.show(
            self.camera.eye(&self.land),
            self.tuning.pick.signs_far,
            tag,
        );
        self.pace.spend(began);
    }

    /// True when `agent` holds the office of the first hamlet, the
    /// one the window's own chat is with.
    fn first_mayor(&self, agent: AgentId) -> bool {
        self.bridge
            .town(HOME)
            .and_then(|town| self.society.town(town))
            .is_some_and(|town| town.leader == agent)
    }

    /// The signs over every house, common and the Hearth.
    fn sign_words(&self) -> Vec<(Thing, Words)> {
        let (pick, chief) = (&self.tuning.pick, &self.tuning.chief);
        let mut signs = Vec::new();
        for agent in &self.society.agents {
            // A house that is gone answers only as a ruin.
            let ruin = agent.remains(&self.land.set.house)
                == Some(Remains::Ruin);
            if agent.archived && !ruin {
                continue;
            }
            let says =
                if !agent.archived && self.first_mayor(agent.id) {
                    "Mayor".to_owned()
                } else {
                    match self.bridge.named(agent.id) {
                        Some((name, _)) => name.clone(),
                        None => continue,
                    }
                };
            let thing = Thing::House(agent.id);
            if let Some(at) = self.place(thing) {
                let at =
                    self.land.on_ground(at) + Vec3::Y * pick.house.1;
                signs.push((
                    thing,
                    Words {
                        at,
                        says: says.to_uppercase(),
                    },
                ));
            }
        }
        for town in &self.society.towns {
            let thing = Thing::Common(town.id);
            if let (Some(at), Some(title)) =
                (self.place(thing), self.bridge.title(town.id))
            {
                let at =
                    self.land.on_ground(at) + Vec3::Y * pick.common.1;
                signs.push((
                    thing,
                    Words {
                        at,
                        says: title.to_uppercase(),
                    },
                ));
            }
        }
        if let Some(at) = self.chief.hearth() {
            let says = format!("{}'s Hearth", self.chief_name)
                .to_uppercase();
            let at = at + Vec3::Y * chief.house.1;
            signs.push((Thing::Hearth, Words { at, says }));
        }
        signs
    }

    /// The words over the thing under the pointer, when it has no
    /// sign of its own.
    fn tag(&self) -> Option<Words> {
        let (pick, chief) = (&self.tuning.pick, &self.tuning.chief);
        let (at, says) = match self.pick.hovered()? {
            Thing::Robot(agent) => {
                let says = if self.first_mayor(agent) {
                    "The Mayor".to_owned()
                } else {
                    self.bridge.named(agent)?.0.clone()
                };
                (
                    self.robots.at(agent)?
                        + Vec3::Y * pick.robot_box.y,
                    says,
                )
            }
            Thing::Mascot => (
                self.chief.mascot()? + Vec3::Y * chief.mascot_box.y,
                format!("{}, Chief", self.chief_name),
            ),
            Thing::House(_) | Thing::Common(_) | Thing::Hearth => {
                return None;
            }
        };
        Some(Words { at, says })
    }

    /// The ground a ring about `thing` lies on, and its radius.
    fn ring(&self, thing: Thing) -> Option<(Vec3, f32)> {
        let at = self.place(thing)?;
        let (pick, chief) = (&self.tuning.pick, &self.tuning.chief);
        let radius = match thing {
            Thing::Robot(_) => {
                pick.robot_box.x.max(pick.robot_box.z) * 0.62
            }
            Thing::House(_) => pick.house.0,
            Thing::Common(_) => pick.common.0,
            Thing::Mascot => {
                chief.mascot_box.x.max(chief.mascot_box.z) * 0.62
            }
            Thing::Hearth => chief.house.0,
        };
        let ground =
            self.land.on_ground(at) + Vec3::Y * pick.ring_lift;
        Some((ground, radius))
    }

    /// The place of `thing` on the ground.
    fn place(&self, thing: Thing) -> Option<Vec2> {
        match thing {
            // An agent without a robot, as one that is gone, is at
            // its house.
            Thing::Robot(agent) => match self.robots.at(agent) {
                Some(at) => Some(at.xz()),
                None => self.place(Thing::House(agent)),
            },
            Thing::House(agent) => {
                let agent = self.society.agent(agent)?;
                let town = self.society.town(agent.town)?;
                Some(plot(&self.land, town.site, agent.plot).at)
            }
            Thing::Common(town) => self
                .society
                .town(town)
                .map(|town| self.land.sites[town.site].at),
            Thing::Mascot => self.chief.mascot().map(|at| at.xz()),
            Thing::Hearth => self.chief.hearth().map(|at| at.xz()),
        }
    }

    /// Brings the thing selected to the middle of the view, and stays
    /// on it if `stay`.
    pub fn focus_on_selected(&mut self, stay: bool) {
        let Some(thing) = self.pick.selected() else {
            return;
        };
        if let Some(at) = self.place(thing) {
            self.camera.focus_on(&self.tuning.camera, at, stay);
            self.following = stay.then_some(thing);
        }
    }

    /// Has the sky show `hour`.
    pub fn set_hour(&mut self, hour: Hour) {
        self.hour = hour;
    }

    /// Goes back to the framing of every hamlet.
    pub fn reset_camera(&mut self) {
        self.camera.reset();
        self.following = None;
    }
}
