use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::window::RequestRedraw;

use super::{HouseMotion, Prop, Shift, shift_houses};
use crate::Clock;
use crate::generator::attire::Attires;
use crate::generator::house::{self, Remains};
use crate::generator::land::Land;
use crate::generator::loading::{Loading, finish_loading};
use crate::generator::society::{AgentId, Society};
use crate::generator::sync::{Drawn, sync};
use crate::generator::work::Passage;
use crate::generator::{
    DetailSettings, GeneratorOptions, GeneratorSettings, House,
    Phase, Tally, testing,
};
use crate::keeper::Act;
use crate::palette::Palette;
use crate::sky::SkySettings;

const FRAME: f32 = 1.0 / 60.0;

/// The generator with nothing drawn and no ground: a town with
/// five houses, one of them a ruin, all there from the start.
fn app(set: GeneratorSettings) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_message::<RequestRedraw>()
        .init_resource::<Clock>()
        .init_resource::<Drawn>()
        .init_resource::<Attires>()
        .init_resource::<Loading>()
        .init_resource::<Tally>()
        .init_resource::<GeneratorOptions>()
        .init_resource::<SkySettings>()
        .init_resource::<DetailSettings>()
        .init_resource::<HouseMotion>()
        .init_state::<Phase>()
        .insert_resource(Land::new(7, set, Palette::default()))
        .add_systems(
            Update,
            (
                finish_loading,
                sync.run_if(
                    resource_changed::<Society>
                        .or_else(resource_changed::<Land>),
                ),
                shift_houses,
            )
                .chain(),
        );
    testing::kit(app.world_mut());
    let mut land = app.world().resource::<Land>().clone();
    let mut society = Society::default();
    let town = society.found_town(&mut land);
    for _ in 0..5 {
        society.add_agent(&land, town).unwrap();
    }
    society.remove_agent(RUIN);
    app.insert_resource(society);
    settle(&mut app);
    app
}

/// The agent whose house is a ruin from the start.
const RUIN: AgentId = AgentId(3);

/// Runs frames, with the clock held, until every mesh asked for
/// is in the world.
fn settle(app: &mut App) {
    let began = Instant::now();
    loop {
        app.update();
        if app.world().resource::<Loading>().builds.is_empty() {
            return;
        }
        assert!(began.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn step(app: &mut App, secs: f32) {
    for _ in 0..(secs / FRAME).round() as usize {
        app.world_mut().resource_mut::<Clock>().t += FRAME;
        app.update();
    }
}

fn change(app: &mut App, with: impl FnOnce(&mut Society, &Land)) {
    let land = app.world().resource::<Land>().clone();
    with(&mut app.world_mut().resource_mut::<Society>(), &land);
    settle(app);
}

/// Every entity drawn as the house of `agent`.
fn houses(app: &mut App, agent: AgentId) -> Vec<Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &House)>()
        .iter(world)
        .filter(|(_, house)| house.agent == agent)
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>()
}

/// The entity the generator holds as the house of `agent`.
fn held(app: &App, agent: AgentId) -> Entity {
    app.world().resource::<Drawn>().houses[&agent].0
}

fn at(app: &App, entity: Entity) -> Transform {
    *app.world().get::<Transform>(entity).unwrap()
}

fn seen(app: &App, entity: Entity) -> Visibility {
    *app.world().get::<Visibility>(entity).unwrap()
}

fn shifting(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query::<&Shift>().iter(world).count()
}

fn props(app: &App, house: Entity) -> Vec<(Entity, Prop)> {
    app.world()
        .get::<Children>(house)
        .into_iter()
        .flatten()
        .filter_map(|child| {
            let prop = app.world().get::<Prop>(*child)?;
            Some((*child, *prop))
        })
        .collect::<Vec<_>>()
}

/// Asserts that `house` stands as it would had it never moved.
/// Seconds from the first move of a house going out to the last.
fn time_out(set: &HouseMotion) -> f32 {
    (set.shudder + set.fall).max(set.rest_after + set.rise)
}

fn rests(app: &App, house: Entity) {
    assert!(app.world().get::<Shift>(house).is_none());
    assert_eq!(at(app, house).scale, Vec3::ONE);
    assert_eq!(seen(app, house), Visibility::Inherited);
    for (child, prop) in props(app, house) {
        assert_eq!(at(app, child), prop.rest);
        assert_eq!(seen(app, child), Visibility::Inherited);
    }
}

fn add(app: &mut App) -> AgentId {
    let mut new = None;
    change(app, |society, land| {
        new = society.add_agent(land, society.towns[0].id);
    });
    new.unwrap()
}

#[test]
fn houses_there_from_the_start_do_not_move() {
    let mut app = app(GeneratorSettings::default());
    assert_eq!(shifting(&mut app), 0);
    let society = app.world().resource::<Society>().clone();
    for agent in &society.agents {
        let all = houses(&mut app, agent.id);
        assert_eq!(all, [held(&app, agent.id)]);
        rests(&app, all[0]);
    }
    let robots = app.world().resource::<Drawn>().robots.clone();
    assert_eq!(robots.len(), 5);
    for robot in robots.values() {
        assert_eq!(seen(&app, *robot), Visibility::Inherited);
        assert!(app.world().get::<Passage>(*robot).is_none());
    }
    let before = held(&app, AgentId(1));
    step(&mut app, 0.5);
    assert_eq!(held(&app, AgentId(1)), before);
    rests(&app, before);
}

#[test]
fn a_new_house_rises_and_its_yard_and_robot_follow() {
    let mut app = app(GeneratorSettings::default());
    let set = app.world().resource::<HouseMotion>().clone();
    let agent = add(&mut app);
    let house = held(&app, agent);
    let robot = app.world().resource::<Drawn>().robots[&agent];
    let yard = props(&app, house);
    assert!(yard.len() >= 2);

    // It starts flat on the ground, with nothing in its yard.
    assert_eq!(seen(&app, house), Visibility::Inherited);
    assert!(at(&app, house).scale.y < 0.05);
    assert!(at(&app, house).scale.x > 1.0);
    assert_eq!(seen(&app, robot), Visibility::Hidden);
    for (child, prop) in &yard {
        assert!(at(&app, *child).scale.y < prop.rest.scale.y * 0.01);
    }

    let mut heights = vec![];
    let frames = (set.rise / FRAME).round() as usize;
    for _ in 0..frames {
        step(&mut app, FRAME);
        heights.push(at(&app, house).scale.y);
    }
    assert!(heights[1] > heights[0]);
    let top = heights.iter().copied().fold(0.0, f32::max);
    assert!(top > 1.05 && top < 1.2, "{top}");
    assert!((heights[frames - 1] - 1.0).abs() < 0.02);
    // The yard is on its way and the robot is unfolding.
    assert!(app.world().get::<Shift>(house).is_some());
    assert!(at(&app, yard[0].0).scale.y > 0.5);
    assert_eq!(seen(&app, robot), Visibility::Inherited);
    assert!(app.world().get::<Passage>(robot).is_some());

    step(&mut app, set.time_in(yard.len()) - set.rise + FRAME);
    rests(&app, house);
    assert_eq!(houses(&mut app, agent), [house]);
    assert_eq!(shifting(&mut app), 0);
}

#[test]
fn a_removed_house_shudders_and_falls_to_its_ruin() {
    let mut set = GeneratorSettings::default();
    set.house.removed = Remains::Ruin;
    let mut app = app(set);
    let set = app.world().resource::<HouseMotion>().clone();
    let agent = AgentId(2);
    let old = held(&app, agent);
    let stood = at(&app, old);
    change(&mut app, |society, _| {
        society.remove_agent(agent);
    });
    let ruin = held(&app, agent);
    assert_ne!(ruin, old);
    assert_eq!(houses(&mut app, agent).len(), 2);
    assert_eq!(seen(&app, ruin), Visibility::Hidden);

    step(&mut app, set.shudder * 0.6);
    let shaken = at(&app, old);
    assert!(shaken.rotation.angle_between(stood.rotation) > 1e-3);
    assert_eq!(shaken.scale.y, 1.0);
    assert_eq!(seen(&app, ruin), Visibility::Hidden);

    step(&mut app, set.shudder * 0.4 + set.fall * 0.6);
    let falling = at(&app, old).scale.y;
    assert!(falling < 0.9 && falling > 0.05, "{falling}");
    // Its remains are coming up under it.
    assert_eq!(seen(&app, ruin), Visibility::Inherited);
    assert!(at(&app, ruin).scale.y < 1.0);
    assert!(app.world().get::<Shift>(ruin).is_some());

    step(&mut app, set.fall * 0.4 + FRAME);
    assert!(app.world().get_entity(old).is_err());
    step(&mut app, time_out(&set) - set.shudder - set.fall);
    rests(&app, ruin);
    assert_eq!(houses(&mut app, agent), [ruin]);
    assert!(app.world().get::<House>(ruin).unwrap().archived);
    assert!(
        app.world().resource::<Drawn>().houses[&agent].2.tris > 0
    );
    assert_eq!(shifting(&mut app), 0);
}

#[test]
fn a_house_stands_until_its_robot_has_taken_its_leave() {
    let mut app = app(GeneratorSettings::default());
    let agent = AgentId(2);
    let old = held(&app, agent);
    let stood = at(&app, old);
    let robot = app.world().resource::<Drawn>().robots[&agent];
    app.world_mut()
        .entity_mut(robot)
        .insert(Passage::new(Act::Leave));
    change(&mut app, |society, _| {
        society.remove_agent(agent);
    });
    step(&mut app, 0.5);
    assert_eq!(at(&app, old), stood);
    assert_eq!(seen(&app, held(&app, agent)), Visibility::Hidden);

    app.world_mut().entity_mut(robot).despawn();
    step(&mut app, 0.2);
    assert_ne!(at(&app, old), stood);
}

#[test]
fn a_restored_house_rises_from_its_ruin() {
    let mut app = app(GeneratorSettings::default());
    let set = app.world().resource::<HouseMotion>().clone();
    let ruin = held(&app, RUIN);
    change(&mut app, |society, land| {
        society.restore(land, RUIN);
    });
    let house = held(&app, RUIN);
    assert_ne!(house, ruin);
    assert!(at(&app, house).scale.y < 0.05);

    step(&mut app, set.clear * 0.5);
    let sinking = at(&app, ruin).scale.y;
    assert!(sinking < 1.0 && sinking > 0.05, "{sinking}");
    assert!(at(&app, house).scale.y > 0.05);
    step(&mut app, set.clear * 0.5 + FRAME);
    assert!(app.world().get_entity(ruin).is_err());

    let yard = props(&app, house).len();
    step(&mut app, set.time_in(yard));
    rests(&app, house);
    assert_eq!(houses(&mut app, RUIN), [house]);
    assert!(!app.world().get::<House>(house).unwrap().archived);
}

#[test]
fn the_garden_tree_stands_through_a_house_going_and_coming() {
    let mut app = app(GeneratorSettings::default());
    let set = app.world().resource::<HouseMotion>().clone();
    let land = app.world().resource::<Land>().clone();
    let society = app.world().resource::<Society>().clone();
    let agent = society
        .agents
        .iter()
        .filter(|a| !a.archived && a.plot != 0)
        .find(|a| {
            let yard =
                house::yard_of(&land, a.id, false, 0.0, a.wings);
            house::garden_tree(&land, a.id, &yard).is_some()
        })
        .expect("a house with a garden tree")
        .id;
    // The tree's size in the world, of the house in sight.
    let tree = |app: &App| {
        let house = held(app, agent);
        let (child, prop) = props(app, house)
            .into_iter()
            .find(|(_, prop)| prop.stays)
            .unwrap();
        assert_ne!(seen(app, child), Visibility::Hidden);
        let size = at(app, house).scale * at(app, child).scale;
        assert!(size.distance(prop.rest.scale) < 1e-4, "{size}");
    };
    change(&mut app, |society, _| {
        society.remove_agent(agent);
    });
    for _ in 0..((time_out(&set) + 0.1) / FRAME) as usize {
        step(&mut app, FRAME);
        tree(&app);
    }
    change(&mut app, |society, land| {
        society.restore(land, agent);
    });
    for _ in 0..70 {
        step(&mut app, FRAME);
        tree(&app);
    }
    rests(&app, held(&app, agent));
}

#[test]
fn a_town_set_down_elsewhere_goes_house_by_house_with_its_roads() {
    use std::collections::HashMap;

    use crate::generator::society::{Ground, TownId};

    let mut app = app(GeneratorSettings::default());
    // Three more towns, one of them with houses.
    let (mut land, mut society) = (
        app.world().resource::<Land>().clone(),
        app.world().resource::<Society>().clone(),
    );
    let moved = society.found_town(&mut land);
    society.found_town(&mut land);
    society.found_town(&mut land);
    let folk = (0..6)
        .filter_map(|_| society.add_agent(&land, moved))
        .collect::<Vec<_>>();
    app.insert_resource(land.clone());
    app.insert_resource(society.clone());
    settle(&mut app);
    step(&mut app, 4.0);

    let roads = |app: &App| {
        app.world()
            .resource::<Drawn>()
            .roads
            .iter()
            .map(|(pair, road)| (*pair, road.entity))
            .collect::<HashMap<_, _>>()
    };
    let before = roads(&app);
    let stood =
        folk.iter().map(|a| held(&app, *a)).collect::<Vec<_>>();
    let was = land.sites[society.town(moved).unwrap().site].at;
    let to = (0..900)
        .map(|i| {
            Vec2::from_angle(i as f32) * (700.0 + i as f32 * 3.0)
        })
        .find(|&p| {
            society.fits(&land, Ground::Town(moved), p).is_ok()
        })
        .unwrap();
    society.move_town(&mut land, moved, to).unwrap();
    app.insert_resource(land);
    settle(&mut app);

    // The houses go and come one after another, not all at
    // once.
    let waits = folk
        .iter()
        .map(|a| held(&app, *a))
        .map(|house| app.world().get::<Shift>(house).unwrap().wait)
        .collect::<Vec<_>>();
    assert!(waits.windows(2).all(|w| w[0] < w[1]), "{waits:?}");
    step(&mut app, 6.0);
    settle(&mut app);
    assert_eq!(shifting(&mut app), 0);
    for (agent, old) in folk.iter().zip(&stood) {
        // The house at the old place is gone, and one house stands at
        // the new.
        assert!(app.world().get_entity(*old).is_err());
        assert_eq!(houses(&mut app, *agent).len(), 1);
        let house = held(&app, *agent);
        rests(&app, house);
        let stands = at(&app, house).translation.xz();
        assert!(stands.distance(to) < 30.0);
        assert!(stands.distance(was) > 600.0);
    }

    // Its roads were laid anew and end where it stands. The
    // roads inside the other towns are the ones that were
    // there.
    let after = roads(&app);
    assert_eq!(before.len(), after.len());
    let society = app.world().resource::<Society>().clone();
    let town_of = |agent: AgentId| society.agent(agent).unwrap().town;
    let mut kept = 0;
    for (pair, entity) in &after {
        let towns = (town_of(pair.0), town_of(pair.1));
        if towns.0 == moved || towns.1 == moved {
            assert_ne!(before[pair], *entity);
            let drawn = app.world().resource::<Drawn>();
            let pts = &drawn.roads[pair].route.line.pts;
            let ends = [pts[0], *pts.last().unwrap()];
            assert!(ends.iter().any(|p| p.xz().distance(to) < 30.0));
        } else if towns.0 == towns.1 {
            assert_eq!(before[pair], *entity);
            kept += 1;
        }
    }
    assert!(kept >= 4);
    assert_eq!(society.towns.len(), 4);
    assert!(society.town(TownId(0)).is_some());
}
