//! The realm as one: the land and the society drawn from the roster,
//! and each part of the scene kept in step with them, a little each
//! frame within what the engine allows.

use bevy_ecs::system::SystemParam;
use glam::{Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Focus, Recipient,
    Roster, Selected,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Animate, MouseButton};
use hamlet_engine_bevy::ui::{NodePointer, PointerMode, open};
use hamlet_engine_bevy::{Reply, decode};

use crate::bridge::Bridge;
use crate::build::mark;
use crate::build::stock::Stock;
use crate::build::upload::{Draw, Upload};
use crate::folk::chief::{Chief, move_chief};
use crate::folk::keeper::{Floor, Lot, Robots, move_robots};
use crate::places::house::Remains;
use crate::places::library::{self, Library, build_library};
use crate::places::moving::{Carry, Carrying, Footprint};
use crate::places::road::{Roads, build_roads};
use crate::places::town::{Towns, build_towns};
use crate::society::{AgentId, Society};
use crate::tuning::WorldTuning;
use crate::view::camera::{Camera, Following};
use crate::view::pick::{Picking, Thing};
use crate::view::signs::{Signs, Words};
use crate::world::ground::{Ground, Sight, build_ground};
use crate::world::land::{Land, Unfit};
use crate::world::layout::plot;
use crate::world::palette::Palette;
use crate::world::sky::{Lights, Sky, SkyClock};

/// The land is grown from this seed, as the realm has always been.
const SEED: u64 = 7;

/// The first hamlet, the home itself, whose mayor the window's own
/// chat is with.
const HOME: &str = "home";

/// The Inspector, brought forward when something is chosen here.
const INSPECTOR: &str = "hamlet.inspector/inspector";

/// The longest frame the realm moves by, in seconds, so a frame that
/// came late does not throw things far.
const LONGEST_FRAME: f32 = 0.1;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Clock>()
        .init_resource::<Now>()
        .init_resource::<Pointer>()
        .init_resource::<Upload>()
        .init_resource::<Carrying>()
        .init_resource::<Following>()
        .init_resource::<SkyClock>()
        .init_resource::<ChiefFocus>()
        .add_systems(Startup, make)
        .configure_sets(
            Update,
            (Hear, Tick.run_if(ticked))
                .chain()
                .run_if(resource_exists::<Land>),
        )
        .add_systems(
            Update,
            (
                follow_roster,
                follow_focus,
                put_up_signs,
                take_carry,
                steer,
            )
                .chain()
                .in_set(Hear),
        )
        .add_systems(
            Update,
            (
                advance,
                crate::world::sky::light,
                glide,
                look,
                aim,
                build_ground,
                build_towns,
                build_roads,
                move_robots,
                move_chief,
                build_library,
                lay_rings,
                show_footprint,
                show_signs,
                settle,
            )
                .chain()
                .in_set(Tick),
        )
        .add_systems(
            PostUpdate,
            point.run_if(resource_exists::<Land>),
        );
}

/// What the realm hears: the agents service, the Inspector and the
/// person over its view.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
struct Hear;

/// A frame of the realm: the sky, the camera, and each part of the
/// scene a step further.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
struct Tick;

/// What every part of the scene reads as it is kept in step.
pub struct Ctx<'a> {
    pub tuning: &'a WorldTuning,
    pub pal: &'a Palette,
    pub land: &'a Land,
    /// The ground as it is drawn, yards and all.
    pub floor: &'a Floor<'a>,
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

/// The resources a [`Ctx`] is made of.
#[derive(SystemParam)]
pub struct Shared<'w> {
    pub tuning: Res<'w, WorldTuning>,
    pub pal: Res<'w, Palette>,
    pub land: Res<'w, Land>,
    pub society: Res<'w, Society>,
    pub stock: Res<'w, Stock>,
    pub sky: Res<'w, Sky>,
    pub clock: Res<'w, Clock>,
    pub now: Res<'w, Now>,
}

impl Shared<'_> {
    /// The ground as it is drawn this frame.
    pub fn floor(&self) -> Floor<'_> {
        Floor::new(&self.land, &self.now.lots)
    }

    pub fn ctx<'a>(&'a self, floor: &'a Floor<'a>) -> Ctx<'a> {
        Ctx {
            tuning: &self.tuning,
            pal: &self.pal,
            land: &self.land,
            floor,
            society: &self.society,
            stock: &self.stock,
            sky: &self.sky,
            clock: self.clock.now,
            eye: self.now.eye,
            beside: self.now.beside,
        }
    }
}

/// The realm's own time.
#[derive(Resource, Default)]
pub struct Clock {
    /// Seconds since the realm began.
    pub now: f32,
    /// Seconds the frame under way moves things by.
    pub dt: f32,
}

/// What the frame under way found as it began.
#[derive(Resource, Default)]
pub struct Now {
    pub eye: Vec3,
    pub beside: Option<(AgentId, Vec3)>,
    /// The ground of each robot's yard.
    pub lots: Vec<Lot>,
    /// The sight of the ground of the camera's view, and of the one
    /// it is going to.
    pub sights: Vec<Sight>,
}

/// Who the chief talks to, and the name the chief at this window
/// signs in with.
#[derive(Resource, Default)]
struct ChiefFocus {
    with: Option<Recipient>,
    chief: String,
}

/// The pointer over the realm's view.
#[derive(Resource, Default)]
pub struct Pointer(pub Option<NodePointer>);

/// The things of the realm and where they are.
#[derive(SystemParam)]
pub struct Things<'w> {
    tuning: Res<'w, WorldTuning>,
    land: Res<'w, Land>,
    society: Res<'w, Society>,
    bridge: Res<'w, Bridge>,
    robots: Res<'w, Robots>,
    chief: Res<'w, Chief>,
    library: Res<'w, Library>,
}

/// The realm with no hamlets yet, its camera drawing into the
/// panel's viewport. Only what is cheap is made here: every mesh is
/// built and handed over as the realm ticks.
fn make(mut draw: Draw) {
    if let Err(why) = made(&mut draw) {
        hamlet_engine_bevy::error(&format!(
            "the realm cannot be drawn: {why}"
        ));
    }
}

fn made(draw: &mut Draw) -> Result<(), String> {
    let (tuning, pal) = WorldTuning::read()?;
    let mut land =
        Land::new(SEED, tuning.generator.clone(), pal.clone());
    let mut society = Society::default();
    let sky = Sky::at(&tuning.looks, &tuning.sky, Sky::hour_at(0));
    let stock = Stock::make(&pal, draw)?;
    let mut camera = Camera::make(&tuning.camera, &sky.look, draw);
    let lights = Lights::make(draw);
    lights.show(&sky.look, draw);
    stock.light_windows(&pal, sky.look.candle, draw);
    // The engine keeps the shader while a mark draws with it.
    let marks = mark::shader(draw)?;
    // A chief takes its ground before any town is founded.
    let chief = Chief::make(
        &tuning,
        &pal,
        &marks,
        &mut land,
        &mut society,
        draw,
    )?;
    // The library takes its ground after every chief.
    let library =
        Library::raise(&pal, &mut land, &mut society, draw)?;
    // The Hearth is in view before any town is founded.
    camera.frame(&tuning.camera, &land, &society);
    let ground = Ground::make(&pal, draw)?;
    let towns = Towns::make(&pal, draw)?;
    let roads = Roads::make(&pal, draw)?;
    let robots = Robots::make(&pal, draw)?;
    let footprint =
        Footprint::make(&land, &pal, &tuning.carry, draw)?;
    let picking = Picking::make(&marks, draw)?;
    let signs = Signs::make(draw);
    let commands = &mut draw.commands;
    commands.insert_resource(Bridge::default());
    commands.insert_resource(stock);
    commands.insert_resource(camera);
    commands.insert_resource(lights);
    commands.insert_resource(sky);
    commands.insert_resource(chief);
    commands.insert_resource(library);
    commands.insert_resource(ground);
    commands.insert_resource(towns);
    commands.insert_resource(roads);
    commands.insert_resource(robots);
    commands.insert_resource(footprint);
    commands.insert_resource(picking);
    commands.insert_resource(signs);
    commands.insert_resource(society);
    commands.insert_resource(land);
    commands.insert_resource(pal);
    commands.insert_resource(tuning);
    commands.insert_resource(Animate(true));
    Ok(())
}

/// True on a call that moves time on: the realm's frame.
fn ticked(time: Res<Time>) -> bool {
    !time.delta().is_zero()
}

/// Brings the society in line with the roster.
fn follow_roster(
    mut replies: MessageReader<Replied>,
    mut events: MessageReader<AgentsEventReceived>,
    tuning: Res<WorldTuning>,
    mut world: (
        ResMut<Bridge>,
        ResMut<Land>,
        ResMut<Society>,
        ResMut<Robots>,
    ),
    mut camera: ResMut<Camera>,
    mut signs: ResMut<Signs>,
) {
    let replied = replies.read().filter_map(|replied| match &replied
        .reply
    {
        Ok(Reply::Agents(AgentsReply::Roster(roster))) => {
            Some(roster)
        }
        Ok(_) => None,
        Err(why) => {
            hamlet_engine_bevy::warn(&format!("the agents: {why}"));
            None
        }
    });
    let told = events.read().filter_map(|event| match &event.event {
        AgentsEvent::Roster(roster) => Some(roster),
        _ => None,
    });
    let rosters = replied.chain(told).collect::<Vec<&Roster>>();
    if rosters.is_empty() {
        return;
    }
    let (bridge, land, society, robots) = &mut world;
    for roster in rosters {
        let moments = bridge.follow(roster, land, society);
        robots.moments(&moments);
        camera.frame(&tuning.camera, land, society);
    }
    signs.stale = true;
}

/// Follows the window's focus: the chief's clock and what is
/// selected.
fn follow_focus(
    mut replies: MessageReader<Replied>,
    mut events: MessageReader<AgentsEventReceived>,
    things: Things,
    mut chief_focus: ResMut<ChiefFocus>,
    mut sky: ResMut<SkyClock>,
    mut signs: ResMut<Signs>,
    mut picking: ResMut<Picking>,
    mut camera: ResMut<Camera>,
    mut following: ResMut<Following>,
) {
    let replied = replies.read().filter_map(|replied| match &replied
        .reply
    {
        Ok(Reply::Agents(AgentsReply::Focus(focus))) => Some(focus),
        _ => None,
    });
    let told = events.read().filter_map(|event| match &event.event {
        AgentsEvent::Focus(focus) => Some(focus),
        _ => None,
    });
    for focus in replied.chain(told).collect::<Vec<&Focus>>() {
        sky.offset = focus.offset;
        chief_focus.with = Some(focus.with.clone());
        if chief_focus.chief != focus.chief {
            chief_focus.chief.clone_from(&focus.chief);
            signs.stale = true;
        }
        // A hamlet, the Hearth or the library chosen elsewhere is
        // brought into view.
        if let Some(
            Thing::Common(_) | Thing::Hearth | Thing::Library,
        ) = picking.follow(focus, &things.bridge)
        {
            things.focus_on(
                &picking,
                &mut camera,
                &mut following,
                false,
            );
        }
    }
}

/// Puts up the signs over every house, common and the Hearth anew,
/// once what they show changed.
fn put_up_signs(
    things: Things,
    focus: Res<ChiefFocus>,
    mut signs: ResMut<Signs>,
    mut draw: Draw,
) {
    if signs.stale {
        signs.put_up(things.sign_words(&focus.chief), &mut draw);
    }
}

/// Takes the town of the hamlet the Inspector carries in hand, to be
/// set down with a click on the ground.
fn take_carry(
    mut events: MessageReader<EventReceived>,
    bridge: Res<Bridge>,
    mut carrying: ResMut<Carrying>,
) {
    for event in events.read() {
        match decode::<String>(&event.payload) {
            Ok(hamlet) => {
                carrying.carry =
                    bridge.town(&hamlet).map(|town| Carry {
                        town,
                        hamlet,
                        to: None,
                        fits: Err(Unfit::Crowded),
                    });
            }
            Err(why) => {
                hamlet_engine_bevy::warn(&format!("carry: {why}"))
            }
        }
    }
}

/// Answers what the person did over the view.
fn steer(
    mut inputs: MessageReader<SceneInput>,
    keys: Query<&SceneKey>,
    tuning: Res<WorldTuning>,
    land: Res<Land>,
    society: Res<Society>,
    mut camera: ResMut<Camera>,
    mut picking: ResMut<Picking>,
    mut carrying: ResMut<Carrying>,
) {
    let thing = |entity: Entity| {
        keys.get(entity).ok().and_then(|key| Thing::of(key.0))
    };
    let set = &tuning.camera;
    for input in inputs.read() {
        camera.input(set, input);
        let still = camera.travel <= set.click_slop;
        let carrying = &mut *carrying;
        let Some(carry) = &mut carrying.carry else {
            picking.input(input, still, thing);
            continue;
        };
        // A town in hand takes the clicks: the left button sets it
        // down where it fits, the right puts it back. The pointer is
        // still followed.
        let SceneInput::Press(press) = input else {
            if let SceneInput::Over(_) | SceneInput::Out(_) = input {
                picking.input(input, still, thing);
            }
            continue;
        };
        match (press.button, press.down) {
            (MouseButton::Primary, false) if still => {
                carry.reach(&land, &society, camera.ray(set));
                if let Some(command) = carry.command() {
                    carrying.set_down =
                        Some((carry.hamlet.clone(), command));
                    carrying.carry = None;
                    picking.pass_click();
                }
            }
            (MouseButton::Secondary, false) if still => {
                carrying.carry = None;
            }
            _ => {}
        }
    }
}

/// Moves the realm's clock on by the frame.
fn advance(time: Res<Time>, mut clock: ResMut<Clock>) {
    clock.dt = time.delta_secs().min(LONGEST_FRAME);
    clock.now += clock.dt;
}

/// Points the camera, on the thing followed until the ground is slid
/// by hand.
fn glide(
    things: Things,
    sky: Res<Sky>,
    clock: Res<Clock>,
    mut camera: ResMut<Camera>,
    mut following: ResMut<Following>,
    mut draw: Draw,
) {
    if camera.following.is_none() {
        following.0 = None;
    }
    if let Some(thing) = following.0 {
        camera.following = things.place(thing);
    }
    camera.tick(
        &things.tuning.camera,
        &things.land,
        &sky.look,
        clock.dt,
        &mut draw,
    );
}

/// Looks about as the frame begins: where the camera stands, who the
/// chief talks to, the yards and the ground in sight, and what the
/// frame may hand over.
fn look(
    things: Things,
    camera: Res<Camera>,
    focus: Res<ChiefFocus>,
    mut upload: ResMut<Upload>,
    mut now: ResMut<Now>,
) {
    *upload = Upload::frame();
    let (tuning, land) = (&things.tuning, &things.land);
    let (view, ahead) = camera.views();
    *now = Now {
        eye: camera.eye(land),
        beside: focus
            .with
            .as_ref()
            .and_then(|with| things.talking_to(with))
            .and_then(|agent| {
                Some((agent, things.robots.at(agent)?))
            }),
        lots: things.robots.lots(),
        sights: [Some(view), ahead]
            .into_iter()
            .flatten()
            .map(|view| {
                view.sight(land, &tuning.camera, &tuning.detail)
            })
            .collect(),
    };
}

/// Finds the robot under the pointer by its box, through whatever
/// stands in front of it.
fn aim(
    things: Things,
    camera: Res<Camera>,
    mut picking: ResMut<Picking>,
) {
    let set = &things.tuning;
    let robot = camera.ray(&set.camera).and_then(|(from, way)| {
        things.robots.under(from, way, set.pick.robot_box)
    });
    if picking.hovered_robot() != robot {
        picking.aim(robot);
    }
}

/// Lays the rings about the thing under the pointer and the one
/// chosen.
fn lay_rings(
    things: Things,
    now: Res<Now>,
    mut picking: ResMut<Picking>,
    mut draw: Draw,
) {
    let rings = [picking.hovered(), picking.selected()]
        .map(|thing| thing.and_then(|thing| things.ring(thing)));
    let floor = Floor::new(&things.land, &now.lots);
    picking.lay(
        rings[0],
        rings[1],
        &floor,
        &things.tuning.pick,
        &mut draw,
    );
}

/// Follows the pointer with the town in hand, and shows where it
/// would stand.
fn show_footprint(
    tuning: Res<WorldTuning>,
    land: Res<Land>,
    society: Res<Society>,
    camera: Res<Camera>,
    mut carrying: ResMut<Carrying>,
    mut footprint: ResMut<Footprint>,
    mut draw: Draw,
) {
    let ray = camera.ray(&tuning.camera);
    if let Some(carry) = &mut carrying.carry {
        // A town that is gone is in nobody's hand.
        if society.town(carry.town).is_none() {
            carrying.carry = None;
        } else {
            carry.reach(&land, &society, ray);
        }
    }
    footprint.show(
        carrying.carry.as_ref(),
        &land,
        &society,
        &tuning.carry,
        &mut draw,
    );
}

/// Shows the signs near the camera, and the tag over the thing under
/// the pointer.
fn show_signs(
    things: Things,
    picking: Res<Picking>,
    camera: Res<Camera>,
    focus: Res<ChiefFocus>,
    mut signs: ResMut<Signs>,
    mut draw: Draw,
) {
    let tag = things.tag(picking.hovered(), &focus.chief);
    signs.show(
        camera.eye(&things.land),
        things.tuning.pick.signs_far,
        tag,
        &mut draw,
    );
}

/// Tells the agents what the person chose and where a town was set
/// down.
fn settle(
    mut commands: Commands,
    society: Res<Society>,
    bridge: Res<Bridge>,
    mut picking: ResMut<Picking>,
    mut carrying: ResMut<Carrying>,
) {
    let choice =
        picking.take_choice().and_then(|thing| match thing {
            None => Some(Selected::Nothing),
            Some(thing) => thing.selects(&society, &bridge),
        });
    if let Some(selected) = choice {
        // The bare ground only clears what was selected.
        let something = selected != Selected::Nothing;
        commands.agents(&AgentsRequest::Select(selected));
        if something {
            open(INSPECTOR);
        }
    }
    if let Some((hamlet, text)) = carrying.set_down.take() {
        commands.agents(&AgentsRequest::Decree { hamlet, text });
    }
}

/// Keeps the pointer over the view: a town in hand, faint where it
/// can be set down and refused where it cannot, then a drag of the
/// camera, then a thing that can be picked.
fn point(
    tuning: Res<WorldTuning>,
    carrying: Res<Carrying>,
    camera: Res<Camera>,
    picking: Res<Picking>,
    mut pointer: ResMut<Pointer>,
) {
    let now = match &carrying.carry {
        Some(carry) => Some(match carry.fits {
            Ok(()) => NodePointer::new(PointerMode::Carry).faint(),
            Err(_) => NodePointer::new(PointerMode::Refuse),
        }),
        None => match camera.dragging(&tuning.camera) {
            Some(mode) => Some(NodePointer::new(mode)),
            None => picking
                .hovered()
                .map(|_| NodePointer::new(PointerMode::Pick)),
        },
    };
    if pointer.0 != now {
        pointer.0 = now;
    }
}

/// Goes back to the framing of every hamlet.
pub fn reset_camera(
    camera: Option<ResMut<Camera>>,
    mut following: ResMut<Following>,
) {
    if let Some(mut camera) = camera {
        camera.reset();
        following.0 = None;
    }
}

/// Brings the thing selected to the middle of the view, and stays on
/// it if `stay`.
pub fn focus_on_selected(
    stay: In<bool>,
    things: Option<Things>,
    picking: Option<Res<Picking>>,
    camera: Option<ResMut<Camera>>,
    mut following: ResMut<Following>,
) {
    if let (Some(things), Some(picking), Some(mut camera)) =
        (things, picking, camera)
    {
        things.focus_on(&picking, &mut camera, &mut following, *stay);
    }
}

impl Things<'_> {
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

    /// True when `agent` holds the office of the first hamlet, the
    /// one the window's own chat is with.
    fn first_mayor(&self, agent: AgentId) -> bool {
        self.bridge
            .town(HOME)
            .and_then(|town| self.society.town(town))
            .is_some_and(|town| town.leader == agent)
    }

    /// The signs over every house, common and the Hearth, the
    /// Hearth's named for `chief`.
    fn sign_words(&self, chief: &str) -> Vec<(Thing, Words)> {
        let (pick, set) = (&self.tuning.pick, &self.tuning.chief);
        let land = &self.land;
        let mut signs = Vec::new();
        for agent in &self.society.agents {
            // A house that is gone answers only as a ruin.
            let ruin =
                agent.remains(&land.set.house) == Some(Remains::Ruin);
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
                let at = land.on_ground(at) + Vec3::Y * pick.house.1;
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
                let at = land.on_ground(at) + Vec3::Y * pick.common.1;
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
            let says = format!("{chief}'s Hearth").to_uppercase();
            let at = at + Vec3::Y * set.house.1;
            signs.push((Thing::Hearth, Words { at, says }));
        }
        if let Some(at) = self.library.at() {
            let at = at + Vec3::Y * library::SIZE.1;
            let says = "THE LIBRARY".to_owned();
            signs.push((Thing::Library, Words { at, says }));
        }
        signs
    }

    /// The words over `hovered`, the thing under the pointer, when it
    /// has no sign of its own.
    fn tag(
        &self,
        hovered: Option<Thing>,
        chief: &str,
    ) -> Option<Words> {
        let (pick, set) = (&self.tuning.pick, &self.tuning.chief);
        let (at, says) = match hovered? {
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
                self.chief.mascot()? + Vec3::Y * set.mascot_box.y,
                format!("{chief}, Chief"),
            ),
            Thing::House(_)
            | Thing::Common(_)
            | Thing::Hearth
            | Thing::Library => {
                return None;
            }
        };
        Some(Words { at, says })
    }

    /// The ground a ring about `thing` lies on, and its radius.
    fn ring(&self, thing: Thing) -> Option<(Vec3, f32)> {
        let at = self.place(thing)?;
        let (pick, set) = (&self.tuning.pick, &self.tuning.chief);
        let radius = match thing {
            Thing::Robot(_) => {
                pick.robot_box.x.max(pick.robot_box.z) * 0.62
            }
            Thing::House(_) => pick.house.0,
            Thing::Common(_) => pick.common.0,
            Thing::Mascot => {
                set.mascot_box.x.max(set.mascot_box.z) * 0.62
            }
            Thing::Hearth => set.house.0,
            Thing::Library => library::SIZE.0,
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
            Thing::Library => self.library.at().map(|at| at.xz()),
        }
    }

    /// Brings the thing `picking` has selected to the middle of the
    /// view, and stays on it if `stay`.
    fn focus_on(
        &self,
        picking: &Picking,
        camera: &mut Camera,
        following: &mut Following,
        stay: bool,
    ) {
        let Some(thing) = picking.selected() else {
            return;
        };
        if let Some(at) = self.place(thing) {
            camera.focus_on(&self.tuning.camera, at, stay);
            following.0 = stay.then_some(thing);
        }
    }
}
