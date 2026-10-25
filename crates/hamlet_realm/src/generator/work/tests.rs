use bevy::prelude::*;

use super::lot::footing;
use super::{Passage, Robot};
use crate::Clock;
use crate::generator::land::Land;
use crate::generator::layout::plot;
use crate::generator::society::AgentId;
use crate::generator::testing;
use crate::keeper::{Act, Keeper};

/// A world of one robot at its porch, with nothing drawn.
fn app() -> (App, Entity) {
    let mut app = App::new();
    testing::rehearse(&mut app);
    let id = testing::stand_in(app.world_mut(), AgentId(3), 3);
    (app, id)
}

/// Runs `secs` of frames. Returns every motion the robot was seen
/// in.
fn run(app: &mut App, id: Entity, secs: f32) -> Vec<Act> {
    let mut acts = Vec::new();
    for _ in 0..(secs * 60.0) as usize {
        app.world_mut().resource_mut::<Clock>().t += 1.0 / 60.0;
        app.update();
        let act = app.world().get::<Keeper>(id).unwrap().act();
        if !acts.contains(&act) {
            acts.push(act);
        }
    }
    acts
}

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
    let mut acts = run(&mut app, id, 4.0);
    assert!(acts.contains(&Act::Wave));
    let robot = app.world().get::<Robot>(id).unwrap();
    assert_eq!(robot.job.as_ref().unwrap().leg.speed, 4.0);
    for _ in 0..200 {
        acts.extend(run(&mut app, id, 0.1));
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
