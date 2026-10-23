//! The realm as one: the land and the society drawn from the roster,
//! and each part of the scene kept in step with them, a little each
//! frame within what the engine allows.

use std::path::Path;

use bevy_ecs::system::SystemParam;
use glam::{Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Focus, Roster, Selected,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Animate, KeyCode, MouseButton};
use hamlet_engine_bevy::ui::{NodePointer, PointerMode, open};
use hamlet_engine_bevy::{Reply, decode};

use crate::bridge::Bridge;
use crate::build::mark;
use crate::build::stock::Stock;
use crate::build::upload::{Draw, Upload};
use crate::folk::chief::{Chief, move_chief};
use crate::folk::hand::Hand;
use crate::folk::keeper::{Floor, Lot, Robots, move_robots};
use crate::places::house::Remains;
use crate::places::library::{self, Library, build_library};
use crate::places::moving::{Carry, Carrying, Footprint, SetDown};
use crate::places::road::{Roads, build_roads};
use crate::places::town::{Towns, build_towns};
use crate::society::{AgentId, Place, Society};
use crate::sound::{Cue, Cues, Mix, Sound};
use crate::tuning::WorldTuning;
use crate::view::camera::{Camera, Following};
use crate::view::pick::{Picking, Thing};
use crate::view::signs::{Signs, Words};
use crate::world::ground::{Ground, Sight, build_ground};
use crate::world::land::Land;
use crate::world::layout::plot;
use crate::world::palette::Palette;
use crate::world::sky::{Lights, Sky, SkyClock};

/// The land is grown from this seed, as the realm has always been.
const SEED: u64 = 7;

/// The first hamlet, the home itself, whose mayor the window's own
/// chat is with.
pub const HOME: &str = "home";

/// The Inspector, brought forward when something is chosen here.
pub const INSPECTOR: &str = "hamlet.agents/inspector";

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
        .init_resource::<Signs>()
        .init_resource::<SkyClock>()
        .init_resource::<ChiefFocus>()
        .add_message::<SetDown>()
        .add_plugins((
            crate::sound::plugin,
            crate::view::kept::plugin,
        ))
        .add_systems(Startup, make)
        // What is played is played in the call it is heard in, a
        // frame of the realm or not.
        .configure_sets(
            Update,
            (Hear, Tick.run_if(ticked), Cues.run_if(ticked), Mix)
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
                set_down,
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
pub struct Hear;

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
    /// The ground of each robot's yard.
    pub lots: Vec<Lot>,
    /// The sight of the ground of the camera's view, and of the one
    /// it is going to.
    pub sights: Vec<Sight>,
}

/// The name the chief at this window signs in with.
#[derive(Resource, Default)]
pub struct ChiefFocus {
    pub chief: String,
}

/// The pointer over the realm's view.
#[derive(Resource, Default)]
pub struct Pointer(pub Option<NodePointer>);

/// The things of the realm and where they are.
#[derive(SystemParam)]
pub struct Things<'w> {
    pub tuning: Res<'w, WorldTuning>,
    pub land: Res<'w, Land>,
    pub society: Res<'w, Society>,
    pub bridge: Res<'w, Bridge>,
    pub robots: Res<'w, Robots>,
    pub chief: Res<'w, Chief>,
    pub library: Res<'w, Library>,
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
    mut cues: MessageWriter<Cue>,
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
        if chief_focus.chief != focus.chief {
            chief_focus.chief.clone_from(&focus.chief);
            signs.stale = true;
        }
        // A hamlet, the Hearth or the library chosen elsewhere is
        // brought into view.
        if let Some(
            thing @ (Thing::Common(_)
            | Thing::Hearth
            | Thing::Library),
        ) = picking.follow(focus, &things.bridge)
        {
            things.focus_on(
                thing,
                &mut camera,
                &mut following,
                false,
                &mut cues,
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
        let far = things.tuning.pick.signs_far;
        signs.put_up(things.sign_words(&focus.chief), far, &mut draw);
    }
}

/// Takes the town of the hamlet the Inspector carries in hand, to be
/// set down with a click on the ground.
fn take_carry(
    mut events: MessageReader<EventReceived>,
    bridge: Res<Bridge>,
    land: Res<Land>,
    society: Res<Society>,
    mut carrying: ResMut<Carrying>,
    mut cues: MessageWriter<Cue>,
) {
    for event in events.read() {
        match decode::<String>(&event.payload) {
            Ok(hamlet) => {
                if let Some(town) = bridge.town(&hamlet) {
                    lift(
                        Place::Town(town),
                        &land,
                        &society,
                        &mut carrying,
                        &mut cues,
                    );
                }
            }
            Err(why) => {
                hamlet_engine_bevy::warn(&format!("carry: {why}"))
            }
        }
    }
}

/// Takes `place` in hand, to be set down with the next click on the
/// ground.
fn lift(
    place: Place,
    land: &Land,
    society: &Society,
    carrying: &mut Carrying,
    cues: &mut MessageWriter<Cue>,
) {
    let Some((at, _)) = society.stands(land, place) else {
        return;
    };
    carrying.carry = Some(Carry::new(place));
    cues.write(Cue {
        sound: Sound::Lift,
        at: land.on_ground(at),
        turn: at.x.to_bits(),
    });
}

/// Takes the chief's Hearth or the library in hand, to be set down
/// with the next click on the ground.
pub fn pick_up(
    place: In<Place>,
    land: Option<Res<Land>>,
    society: Option<Res<Society>>,
    mut carrying: ResMut<Carrying>,
    mut cues: MessageWriter<Cue>,
) {
    if let (Some(land), Some(society)) = (land, society) {
        lift(*place, &land, &society, &mut carrying, &mut cues);
    }
}

/// Answers what the person did over the view.
pub fn steer(
    mut inputs: MessageReader<SceneInput>,
    keys: Query<&SceneKey>,
    tuning: Res<WorldTuning>,
    land: Res<Land>,
    society: Res<Society>,
    mut camera: ResMut<Camera>,
    mut picking: ResMut<Picking>,
    mut carrying: ResMut<Carrying>,
    mut hand: ResMut<Hand>,
    mut following: ResMut<Following>,
    mut commands: Commands,
) {
    let thing = |entity: Entity| {
        keys.get(entity).ok().and_then(|key| Thing::of(key.0))
    };
    let set = &tuning.camera;
    for input in inputs.read() {
        if let SceneInput::Key(key) = input {
            if key.down && key.code == KeyCode::Escape {
                // Escape lets go of one thing at a time: the place in
                // hand, what is followed, then what is selected.
                if carrying.carry.is_some() {
                    carrying.carry = None;
                } else if camera.following.is_some() {
                    camera.following = None;
                    following.0 = None;
                } else if picking.selected().is_some()
                    || picking.also().is_some()
                {
                    picking.clear();
                    commands.agents(&AgentsRequest::Select(
                        Selected::Nothing,
                    ));
                }
            }
            continue;
        }
        // A robot in the hand holds the ground still, and the release
        // that sets it down is the hand's.
        let input = &match carrying.carry {
            Some(_) => *input,
            None => match hand.input(
                input,
                picking.hovered_robot(),
                set.click_slop,
            ) {
                Some(input) => input,
                None => {
                    // The camera still hears the button let go, or
                    // it slides on with the pointer after the drop.
                    camera.input(set, input);
                    picking.pass_click();
                    continue;
                }
            },
        };
        camera.input(set, input);
        let still = camera.travel <= set.click_slop;
        let carrying = &mut *carrying;
        let Some(carry) = &mut carrying.carry else {
            picking.input(input, still, thing);
            continue;
        };
        // A place in hand takes the clicks: the left button sets it
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
                if let Some(set_down) = carry.set_down() {
                    carrying.set_down = Some(set_down);
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

/// Tells where a place was set down: a town to its hamlet as a
/// chief's command, the Hearth and the library as a [`SetDown`].
fn set_down(
    mut commands: Commands,
    land: Res<Land>,
    bridge: Res<Bridge>,
    mut carrying: ResMut<Carrying>,
    mut set_downs: MessageWriter<SetDown>,
    mut cues: MessageWriter<Cue>,
) {
    let Some(set) = carrying.set_down.take() else {
        return;
    };
    match set.place {
        Place::Town(town) => {
            let Some(hamlet) = bridge.hamlet(town) else {
                return;
            };
            let (x, z) = (set.to.x, set.to.y);
            commands.agents(&AgentsRequest::Decree {
                text: format!("/move-hamlet {hamlet} {x:.1} {z:.1}"),
                hamlet: hamlet.to_owned(),
            });
        }
        Place::Chief | Place::Library => {
            set_downs.write(set);
        }
    }
    cues.write(Cue {
        sound: Sound::Drop,
        at: land.on_ground(set.to),
        turn: set.to.x.to_bits(),
    });
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

/// Looks about as the frame begins: where the camera stands, the
/// yards and the ground in sight, and what the frame may hand over.
fn look(
    things: Things,
    camera: Res<Camera>,
    mut upload: ResMut<Upload>,
    mut now: ResMut<Now>,
) {
    *upload = Upload::frame();
    let (tuning, land) = (&things.tuning, &things.land);
    let (view, ahead) = camera.views();
    *now = Now {
        eye: camera.eye(land),
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
    let rings =
        [picking.hovered(), picking.selected(), picking.also()]
            .map(|thing| thing.and_then(|thing| things.ring(thing)));
    let floor = Floor::new(&things.land, &now.lots);
    picking.lay(rings, &floor, &things.tuning.pick, &mut draw);
}

/// Follows the pointer with the place in hand, and shows where it
/// would stand.
fn show_footprint(
    tuning: Res<WorldTuning>,
    land: Res<Land>,
    society: Res<Society>,
    camera: Res<Camera>,
    pal: Res<Palette>,
    mut carrying: ResMut<Carrying>,
    mut footprint: ResMut<Footprint>,
    mut words: ResMut<PointerWords>,
    mut draw: Draw,
) {
    let ray = camera.ray(&tuning.camera);
    if let Some(carry) = &mut carrying.carry {
        // A place that is gone is in nobody's hand.
        if society.stands(&land, carry.place).is_none() {
            carrying.carry = None;
        } else {
            carry.reach(&land, &society, ray);
        }
    }
    let said = carrying
        .carry
        .as_ref()
        .map(|carry| carry.words(&tuning.carry, &pal));
    if words.0.as_ref().map(|now| &now.text)
        != said.as_ref().map(|said| &said.text)
    {
        words.0 = said;
    }
    footprint.show(
        carrying.carry.as_ref(),
        &land,
        &society,
        &tuning.carry,
        &mut draw,
    );
}

/// Tells the agents what the person chose.
fn settle(
    mut commands: Commands,
    things: Things,
    mut picking: ResMut<Picking>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    let Some(chosen) = picking.take_choice() else {
        return;
    };
    let selected = match chosen {
        None => Some(Selected::Nothing),
        Some(thing) => thing.selects(&things.society, &things.bridge),
    };
    if let Some(at) = chosen.and_then(|thing| things.place(thing)) {
        *turn = turn.wrapping_add(1);
        cues.write(Cue {
            sound: Sound::Select,
            at: things.land.on_ground(at),
            turn: *turn,
        });
    }
    if let Some(selected) = selected {
        // The bare ground only clears what was selected.
        let something = selected != Selected::Nothing;
        commands.agents(&AgentsRequest::Select(selected));
        if something {
            open(INSPECTOR);
        }
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
    hand: Res<Hand>,
    mut pointer: ResMut<Pointer>,
) {
    let now = match &carrying.carry {
        Some(carry) => Some(match carry.fits {
            Ok(()) => NodePointer::new(PointerMode::Carry).faint(),
            Err(_) => NodePointer::new(PointerMode::Refuse),
        }),
        // A robot in the hand is seen through it.
        None if hand.lifted().is_some() => {
            Some(NodePointer::new(PointerMode::Carry).faint())
        }
        None => match camera.dragging(&tuning.camera) {
            Some(mode) => Some(NodePointer::new(mode)),
            None if hand.ready() => {
                Some(NodePointer::new(PointerMode::Grab))
            }
            None => picking
                .hovered()
                .map(|_| NodePointer::new(PointerMode::Pick)),
        },
    };
    if pointer.0 != now {
        pointer.0 = now;
    }
}

/// Goes back to the framing of every hamlet, or of the thing
/// followed.
pub fn reset_camera(camera: Option<ResMut<Camera>>) {
    if let Some(mut camera) = camera {
        camera.reset();
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
    mut cues: MessageWriter<Cue>,
) {
    if let (Some(things), Some(picking), Some(mut camera)) =
        (things, picking, camera)
        && let Some(thing) = picking.selected()
    {
        things.focus_on(
            thing,
            &mut camera,
            &mut following,
            *stay,
            &mut cues,
        );
    }
}

/// Brings `thing` to the middle of the view, and stays on it if
/// `stay`.
pub fn focus_on_thing(
    focus: In<(Thing, bool)>,
    things: Option<Things>,
    camera: Option<ResMut<Camera>>,
    mut following: ResMut<Following>,
    mut cues: MessageWriter<Cue>,
) {
    let (thing, stay) = *focus;
    if let (Some(things), Some(mut camera)) = (things, camera) {
        things.focus_on(
            thing,
            &mut camera,
            &mut following,
            stay,
            &mut cues,
        );
    }
}

impl Things<'_> {
    /// True when `agent` holds the office of the first hamlet, the
    /// one the window's own chat is with.
    pub fn first_mayor(&self, agent: AgentId) -> bool {
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
        // A workbench is signed with the folder its hamlet works on.
        for town in &self.society.towns {
            let folder = self
                .bridge
                .record(town.id)
                .and_then(|hamlet| hamlet.folder.as_deref())
                .map(|folder| {
                    Path::new(folder).file_name().map_or_else(
                        || folder.to_owned(),
                        |name| name.to_string_lossy().into_owned(),
                    )
                });
            if let Some(folder) = folder {
                let works = &self.tuning.works;
                let at = works.bench(land, town.site)
                    + Vec3::Y * works.bench_room().1;
                signs.push((
                    Thing::Workbench(town.id),
                    Words {
                        at,
                        says: folder.to_uppercase(),
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
            Thing::Workbench(_) => self.tuning.works.bench_room().0,
            Thing::Clockwork(_) => {
                self.tuning.works.clockwork_radius()
            }
        };
        let ground =
            self.land.on_ground(at) + Vec3::Y * pick.ring_lift;
        Some((ground, radius))
    }

    /// The place of `thing` on the ground.
    pub fn place(&self, thing: Thing) -> Option<Vec2> {
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
            Thing::Workbench(town) => {
                let town = self.society.town(town)?;
                Some(
                    self.tuning
                        .works
                        .bench(&self.land, town.site)
                        .xz(),
                )
            }
            Thing::Clockwork(town) => {
                let town = self.society.town(town)?;
                Some(
                    self.tuning
                        .works
                        .clockwork(&self.land, town.site)
                        .xz(),
                )
            }
            Thing::Mascot => self.chief.mascot().map(|at| at.xz()),
            Thing::Hearth => self.chief.hearth().map(|at| at.xz()),
            Thing::Library => self.library.at().map(|at| at.xz()),
        }
    }

    /// Brings `thing` to the middle of the view, and stays on it if
    /// `stay`.
    fn focus_on(
        &self,
        thing: Thing,
        camera: &mut Camera,
        following: &mut Following,
        stay: bool,
        cues: &mut MessageWriter<Cue>,
    ) {
        if let Some(at) = self.place(thing) {
            camera.focus_on(&self.tuning.camera, at, stay);
            following.0 = stay.then_some(thing);
            cues.write(Cue {
                sound: Sound::Focus,
                at: Vec3::new(at.x, 0.0, at.y),
                turn: 0,
            });
        }
    }
}
