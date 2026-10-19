use bevy::prelude::*;
use hamlet_core::agents::{
    Activity, Busy, Carried, Citizen, Doing, Left, Lived, LivesIn,
    Mayor, MergedInto, Road, Settlement, Spot, Traffic, Voted,
};
use hamlet_rules::hamlet::{HOME, RoadUse};

use super::housing::{
    close, come_back, house, leave, make_room, move_out, resite,
};
use super::roads::{lay, unlay, walk};
use super::state::{show, vote};
use super::view::{aim, tether};
use super::{Housed, Laid, Returning, Sited, Unhoused, town_of};
use crate::generator::land::Land;
use crate::generator::society::{
    Agent, Pair, Place, RoadLevel, Society,
};
use crate::generator::{
    Ahead, GenSettings, GenView, Objecting, Traffic as Trips, Voting,
};
use crate::keeper::KState;
use crate::keeper::work::Task;
use crate::palette::Palette;
use crate::pick::{Centre, Glide, Leash, Orbit, Stance};
use crate::sky::CameraSettings;
use crate::{Clock, Stage};

/// The world's side of the hamlets, with no hamlet in it.
fn bare() -> App {
    let mut app = App::new();
    app.init_resource::<Stage>()
        .init_resource::<Society>()
        .init_resource::<Trips>()
        .init_resource::<Objecting>()
        .init_resource::<Voting>()
        .init_resource::<Clock>()
        .insert_resource(Land::new(
            1,
            GenSettings::default(),
            Palette::default(),
        ))
        .add_observer(leave)
        .add_observer(come_back)
        .add_observer(move_out)
        .add_observer(close)
        .add_observer(unlay)
        .add_observer(walk)
        .add_observer(vote)
        .init_resource::<CameraSettings>()
        .init_resource::<Glide>()
        .init_resource::<Orbit>()
        .init_resource::<Leash>()
        .init_resource::<Centre>()
        .init_resource::<Stance>()
        .init_resource::<GenView>()
        .init_resource::<Ahead>()
        .add_systems(
            Update,
            (make_room, house, resite, lay, show, tether, aim)
                .chain(),
        );
    app
}

fn app() -> (App, Entity) {
    let mut app = bare();
    let hamlet = settle(&mut app, HOME);
    (app, hamlet)
}

#[test]
fn a_hamlet_that_is_removed_takes_its_town_with_it() {
    let (mut app, _) = app();
    let mill = settle(&mut app, "mill");
    let miller = app.world_mut().spawn((
        Citizen(1),
        LivesIn(mill),
        Lived::default(),
    ));
    let miller = miller.id();
    app.update();
    assert_eq!(app.world().resource::<Society>().towns.len(), 2);
    let world = app.world_mut();
    let of_mill = world
        .query::<(Entity, &LivesIn)>()
        .iter(world)
        .filter(|(_, lives)| lives.0 == mill)
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    assert!(of_mill.contains(&miller));
    for agent in of_mill {
        world.entity_mut(agent).despawn();
    }
    world.entity_mut(mill).despawn();
    app.update();
    let society = app.world().resource::<Society>();
    assert_eq!(society.towns.len(), 1);
    let town = society.towns[0].id;
    assert!(society.agents.iter().all(|a| a.town == town));
    assert!(society.roads.is_empty());
}

/// A hamlet with its first mayor in office.
fn settle(app: &mut App, name: &str) -> Entity {
    let hamlet = app
        .world_mut()
        .spawn(Settlement {
            home: std::path::PathBuf::new(),
            name: name.to_string(),
        })
        .id();
    let mayor = (Citizen(0), LivesIn(hamlet), Mayor);
    app.world_mut().spawn(mayor);
    hamlet
}

fn entity(app: &mut App, number: u64) -> Option<Entity> {
    let world = app.world_mut();
    let first = world
        .query::<(Entity, &Settlement)>()
        .iter(world)
        .find(|(_, s)| s.name == HOME)
        .map(|(entity, _)| entity)?;
    world
        .query::<(Entity, &Citizen, &LivesIn)>()
        .iter(world)
        .find(|(_, citizen, lives)| {
            citizen.0 == number && lives.0 == first
        })
        .map(|(entity, ..)| entity)
}

/// Sets who lives in the hamlet and who left, each by its
/// number, as the core's sync does, and runs a frame.
fn tell(
    app: &mut App,
    hamlet: Entity,
    living: &[u64],
    gone: &[(u64, Option<u64>)],
) {
    for number in living.iter().chain(gone.iter().map(|(n, _)| n)) {
        if entity(app, *number).is_none() {
            app.world_mut()
                .spawn((Citizen(*number), LivesIn(hamlet)));
        }
    }
    for number in living {
        let agent = entity(app, *number).unwrap();
        app.world_mut()
            .entity_mut(agent)
            .remove::<(Left, MergedInto)>();
    }
    for (number, into) in gone {
        let agent = entity(app, *number).unwrap();
        if app.world().get::<Left>(agent).is_some() {
            continue;
        }
        let host = into.map(|into| entity(app, into).unwrap());
        let mut agent = app.world_mut().entity_mut(agent);
        match host {
            Some(host) => {
                agent.insert((Left, MergedInto(host)));
            }
            None => {
                agent.insert(Left);
            }
        }
    }
    app.update();
}

fn agent(app: &mut App, number: u64) -> Agent {
    let agent = entity(app, number).unwrap();
    let housed = app.world().get::<Housed>(agent).unwrap().0;
    let society = app.world().resource::<Society>();
    society.agent(housed).unwrap().clone()
}

#[test]
fn the_world_shows_who_left_and_who_came_back() {
    let (mut app, hamlet) = app();
    tell(&mut app, hamlet, &[1, 2, 3], &[]);
    let plots = [1, 2, 3].map(|id| agent(&mut app, id).plot);

    // Removed: a ruin.
    tell(&mut app, hamlet, &[1, 3], &[(2, None)]);
    assert!(agent(&mut app, 2).archived);

    // Merged: a ruin, and a wing on the house that took it in.
    tell(&mut app, hamlet, &[1], &[(2, None), (3, Some(1))]);
    assert!(agent(&mut app, 3).archived);
    assert_eq!(agent(&mut app, 1).wings, 1);

    // Split and restored: both stand again where they stood.
    tell(&mut app, hamlet, &[1, 3, 2], &[]);
    assert_eq!(agent(&mut app, 1).wings, 0);
    for (id, plot) in [1, 2, 3].into_iter().zip(plots) {
        let now = agent(&mut app, id);
        assert!(!now.archived);
        assert_eq!(now.plot, plot);
    }
    let world = app.world_mut();
    let returning = world
        .query_filtered::<(), With<Returning>>()
        .iter(world)
        .count();
    assert_eq!(returning, 2);
}

#[test]
fn a_world_opened_later_puts_everyone_on_the_same_plot() {
    let (mut first, hamlet) = app();
    tell(&mut first, hamlet, &[1, 2, 3], &[]);
    let (mut later, hamlet) = app();
    tell(&mut later, hamlet, &[1], &[(2, None), (3, Some(1))]);
    for id in [1, 2, 3] {
        assert_eq!(
            agent(&mut later, id).plot,
            agent(&mut first, id).plot
        );
    }
    assert!(
        agent(&mut later, 2).archived
            && agent(&mut later, 3).archived
    );
    assert_eq!(agent(&mut later, 1).wings, 1);
}

#[test]
fn a_newcomer_takes_the_plot_of_one_who_left_and_keeps_it() {
    // Two is removed before three comes, so three builds where
    // two stood, in a world that sees it happen and in one that
    // is opened afterward.
    let lived =
        |since: u64, until: Option<u64>| Lived { since, until };
    let (mut seen, hamlet) = app();
    tell(&mut seen, hamlet, &[1, 2], &[]);
    let stood = agent(&mut seen, 2).plot;
    tell(&mut seen, hamlet, &[1], &[(2, None)]);
    tell(&mut seen, hamlet, &[1, 3], &[(2, None)]);
    assert_eq!(agent(&mut seen, 3).plot, stood);
    assert!(agent(&mut seen, 2).cleared);

    let (mut later, hamlet) = app();
    for (number, times) in [
        (1, lived(10, None)),
        (2, lived(20, Some(30))),
        (3, lived(40, None)),
    ] {
        later.world_mut().spawn((
            Citizen(number),
            LivesIn(hamlet),
            times,
        ));
    }
    tell(&mut later, hamlet, &[1, 3], &[(2, None)]);
    assert_eq!(agent(&mut later, 3).plot, stood);
    assert!(agent(&mut later, 2).cleared);
    assert_eq!(agent(&mut later, 1).plot, agent(&mut seen, 1).plot);
}

#[test]
fn an_agent_with_no_plot_waits_for_one_and_the_society_rests() {
    let (mut app, hamlet) = app();
    let plots = app.world().resource::<Land>().set.town.plots;
    let all = (1..=plots as u64 + 1).collect::<Vec<_>>();
    tell(&mut app, hamlet, &all, &[]);
    let waiting = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<(), With<Unhoused>>()
            .iter(world)
            .count()
    };
    let left_out = waiting(&mut app);
    assert!(left_out > 0);
    app.update();
    assert!(!app.world().is_resource_changed::<Society>());

    // One leaves, and one of those waiting takes its plot.
    tell(&mut app, hamlet, &all[1..], &[(1, None)]);
    assert_eq!(waiting(&mut app), left_out - 1);
}

#[test]
fn roads_and_work_follow_the_entities() {
    let (mut app, hamlet) = app();
    tell(&mut app, hamlet, &[1, 2], &[]);
    let (a, b) =
        (entity(&mut app, 1).unwrap(), entity(&mut app, 2).unwrap());
    let traffic = |messages, level| Traffic {
        level,
        messages,
        last_from: Some(a),
    };
    let road = app
        .world_mut()
        .spawn((Road { ends: (a, b) }, traffic(0, RoadUse::Dirt)))
        .id();
    app.world_mut().entity_mut(a).insert((
        Busy,
        Activity {
            doing: Doing::Fetch,
            step: 2,
            failures: 0,
        },
    ));
    app.update();
    let pair = app.world().get::<Laid>(road).unwrap().0;
    let level = |app: &App| {
        app.world()
            .resource::<Society>()
            .road(pair)
            .map(|r| r.level)
    };
    assert_eq!(level(&app), Some(RoadLevel::Dirt));
    let one = agent(&mut app, 1);
    assert_eq!(one.state, KState::Working);
    assert_eq!(one.work.task, Task::Fetch);

    // A message: the road is used more and its sender walks.
    app.world_mut()
        .entity_mut(road)
        .insert(traffic(3, RoadUse::Paved));
    app.world_mut().trigger(Carried { road, from: a });
    app.update();
    assert_eq!(level(&app), Some(RoadLevel::Paved));
    let trips = &app.world().resource::<Trips>().trips;
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0].walker, one.id);

    app.world_mut().entity_mut(road).despawn();
    assert_eq!(level(&app), None);
}

#[test]
fn several_hamlets_are_towns_with_roads_between_and_ballots_are_seen()
{
    let (mut app, hamlet) = app();
    let mill = settle(&mut app, "mill");
    tell(&mut app, hamlet, &[1, 2], &[]);
    let miller = {
        let world = app.world_mut();
        world
            .query::<(Entity, &LivesIn)>()
            .iter(world)
            .find(|(_, lives)| lives.0 == mill)
            .map(|(entity, _)| entity)
            .unwrap()
    };
    let housed = |app: &App, agent: Entity| {
        let id = app.world().get::<Housed>(agent).unwrap().0;
        app.world().resource::<Society>().agent(id).unwrap().clone()
    };
    let mayor = entity(&mut app, 0).unwrap();
    let (here, there) = (housed(&app, mayor), housed(&app, miller));
    assert_ne!(here.town, there.town);
    // Each first mayor has its town's hall.
    let society = app.world().resource::<Society>();
    assert_eq!(society.town(there.town).unwrap().leader, there.id);

    // The office moves and nobody changes house.
    let one = entity(&mut app, 1).unwrap();
    let plot = housed(&app, one).plot;
    app.world_mut().entity_mut(mayor).remove::<Mayor>();
    app.world_mut().entity_mut(one).insert(Mayor);
    app.update();
    assert_eq!(housed(&app, one).plot, plot);
    assert_eq!(housed(&app, mayor).id, here.id);

    // A road between the mayors crosses the towns.
    let ends = if mayor <= miller {
        (mayor, miller)
    } else {
        (miller, mayor)
    };
    let traffic = Traffic {
        level: RoadUse::Dirt,
        messages: 0,
        last_from: None,
    };
    app.world_mut().spawn((Road { ends }, traffic));
    app.update();
    let pair = Pair::new(here.id, there.id);
    assert!(app.world().resource::<Society>().road(pair).is_some());

    // A ballot is seen where it was cast.
    app.world_mut().trigger(Voted { agent: one });
    let voter = housed(&app, one).id;
    assert!(app.world().resource::<Voting>().0.contains_key(&voter));

    // An agent that is gone for good leaves a ruin.
    let two = entity(&mut app, 2).unwrap();
    let was = app.world().get::<Housed>(two).unwrap().0;
    app.world_mut().entity_mut(two).despawn();
    let society = app.world().resource::<Society>();
    assert!(society.agent(was).unwrap().archived);
}

/// The place of each hamlet's town, by the hamlet's name.
fn towns(app: &mut App) -> Vec<(String, Vec2)> {
    let world = app.world_mut();
    let mut found = world
        .query::<(&Settlement, &Sited)>()
        .iter(world)
        .map(|(settlement, sited)| (settlement.name.clone(), sited.0))
        .collect::<Vec<_>>();
    found.sort();
    let (land, society) =
        (world.resource::<Land>(), world.resource::<Society>());
    found
        .into_iter()
        .map(|(name, town)| {
            let site = society.town(town).unwrap().site;
            (name, land.sites[site].at)
        })
        .collect()
}

/// Open ground far from every town that the town of `hamlet`
/// fits on.
fn meadow(app: &mut App, hamlet: Entity) -> Vec2 {
    let town = town_of(app.world(), hamlet).unwrap();
    let land = app.world().resource::<Land>();
    let society = app.world().resource::<Society>();
    (0..900)
        .map(|i| {
            Vec2::from_angle(i as f32) * (260.0 + i as f32 * 3.0)
        })
        .find(|&p| society.fits(land, Place::Town(town), p).is_ok())
        .unwrap()
}

#[test]
fn hamlets_with_no_place_recorded_stand_where_they_always_did() {
    let (mut app, _) = app();
    // One as an old home has it, one as a new one has it.
    settle(&mut app, "mill");
    let forge = settle(&mut app, "forge");
    app.world_mut().entity_mut(forge).insert(Spot(None));
    app.update();
    let land = app.world().resource::<Land>().clone();
    let fresh =
        Land::new(1, GenSettings::default(), Palette::default());
    assert_eq!(land.sites, fresh.sites);
    let free = fresh
        .sites
        .iter()
        .filter(|site| site.buildable)
        .map(|site| site.at)
        .collect::<Vec<_>>();
    let mut stand = towns(&mut app)
        .into_iter()
        .map(|(_, at)| at)
        .collect::<Vec<_>>();
    stand.sort_by(|a, b| a.x.total_cmp(&b.x));
    let mut first = free[..3].to_vec();
    first.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(stand, first);
}

#[test]
fn a_town_follows_where_the_record_sets_it_down() {
    let (mut app, hamlet) = app();
    let mill = settle(&mut app, "mill");
    tell(&mut app, hamlet, &[1, 2], &[]);
    let before = towns(&mut app);
    let people = app.world().resource::<Society>().clone();
    let to = meadow(&mut app, mill);
    let view = *app.world().resource::<GenView>();

    app.world_mut()
        .entity_mut(mill)
        .insert(Spot(Some((to.x, to.y))));
    app.update();
    let after = towns(&mut app);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], ("mill".to_string(), to));
    // Everyone is who and where in town they were.
    assert_eq!(*app.world().resource::<Society>(), people);
    // The camera looks where it looked, though the middle of
    // the hamlets is elsewhere now.
    let now = *app.world().resource::<GenView>();
    assert!(now.focus.distance(view.focus) < 0.01);
    assert!((now.dist - view.dist).abs() < 0.01);
    assert_ne!(app.world().resource::<Centre>().0, view.focus);

    // Ground that cannot take it leaves it where it is.
    let home = before[0].1;
    app.world_mut()
        .entity_mut(mill)
        .insert(Spot(Some((home.x + 6.0, home.y))));
    app.update();
    assert_eq!(towns(&mut app), after);

    // A world opened later puts it where the record says from
    // the start, and the others where they were.
    let mut later = bare();
    settle(&mut later, HOME);
    let mill = settle(&mut later, "mill");
    later
        .world_mut()
        .entity_mut(mill)
        .insert(Spot(Some((to.x, to.y))));
    later.update();
    assert_eq!(towns(&mut later), after);
}

#[test]
fn a_move_a_chief_commands_is_replayed_on_the_next_launch() {
    use hamlet_core::agents;
    use hamlet_core::core::HomeDir;
    use hamlet_home::Home;
    use hamlet_rules::chief;
    use hamlet_rules::realm::{Origin, Realm};
    use hamlet_testing::Scratch;

    let home = Scratch::new("world-moved");
    let launch = || {
        let mut app = bare();
        app.insert_resource(HomeDir(Home {
            root: home.to_path_buf(),
            workspace: home.join("workspace"),
        }))
        .add_plugins(agents::plugin);
        app.update();
        app.update();
        app
    };
    let realm = Realm::new(home.to_path_buf());
    let origin = Origin::Human {
        user: "alice".into(),
    };
    realm.found("mill", "miller", "Grinds.", origin).unwrap();
    let mut first = launch();
    let before = towns(&mut first);
    assert_eq!(before.len(), 2);
    let mill = {
        let world = first.world_mut();
        world
            .query::<(Entity, &Settlement)>()
            .iter(world)
            .find(|(_, settlement)| settlement.name == "mill")
            .map(|(entity, _)| entity)
            .unwrap()
    };
    let to = meadow(&mut first, mill);

    let said = chief::command(
        &realm,
        HOME,
        "alice",
        &format!("/move-hamlet mill {} {}", to.x, to.y),
    );
    assert!(said.unwrap().is_ok());
    // The world sees the record change and follows it.
    first.update();
    first.update();
    let after = towns(&mut first);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], ("mill".to_string(), to));

    let mut next = launch();
    assert_eq!(towns(&mut next), after);
    let people = |app: &App| {
        let society = app.world().resource::<Society>();
        society
            .agents
            .iter()
            .map(|a| (a.id, a.town, a.plot))
            .collect::<Vec<_>>()
    };
    assert_eq!(people(&next), people(&first));
}
