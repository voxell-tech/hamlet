use std::collections::HashSet;
use std::fmt::Debug;
use std::time::Duration;

use bevy::camera::NormalizedRenderTarget;
use bevy::picking::backend::HitData;
use bevy::picking::pointer::{Location, PointerId};
use bevy::time::TimeUpdateStrategy;
use hamlet_client::ClientPlugin;
use hamlet_client::realm::{Busy, Road, Settlement};
use hamlet_protocol::v0;
use hamlet_testing::realm;

use super::pool::gauge::shade;
use super::toys::{Kind, Toy};
use super::*;
use crate::Clock;
use crate::bubble::{self, Pinned};
use crate::generator::Common;
use crate::generator::land::Land;
use crate::generator::society::{AgentId, TownId};
use crate::generator::testing::{rehearse, stand_in};
use crate::hamlet::Sited;
use crate::hamlet::tests::{camera, receive_change, receive_state};
use crate::keeper::Keeper;
use crate::pick::{PickSettings, WorldSurface};
use crate::play::{Play, Played};
use crate::sky::CameraSettings;

/// The deeds the hand told the world of and its wishes of the core.
#[derive(Resource, Default)]
struct Log {
    played: Vec<Play>,
    wishes: Vec<Wish>,
}

fn collect(
    mut played: MessageReader<Played>,
    mut wishes: MessageReader<Wish>,
    mut log: ResMut<Log>,
) {
    log.played.extend(played.read().map(|played| played.what));
    log.wishes.extend(wishes.read().cloned());
}

/// A world of robots at their porches, each with its agent, the box
/// a pointer hits and nothing drawn.
struct Cast {
    app: App,
    agents: Vec<Entity>,
    robots: Vec<Entity>,
    boxes: Vec<Entity>,
    surface: Entity,
}

impl Cast {
    fn new(count: u32) -> Self {
        let mut app = App::new();
        rehearse(&mut app);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f32(1.0 / 60.0),
        ))
        .init_resource::<Hovered>()
        .init_resource::<CameraSettings>()
        .init_resource::<PickSettings>()
        .init_resource::<Log>()
        .add_message::<Played>()
        .add_systems(Update, bubble::unpin)
        .add_systems(PostUpdate, collect);
        app.insert_resource(hamlet_client::realm::Deeds(vec![
            "give".into(),
            "road".into(),
        ]))
        .add_plugins(plugin);
        let (mut agents, mut robots, mut boxes) =
            (vec![], vec![], vec![]);
        for i in 1..=count {
            let world = app.world_mut();
            let agent = world.spawn(Housed(AgentId(i))).id();
            let robot = stand_in(world, AgentId(i), i);
            let hit = world
                .spawn((
                    Thing::Resident(agent),
                    ChildOf(robot),
                    Pickable::default(),
                    Transform::default(),
                ))
                .id();
            agents.push(agent);
            robots.push(robot);
            boxes.push(hit);
        }
        let surface = app.world_mut().spawn(WorldSurface).id();
        let mut cast = Cast {
            app,
            agents,
            robots,
            boxes,
            surface,
        };
        cast.run(0.5);
        cast
    }

    /// Runs `secs` of frames. Returns every motion the first robot
    /// was seen in, and the furthest it moved in a frame.
    fn run(&mut self, secs: f32) -> (Vec<Act>, f32) {
        let (mut acts, mut furthest) = (Vec::new(), 0.0f32);
        for _ in 0..(secs * 60.0).round() as usize {
            let before = self.at(0);
            self.app.world_mut().resource_mut::<Clock>().t +=
                1.0 / 60.0;
            self.app.update();
            furthest = furthest.max(before.distance(self.at(0)));
            let act = self.act(0);
            if !acts.contains(&act) {
                acts.push(act);
            }
        }
        (acts, furthest)
    }

    fn at(&self, i: usize) -> Vec3 {
        let robot = self.robots[i];
        self.app
            .world()
            .get::<Transform>(robot)
            .unwrap()
            .translation
    }

    fn act(&self, i: usize) -> Act {
        self.app
            .world()
            .get::<Keeper>(self.robots[i])
            .unwrap()
            .act()
    }

    fn playing(&self, i: usize) -> Option<Act> {
        self.app
            .world()
            .get::<Passage>(self.robots[i])
            .map(Passage::act)
    }

    fn porch(&self, i: usize) -> Vec3 {
        self.app
            .world()
            .get::<Robot>(self.robots[i])
            .unwrap()
            .porch()
    }

    fn set(&mut self) -> Mut<'_, PlaySettings> {
        self.app.world_mut().resource_mut::<PlaySettings>()
    }

    /// The deeds played since this was last asked.
    fn played(&mut self) -> Vec<Play> {
        let mut log = self.app.world_mut().resource_mut::<Log>();
        std::mem::take(&mut log.played)
    }

    /// The deeds played, the selecting that every click does aside.
    fn played_at(&mut self) -> Vec<Play> {
        let mut played = self.played();
        played.retain(|what| *what != Play::Select);
        played
    }

    fn wishes(&mut self) -> Vec<Wish> {
        let mut log = self.app.world_mut().resource_mut::<Log>();
        std::mem::take(&mut log.wishes)
    }

    fn pointer<E: Debug + Clone + Reflect>(
        &mut self,
        target: Entity,
        event: E,
    ) {
        let at = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        let world = self.app.world_mut();
        world.trigger(Pointer::new(
            PointerId::Mouse,
            at,
            event,
            target,
        ));
        world.flush();
    }

    fn hit(&self) -> HitData {
        HitData::new(self.surface, 0.0, None, None)
    }

    fn click(&mut self, target: Entity) {
        let event = Click {
            button: PointerButton::Primary,
            hit: self.hit(),
            duration: Duration::ZERO,
            count: 1,
        };
        self.pointer(target, event);
    }

    fn press(&mut self) {
        let event = Press {
            button: PointerButton::Primary,
            hit: self.hit(),
            count: 1,
        };
        self.app.world_mut().resource_mut::<Travel>().0 = 0.0;
        self.pointer(self.surface, event);
    }

    fn release(&mut self) {
        let event = Release {
            button: PointerButton::Primary,
            hit: self.hit(),
        };
        self.pointer(self.surface, event);
    }

    /// Puts the pointer on `target`, which is no thing, or on
    /// nothing.
    fn over(&mut self, target: Option<Entity>) {
        let hit = self.hit();
        let mut over =
            self.app.world_mut().resource_mut::<HoverMap>();
        over.clear();
        if let Some(target) = target {
            over.entry(PointerId::Mouse)
                .or_default()
                .insert(target, hit);
        }
    }

    fn hover(&mut self, thing: Option<Thing>) {
        self.app.world_mut().resource_mut::<Hovered>().0 = thing;
    }

    /// Puts the pointer straight over `at` on the ground.
    fn point(&mut self, at: Vec3) {
        let mut reach = self.app.world_mut().resource_mut::<Reach>();
        reach.ray =
            Some(Ray3d::new(at + Vec3::Y * 40.0, Dir3::NEG_Y));
    }

    fn thing(&self, i: usize) -> Thing {
        Thing::Resident(self.agents[i])
    }

    /// Presses on the robot `i` and drags it over `to`.
    fn lift(&mut self, i: usize, to: Vec3) {
        let thing = self.thing(i);
        self.hover(Some(thing));
        self.press();
        self.app.world_mut().resource_mut::<Travel>().0 = 30.0;
        self.point(to);
        self.run(0.6);
    }

    fn pickable(&self, i: usize) -> bool {
        self.app.world().get::<Pickable>(self.boxes[i]).is_some()
    }
}

#[test]
fn two_quick_clicks_poke_a_robot_and_one_click_does_not() {
    let mut cast = Cast::new(1);
    let hit = cast.boxes[0];
    cast.click(hit);
    cast.run(1.0);
    assert_eq!(cast.played(), [Play::Select]);
    assert_eq!(cast.playing(0), None);

    // Slower than a double click: two single clicks.
    cast.click(hit);
    cast.run(1.0);
    assert_eq!(cast.played(), [Play::Select]);

    cast.click(hit);
    cast.run(0.2);
    cast.click(hit);
    let (acts, _) = cast.run(0.3);
    assert_eq!(cast.played_at(), [Play::Poke]);
    assert_eq!(cast.playing(0), Some(Act::Startle));
    assert!(acts.contains(&Act::Startle));
    // It ends, and the robot is as it was.
    cast.run(3.0);
    assert_eq!(cast.playing(0), None);
    assert_eq!(cast.act(0), Act::Idle);
    assert!(cast.at(0).distance(cast.porch(0)) < 0.02);
}

#[test]
fn a_click_with_shift_or_after_a_drag_is_no_poke() {
    let mut cast = Cast::new(1);
    let hit = cast.boxes[0];
    cast.app.world_mut().resource_mut::<Travel>().0 = 40.0;
    cast.click(hit);
    cast.click(hit);
    cast.run(0.2);
    assert!(cast.played_at().is_empty());

    cast.app.world_mut().resource_mut::<Travel>().0 = 0.0;
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ShiftLeft);
    cast.app.insert_resource(keys);
    cast.click(hit);
    cast.click(hit);
    cast.run(0.2);
    assert!(cast.played_at().is_empty());
}

#[test]
fn poked_too_often_a_robot_sulks_and_is_left_alone() {
    let mut cast = Cast::new(1);
    let hit = cast.boxes[0];
    let pokes = cast.set().annoy_pokes;
    for _ in 0..pokes {
        cast.click(hit);
        cast.click(hit);
        cast.run(0.3);
    }
    let played = cast.played_at();
    assert_eq!(played.len(), pokes);
    assert_eq!(played.last(), Some(&Play::Annoy));
    assert!(played[..pokes - 1].iter().all(|p| *p == Play::Poke));
    assert_eq!(cast.playing(0), Some(Act::Sulk));

    // It answers nothing while it sulks.
    cast.click(hit);
    cast.click(hit);
    cast.run(0.5);
    assert!(cast.played_at().is_empty());
    assert_eq!(cast.playing(0), Some(Act::Sulk));

    cast.run(4.0);
    assert_eq!(cast.playing(0), None);
    assert_eq!(cast.act(0), Act::Idle);
    // Then it can be poked again, and takes it well.
    cast.click(hit);
    cast.click(hit);
    cast.run(0.2);
    assert_eq!(cast.played_at(), [Play::Poke]);
}

#[test]
fn a_pointer_that_rests_on_a_robot_pats_it_and_one_passing_does_not()
{
    let mut cast = Cast::new(1);
    let thing = cast.thing(0);
    let hold = cast.set().pat_hold;
    let at = |cast: &mut Cast, at: Vec2| {
        cast.app.world_mut().resource_mut::<Reach>().at = Some(at);
    };

    // Passing: it moves on all the while.
    cast.hover(Some(thing));
    for i in 0..(hold * 2.0 * 60.0) as usize {
        at(&mut cast, Vec2::new(100.0 + i as f32 * 3.0, 80.0));
        cast.run(1.0 / 60.0);
    }
    assert!(cast.played_at().is_empty());

    // Resting.
    at(&mut cast, Vec2::new(300.0, 80.0));
    cast.run(hold - 0.2);
    assert!(cast.played_at().is_empty());
    let (acts, _) = cast.run(0.4);
    assert_eq!(cast.played_at(), [Play::Pat]);
    assert!(acts.contains(&Act::Wiggle));
    let world = cast.app.world_mut();
    let hearts = world
        .query::<&Toy>()
        .iter(world)
        .filter(|toy| toy.kind == Kind::Heart)
        .count();
    assert_eq!(hearts, 1);

    // Resting on is not patted over and over.
    cast.run(hold * 2.0);
    assert!(cast.played_at().is_empty());
    assert_eq!(cast.playing(0), None);
    assert_eq!(cast.act(0), Act::Idle);
    // The heart is gone.
    let world = cast.app.world_mut();
    assert_eq!(world.query::<&Toy>().iter(world).count(), 0);

    // A stroke, back and forth, pats it sooner.
    let wait = cast.set().stroke_rest;
    cast.run(wait);
    for i in 0..40 {
        let x = 300.0 + 30.0 * (i as f32 * 0.8).sin();
        at(&mut cast, Vec2::new(x, 80.0));
        cast.run(1.0 / 60.0);
    }
    assert_eq!(cast.played_at(), [Play::Pat]);

    // Off the robot, nothing.
    cast.hover(None);
    cast.run(hold * 2.0);
    assert!(cast.played_at().is_empty());
}

#[test]
fn a_dragged_robot_is_lifted_dangles_and_scurries_home_when_let_go() {
    let mut cast = Cast::new(1);
    let porch = cast.porch(0);
    let away = porch + Vec3::new(3.0, 0.0, 2.0);
    cast.lift(0, away);
    assert_eq!(cast.played_at(), [Play::Lift]);
    assert_eq!(cast.playing(0), Some(Act::Dangle));
    assert_eq!(cast.act(0), Act::Dangle);
    let world = cast.app.world();
    assert!(world.resource::<Taken>().drag);
    assert_eq!(
        world.resource::<Hand>().lifted(),
        Some(cast.robots[0])
    );
    assert!(!cast.pickable(0));
    // It hangs over where the pointer is, clear of the ground.
    let land = cast.app.world().resource::<Land>().clone();
    let held = cast.at(0);
    assert!(held.xz().distance(away.xz()) < 0.1);
    let lift = cast.set().lift_height;
    assert!(held.y > Robot::ground(&land, held.xz()) + lift * 0.8);
    // It stays up for as long as it is held.
    cast.run(3.0);
    assert_eq!(cast.playing(0), Some(Act::Dangle));

    cast.release();
    let (acts, _) = cast.run(1.0);
    assert_eq!(cast.played_at(), [Play::Drop]);
    assert!(acts.contains(&Act::Land));
    assert!(cast.pickable(0));
    let world = cast.app.world();
    assert!(!world.resource::<Taken>().drag);
    assert_eq!(world.resource::<Hand>().lifted(), None);
    let down = cast.at(0);
    assert!((down.y - Robot::ground(&land, down.xz())).abs() < 0.3);

    // It walks home, faster than it ambles, and never jumps.
    let (acts, furthest) = cast.run(6.0);
    assert!(acts.contains(&Act::Walk), "{acts:?}");
    let scurry = cast.set().scurry;
    assert!(furthest < scurry / 60.0 * 1.6, "{furthest}");
    assert!(cast.at(0).distance(porch) < 0.05);
    assert_eq!(cast.act(0), Act::Idle);
    assert!(cast.wishes().is_empty());
}

#[test]
fn a_press_that_does_not_move_lifts_nothing() {
    let mut cast = Cast::new(1);
    let thing = cast.thing(0);
    cast.hover(Some(thing));
    cast.press();
    cast.run(0.5);
    assert!(cast.app.world().resource::<Taken>().drag);
    cast.release();
    cast.run(0.2);
    assert!(cast.played_at().is_empty());
    assert_eq!(cast.playing(0), None);
    assert!(!cast.app.world().resource::<Taken>().drag);

    // A press on the bare ground leaves the drag to the camera.
    cast.hover(None);
    cast.press();
    assert!(!cast.app.world().resource::<Taken>().drag);
}

#[test]
fn robots_look_at_a_pointer_that_passes_and_now_and_then_one_waves() {
    let mut cast = Cast::new(1);
    let here = cast.porch(0);
    cast.set().wave_chance = 0.0;
    let look = |cast: &Cast| {
        cast.app.world().get::<Mood>(cast.robots[0]).unwrap().look
    };
    cast.point(here + Vec3::new(40.0, 0.0, 0.0));
    cast.run(0.5);
    assert_eq!(look(&cast), Vec2::ZERO);

    cast.point(here + Vec3::new(1.5, 0.0, 0.2));
    cast.run(1.0);
    let seen = look(&cast);
    assert!(seen.x.abs() > 0.2 && seen.y > 0.0, "{seen}");
    // With no luck it does not wave.
    assert!(cast.played_at().is_empty());
    cast.point(here + Vec3::new(40.0, 0.0, 0.0));
    cast.run(1.5);
    assert_eq!(look(&cast), Vec2::ZERO);

    cast.set().wave_chance = 1.0;
    cast.point(here + Vec3::new(1.5, 0.0, 0.2));
    let (acts, _) = cast.run(0.5);
    assert_eq!(cast.played_at(), [Play::Wave]);
    assert!(acts.contains(&Act::Wave));
    cast.run(2.5);
    assert_eq!(cast.playing(0), None);
    assert_eq!(cast.act(0), Act::Idle);

    // Coming back at once, it has waved already.
    cast.point(here + Vec3::new(40.0, 0.0, 0.0));
    cast.run(0.5);
    cast.point(here + Vec3::new(1.5, 0.0, 0.2));
    cast.run(1.0);
    assert!(cast.played_at().is_empty());
}

#[test]
fn a_click_cheers_a_robot_whose_work_just_ended_and_its_neighbours_join()
 {
    let mut cast = Cast::new(2);
    let (agent, hit) = (cast.agents[0], cast.boxes[0]);
    cast.set().cheer_reach = 30.0;
    cast.app.world_mut().entity_mut(agent).insert(Busy);
    cast.run(0.2);
    assert!(cast.app.world().get::<Proud>(agent).is_none());
    cast.app.world_mut().entity_mut(agent).remove::<Busy>();
    cast.run(0.2);
    assert!(cast.app.world().get::<Proud>(agent).is_some());

    cast.click(hit);
    cast.run(0.1);
    assert_eq!(cast.played_at(), [Play::Cheer]);
    assert_eq!(cast.playing(0), Some(Act::Celebrate));
    assert!(cast.app.world().get::<Proud>(agent).is_none());
    // The neighbour joins a moment later.
    cast.run(1.5);
    assert_eq!(cast.playing(1), Some(Act::Celebrate));
    cast.run(4.0);
    assert_eq!((cast.playing(0), cast.playing(1)), (None, None));
    assert_eq!(cast.act(1), Act::Idle);

    // Cheered once: the next click is only a click.
    cast.click(hit);
    cast.run(1.0);
    assert!(cast.played_at().is_empty());

    // Left too long, the moment has passed.
    cast.app.world_mut().entity_mut(agent).insert(Busy);
    cast.run(0.1);
    cast.app.world_mut().entity_mut(agent).remove::<Busy>();
    let window = cast.set().cheer_window;
    cast.run(window + 1.0);
    assert!(cast.app.world().get::<Proud>(agent).is_none());
}

#[test]
fn a_file_dropped_on_a_robot_or_its_house_is_handed_to_its_agent() {
    let mut cast = Cast::new(1);
    let drop = |cast: &mut Cast, path: &str| {
        cast.app.world_mut().write_message(
            FileDragAndDrop::DroppedFile {
                window: Entity::PLACEHOLDER,
                path_buf: PathBuf::from(path),
            },
        );
    };
    // The pointer is seen on the house a moment after the drop.
    drop(&mut cast, "/desk/list.txt");
    cast.run(0.5);
    assert!(cast.wishes().is_empty());
    let house = Thing::ResidentHouse(cast.agents[0]);
    cast.hover(Some(house));
    let (acts, _) = cast.run(0.3);
    assert_eq!(
        cast.wishes(),
        [Wish::Letter {
            to: cast.agents[0],
            path: PathBuf::from("/desk/list.txt"),
        }]
    );
    assert_eq!(cast.played_at(), [Play::Letter]);
    assert!(acts.contains(&Act::Receive));
    cast.run(3.0);
    assert_eq!(cast.playing(0), None);
    assert!(cast.wishes().is_empty());

    // One that fell on nothing is nobody's.
    cast.hover(None);
    drop(&mut cast, "/desk/other.txt");
    let wait = cast.set().letter_wait;
    cast.run(wait + 0.5);
    cast.hover(Some(house));
    cast.run(0.5);
    assert!(cast.wishes().is_empty());
    assert!(cast.played_at().is_empty());
}

#[test]
fn four_files_dropped_together_send_one_manifest_with_four_roots() {
    use hamlet_client::outbox::Outbox;
    use hamlet_client::realm::{Citizen, Deeds, LivesIn};
    let dir = hamlet_testing::Scratch::new("hand-four-files");
    let mut cast = Cast::new(1);
    cast.app
        .add_plugins(ClientPlugin { server: None })
        .insert_resource(Deeds(vec!["give".into()]))
        .add_message::<crate::moving::SetDown>()
        .add_plugins(crate::orders::plugin);
    let hamlet = cast
        .app
        .world_mut()
        .spawn(Settlement {
            name: "home".into(),
        })
        .id();
    cast.app.world_mut().entity_mut(cast.agents[0]).insert((
        Citizen(0),
        Name::new("founder"),
        LivesIn(hamlet),
    ));
    cast.hover(Some(Thing::ResidentHouse(cast.agents[0])));
    for n in 0..4 {
        let path = dir.join(format!("book-{n}.txt"));
        std::fs::write(&path, "book").unwrap();
        cast.app.world_mut().write_message(
            FileDragAndDrop::DroppedFile {
                window: Entity::PLACEHOLDER,
                path_buf: path,
            },
        );
    }
    cast.run(0.1);
    let messages = cast
        .app
        .world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .collect::<Vec<_>>();
    let [
        v0::ClientMessage::Give(v0::Give::Files {
            files, to, ..
        }),
    ] = &messages[..]
    else {
        panic!("one manifest: {messages:?}");
    };
    assert_eq!(to.agent, "founder");
    assert_eq!(
        files
            .iter()
            .map(|file| (file.path.as_str(), file.size))
            .collect::<Vec<_>>(),
        [
            ("book-0.txt", 4),
            ("book-1.txt", 4),
            ("book-2.txt", 4),
            ("book-3.txt", 4)
        ]
    );
}

#[test]
fn a_click_on_a_thinking_agents_light_pins_its_thought() {
    let mut cast = Cast::new(1);
    let light = cast
        .app
        .world_mut()
        .spawn((
            Thought {
                of: AgentId(1),
                over: false,
            },
            GlobalTransform::default(),
        ))
        .id();
    // While the pointer is on it, a click is not one on bare ground.
    cast.over(Some(light));
    cast.run(0.1);
    assert!(cast.app.world().resource::<Taken>().click);
    // The pointer goes onto a tag over the light, and no event says
    // it left.
    cast.over(None);
    cast.run(0.1);
    assert!(!cast.app.world().resource::<Taken>().click);

    cast.over(Some(light));
    cast.click(light);
    cast.run(0.1);
    assert_eq!(cast.played_at(), [Play::Nudge]);
    let agent = cast.agents[0];
    assert!(cast.app.world().get::<Pinned>(agent).is_some());
    // The pin runs out by itself.
    let pin = cast.set().nudge_pin;
    cast.run(pin + 0.5);
    assert!(cast.app.world().get::<Pinned>(agent).is_none());

    // The thought ends with the pointer still on its light.
    cast.app.world_mut().entity_mut(light).despawn();
    cast.run(0.1);
    assert!(!cast.app.world().resource::<Taken>().click);
}

#[test]
fn a_robot_dropped_on_another_greets_it_and_asks_for_a_road() {
    let mut cast = Cast::new(2);
    cast.set().wave_chance = 0.0;
    let (a, b) = (cast.agents[0], cast.agents[1]);
    let other = cast.at(1);
    cast.lift(0, other);
    cast.played_at();
    // The pointer reaches the other robot through the one it holds.
    let thing = cast.thing(1);
    cast.hover(Some(thing));
    cast.release();
    let (acts, _) = cast.run(0.8);
    assert_eq!(cast.wishes(), [Wish::Road { a, b }]);
    assert_eq!(cast.played_at(), [Play::Introduce]);
    assert!(acts.contains(&Act::Greet));
    assert_eq!(cast.playing(1), Some(Act::Greet));
    // They stand a step apart, face to face.
    let gap = cast.set().greet_gap;
    let (here, there) = (cast.at(0), cast.at(1));
    assert!((here.xz().distance(there.xz()) - gap).abs() < 0.05);
    cast.run(1.0);
    let facing = |i: usize| {
        let robot = cast.robots[i];
        let at = cast.app.world().get::<Transform>(robot).unwrap();
        at.rotation * Vec3::Z
    };
    let to_other = (there - here).with_y(0.0).normalize();
    assert!(facing(0).dot(to_other) > 0.9);
    assert!(facing(1).dot(-to_other) > 0.9);

    // Then the one that came goes home.
    cast.run(8.0);
    assert!(cast.at(0).distance(cast.porch(0)) < 0.05);
    assert_eq!((cast.playing(0), cast.playing(1)), (None, None));

    // With a road already there they only greet.
    let ends = if a <= b { (a, b) } else { (b, a) };
    cast.app.world_mut().spawn(Road { ends });
    cast.played_at();
    let house = Thing::ResidentHouse(b);
    cast.lift(0, other);
    cast.hover(Some(house));
    cast.release();
    cast.run(0.8);
    assert!(cast.wishes().is_empty());
    assert_eq!(cast.played_at(), [Play::Lift, Play::Introduce]);

    // Set down on its own house, it has met nobody.
    cast.run(8.0);
    cast.played_at();
    let own = Thing::ResidentHouse(a);
    let porch = cast.porch(0);
    cast.lift(0, porch + Vec3::X);
    cast.hover(Some(own));
    cast.release();
    cast.run(0.8);
    assert_eq!(cast.played_at(), [Play::Lift, Play::Drop]);
}

#[test]
fn a_gift_lands_in_the_yard_and_two_robots_greet_where_they_stand() {
    let mut cast = Cast::new(2);
    let (a, b) = (cast.agents[0], cast.agents[1]);
    gifted(cast.app.world_mut(), a);
    let (acts, _) = cast.run(0.3);
    assert_eq!(cast.played_at(), [Play::Gift]);
    assert!(acts.contains(&Act::Wiggle));
    let gifts = |cast: &mut Cast| {
        let world = cast.app.world_mut();
        world
            .query::<(&Toy, &Transform)>()
            .iter(world)
            .filter(|(toy, _)| toy.kind == Kind::Gift)
            .map(|(_, at)| at.translation)
            .collect::<Vec<_>>()
    };
    cast.run(2.0);
    let set = PlaySettings::default();
    let robot =
        cast.app.world().get::<Robot>(cast.robots[0]).unwrap();
    let yard = robot.by_porch(set.gift_at);
    let seen = gifts(&mut cast);
    assert_eq!(seen.len(), 1);
    assert!(seen[0].distance(yard) < 0.02);
    // It is taken in after a while.
    cast.run(set.gift_stays);
    assert!(gifts(&mut cast).is_empty());

    greet(cast.app.world_mut(), a, b);
    cast.run(0.3);
    assert_eq!(cast.played_at(), [Play::Introduce]);
    assert_eq!(
        (cast.playing(0), cast.playing(1)),
        (Some(Act::Greet), Some(Act::Greet))
    );
    cast.run(3.0);
    assert_eq!((cast.act(0), cast.act(1)), (Act::Idle, Act::Idle));
}

#[test]
fn the_meter_follows_the_budget() {
    let mut cast = Cast::new(1);
    cast.app.add_plugins(ClientPlugin { server: None });
    let mut state = realm::state();
    state.hamlets[0].budget.left = Some(0.75);
    state.hamlets[0].budget.remains =
        Some("125 orchard credits".into());
    state.hamlets[0].budget.period_words = "this harvest".into();
    receive_state(&mut cast.app, state);
    let world = cast.app.world_mut();
    let hamlet = world
        .query_filtered::<Entity, With<Settlement>>()
        .single(world)
        .unwrap();
    world.entity_mut(hamlet).insert(Sited(TownId(0)));
    world.spawn((
        Common { town: TownId(0) },
        Transform::default(),
        Visibility::default(),
    ));
    let meter = cast.set().meter_at;
    let target = Vec3::new(meter.x, 4.0, meter.y);
    camera(&mut cast.app, target + Vec3::new(0.0, 6.0, 16.0), target);
    cast.run(0.2);
    let world = cast.app.world_mut();
    assert!(world.query::<&Text>().iter(world).any(
        |text| text.0 == "125 orchard credits LEFT THIS HARVEST"
    ));

    let heap = |cast: &mut Cast| {
        let world = cast.app.world_mut();
        world
            .query::<(&pool::Level, &Transform, &Visibility)>()
            .iter(world)
            .map(|(_, at, seen)| (at.scale.y, *seen))
            .collect::<Vec<_>>()
    };
    assert_eq!(heap(&mut cast), [(0.75, Visibility::Inherited)]);

    // Empty or overspent, the whole of the meter blinks.
    for left in [0.0, -0.25] {
        let mut changed = realm::hamlet("home");
        changed.budget.left = Some(left);
        receive_change(
            &mut cast.app,
            v0::Update::Hamlet { hamlet: changed },
        );
        let mut lit = HashSet::new();
        for _ in 0..12 {
            cast.run(0.1);
            let (tall, seen) = heap(&mut cast)[0];
            assert_eq!(tall, 1.0);
            lit.insert(seen == Visibility::Inherited);
        }
        assert_eq!(lit.len(), 2);
    }
    // The light's colour follows what is left: red at nothing,
    // green at all of it, and the two between in order.
    let stops =
        [Vec3::X, Vec3::new(1.0, 0.5, 0.0), Vec3::ONE, Vec3::Y];
    assert_eq!(shade(&stops, 0.0), stops[0]);
    assert_eq!(shade(&stops, 1.0), stops[3]);
    assert_eq!(shade(&stops, 2.0 / 3.0), stops[2]);
    let low = shade(&stops, 1.0 / 6.0);
    assert_eq!(low, stops[0].lerp(stops[1], 0.5));
}

#[test]
fn a_file_dropped_on_the_founder_begins_a_protocol_gift_without_a_house()
 {
    use hamlet_client::outbox::Outbox;
    use hamlet_testing::Scratch;

    let dir = Scratch::new("hand-founder-file");
    let path = dir.join("list.txt");
    std::fs::write(&path, "Dune").unwrap();
    let mut cast = Cast::new(1);
    cast.app
        .add_plugins(ClientPlugin { server: None })
        .add_message::<crate::moving::SetDown>()
        .add_plugins(crate::orders::plugin);
    let mut state = realm::state();
    state.deeds = Some(vec!["give".into()]);
    receive_state(&mut cast.app, state);
    let founder =
        hamlet_client::realm::mayor(cast.app.world_mut()).unwrap();
    cast.app
        .world_mut()
        .entity_mut(cast.agents[0])
        .remove::<Housed>();
    cast.app
        .world_mut()
        .entity_mut(founder)
        .insert(Housed(AgentId(1)));
    cast.app
        .world_mut()
        .entity_mut(cast.boxes[0])
        .insert(Thing::Keeper);
    cast.hover(Some(Thing::Keeper));
    cast.app.world_mut().write_message(
        FileDragAndDrop::DroppedFile {
            window: Entity::PLACEHOLDER,
            path_buf: path,
        },
    );
    cast.run(0.1);
    let messages = cast
        .app
        .world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .collect::<Vec<_>>();
    let [
        v0::ClientMessage::Give(v0::Give::Files {
            to, files, ..
        }),
    ] = &messages[..]
    else {
        panic!("one manifest: {messages:?}");
    };
    assert_eq!(to.agent, "founder");
    assert_eq!(
        files,
        &[v0::GiveFilesFiles {
            path: "list.txt".into(),
            size: 4
        }]
    );
}
