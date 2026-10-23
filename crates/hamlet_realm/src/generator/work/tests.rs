use bevy::prelude::*;

use super::lot::footing;
use super::{Objecting, Passage, Robot};
use crate::Clock;
use crate::generator::land::Land;
use crate::generator::layout::plot;
use crate::generator::society::AgentId;
use crate::generator::testing;
use crate::keeper::work::{Paper, Station, Task, Work};
use crate::keeper::{Act, Keeper, KeeperState};

/// A world of one robot at its porch, with nothing drawn.
fn app() -> (App, Entity) {
    let mut app = App::new();
    testing::rehearse(&mut app);
    let id = testing::stand_in(app.world_mut(), AgentId(3), 3);
    (app, id)
}

/// Runs `secs` of frames. Returns the furthest the robot moved
/// in one of them, and every motion it was seen in.
fn run(app: &mut App, id: Entity, secs: f32) -> (f32, Vec<Act>) {
    let mut furthest = 0.0f32;
    let mut acts = Vec::new();
    let at = |app: &App| {
        app.world().get::<Transform>(id).unwrap().translation
    };
    for _ in 0..(secs * 60.0) as usize {
        let before = at(app);
        app.world_mut().resource_mut::<Clock>().t += 1.0 / 60.0;
        app.update();
        furthest = furthest.max(before.distance(at(app)));
        let act = app.world().get::<Keeper>(id).unwrap().act();
        if !acts.contains(&act) {
            acts.push(act);
        }
    }
    (furthest, acts)
}

fn set(app: &mut App, id: Entity, state: KeeperState, work: Work) {
    let mut robot = app.world_mut().get_mut::<Robot>(id).unwrap();
    robot.state = state;
    robot.work = work;
}

fn step(task: Task, step: u32, failures: u32) -> Work {
    Work {
        task,
        step,
        failures,
    }
}

/// A step at an amble covers 0.017 a frame.
const STEP: f32 = 0.03;

#[test]
fn a_robot_set_down_across_the_town_goes_home_round_the_houses() {
    let (mut app, id) = app();
    run(&mut app, id, 0.2);
    let land = app.world().resource::<Land>();
    let town = land.set.town.clone();
    let others = (0..town.plots)
        .filter(|&k| k != 3)
        .map(|k| plot(land, 0, k))
        .filter(|p| p.usable)
        .collect::<Vec<_>>();
    // Behind the house furthest from its own.
    let home = plot(land, 0, 3).at;
    let far = others
        .iter()
        .max_by(|a, b| {
            a.at.distance(home).total_cmp(&b.at.distance(home))
        })
        .unwrap();
    let from = far.at - far.facing() * (town.house_radius + 1.2);
    let from = Vec3::new(from.x, footing(land, from), from.y);
    let porch = app.world().get::<Robot>(id).unwrap().lot.porch;
    let mut robot = app.world_mut().entity_mut(id);
    robot.get_mut::<Transform>().unwrap().translation = from;
    robot.get_mut::<Robot>().unwrap().put_down(Some(4.0));
    // A wave on the way does not slow the rest of it.
    run(&mut app, id, 0.5);
    app.world_mut()
        .entity_mut(id)
        .insert(Passage::new(Act::Wave));
    let mut acts = run(&mut app, id, 4.0).1;
    assert!(acts.contains(&Act::Wave));
    let robot = app.world().get::<Robot>(id).unwrap();
    assert_eq!(robot.job.as_ref().unwrap().leg.speed, 4.0);
    for _ in 0..200 {
        acts.extend(run(&mut app, id, 0.1).1);
        let at =
            app.world().get::<Transform>(id).unwrap().translation;
        for other in &others {
            assert!(
                at.xz().distance(other.at) > town.house_radius,
                "through a house at {at}"
            );
        }
    }
    assert!(!acts.contains(&Act::Appear));
    let at = app.world().get::<Transform>(id).unwrap().translation;
    assert!(at.distance(porch) < 0.05, "{at}");
}

#[test]
fn a_robot_walks_from_chore_to_chore_and_never_jumps() {
    let (mut app, id) = app();
    run(&mut app, id, 0.5);
    let at = |app: &App| {
        app.world().get::<Transform>(id).unwrap().translation
    };
    let stand = |app: &App, station: Station| {
        let robot = app.world().get::<Robot>(id).unwrap();
        let land = app.world().resource::<Land>();
        robot.lot.stand(land, station, 0.0).unwrap().0
    };
    let porch = at(&app);
    assert!(porch.distance(stand(&app, Station::Porch)) < 1e-3);

    set(&mut app, id, KeeperState::Working, step(Task::Run, 1, 0));
    let (moved, acts) = run(&mut app, id, 9.0);
    assert!(moved < STEP);
    assert!(at(&app).distance(stand(&app, Station::Engine)) < 0.02);
    assert_eq!(acts, [Act::Amble, Act::Crank]);
    // The engine turns only while it is worked.
    assert!(app.world().get::<Robot>(id).unwrap().steaming);

    set(&mut app, id, KeeperState::Working, step(Task::Think, 2, 0));
    let (moved, acts) = run(&mut app, id, 12.0);
    assert!(moved < STEP);
    assert!(acts.contains(&Act::Pace) && acts.contains(&Act::Think));
    assert!(!app.world().get::<Robot>(id).unwrap().steaming);

    // A tool that is over in a moment is still seen in use.
    let write = Task::Write(Paper::File);
    set(&mut app, id, KeeperState::Working, step(write, 3, 0));
    run(&mut app, id, 0.1);
    set(&mut app, id, KeeperState::Working, step(Task::Think, 4, 0));
    let (moved, acts) = run(&mut app, id, 9.0);
    assert!(moved < STEP);
    assert!(acts.contains(&Act::Write));
    assert!(
        acts.ends_with(&[Act::Pace]) || acts.ends_with(&[Act::Think])
    );

    // A failure startles it at once, where it stands.
    set(&mut app, id, KeeperState::Working, step(Task::Think, 5, 1));
    let here = at(&app);
    let (_, acts) = run(&mut app, id, 0.2);
    assert!(acts.contains(&Act::Oops));
    assert!(at(&app).distance(here) < 0.05);
    run(&mut app, id, 4.0);

    set(&mut app, id, KeeperState::Rest, step(Task::Think, 5, 1));
    let (moved, acts) = run(&mut app, id, 14.0);
    assert!(moved < STEP);
    assert!(
        acts.contains(&Act::Done) || acts.contains(&Act::Celebrate)
    );
    assert!(acts.contains(&Act::Walk));
    assert_eq!(
        app.world().get::<Keeper>(id).unwrap().act(),
        Act::Idle
    );
    assert!(at(&app).distance(porch) < 0.02);
}

#[test]
fn with_only_busy_known_it_keeps_a_round_of_its_own() {
    let (mut app, id) = app();
    run(&mut app, id, 0.2);
    set(&mut app, id, KeeperState::Working, Work::default());
    let (moved, acts) = run(&mut app, id, 60.0);
    assert!(moved < STEP);
    assert!(acts.contains(&Act::Pace));
    assert!(acts.len() >= 5, "{acts:?}");
}

#[test]
fn a_waiting_robot_holds_its_request_up_or_argues() {
    let (mut app, id) = app();
    app.init_resource::<Objecting>();
    run(&mut app, id, 0.2);
    set(&mut app, id, KeeperState::Waiting, Work::default());
    let (_, acts) = run(&mut app, id, 6.0);
    assert_eq!(acts.last(), Some(&Act::Wait));

    let agent = app.world().get::<Robot>(id).unwrap().agent;
    app.world_mut().resource_mut::<Objecting>().0.insert(agent);
    let (_, acts) = run(&mut app, id, 1.0);
    assert_eq!(acts.last(), Some(&Act::Object));
    // The chief replied to the objection; something else waits.
    app.world_mut().resource_mut::<Objecting>().0.clear();
    let (_, acts) = run(&mut app, id, 1.0);
    assert_eq!(acts.last(), Some(&Act::Wait));
    set(&mut app, id, KeeperState::Rest, Work::default());
    let (_, acts) = run(&mut app, id, 6.0);
    assert_eq!(acts.last(), Some(&Act::Idle));
}

#[test]
fn a_still_is_the_same_whatever_came_before() {
    let place = |warm: f32| {
        let (mut app, id) = app();
        set(&mut app, id, KeeperState::Working, Work::default());
        run(&mut app, id, warm);
        let mut clock = app.world_mut().resource_mut::<Clock>();
        clock.frozen = true;
        clock.t = 41.3;
        app.update();
        let at = *app.world().get::<Transform>(id).unwrap();
        (at.translation, at.rotation)
    };
    assert_eq!(place(0.1), place(7.0));
}

fn tick(app: &mut App, secs: f32) {
    for _ in 0..(secs * 60.0) as usize {
        app.world_mut().resource_mut::<Clock>().t += 1.0 / 60.0;
        app.update();
    }
}

#[test]
fn a_robot_on_its_way_out_bows_and_is_gone() {
    let (mut app, id) = app();
    tick(&mut app, 0.5);
    app.world_mut()
        .entity_mut(id)
        .insert(Passage::new(Act::Leave));
    tick(&mut app, 2.0);
    // Its work leaves it to its bow.
    let keeper = app.world().get::<Keeper>(id).unwrap();
    assert_eq!(keeper.act(), Act::Leave);
    tick(&mut app, 2.0);
    assert!(app.world().get_entity(id).is_err());
}
