use std::path::Path;

use hamlet_home::limits;
use hamlet_rules::hamlet::{FOUNDER, MAYOR};
use hamlet_rules::lifecycle::Who;
use hamlet_rules::realm;
use hamlet_testing::Scratch;

use super::*;

fn app(home: &Path) -> App {
    let mut app = App::new();
    app.add_plugins(AgentsPlugin {
        home: home.to_path_buf(),
    });
    app
}

/// The address of `agent` inside its hamlet.
fn name(world: &World, agent: Entity) -> String {
    if world.get::<Mayor>(agent).is_some() {
        return MAYOR.to_string();
    }
    world.get::<Name>(agent).unwrap().as_str().to_string()
}

/// Asserts that the entities say what the record of the first
/// hamlet says.
fn in_step(app: &mut App, hamlet: &Hamlet) {
    let world = app.world_mut();
    let census = hamlet.census();
    let mut living = world
        .query_filtered::<(&Citizen, &Name, &Purpose, Has<Mayor>), Without<Left>>()
        .iter(world)
        .map(|(c, n, p, mayor)| {
            (c.0, n.as_str().to_string(), p.0.clone(), mayor)
        })
        .collect::<Vec<_>>();
    living.sort();
    let mut told = census
        .members()
        .iter()
        .map(|r| {
            let mayor = r.id == census.mayor.id;
            (r.id, r.name.clone(), r.purpose.clone(), mayor)
        })
        .collect::<Vec<_>>();
    told.sort();
    assert_eq!(living, told);

    let gone = world
        .query_filtered::<(&Citizen, Option<&MergedInto>), With<Left>>()
        .iter(world)
        .map(|(c, into)| (c.0, into.map(|m| m.0)))
        .collect::<Vec<_>>();
    let mut gone = gone
        .into_iter()
        .map(|(number, into)| {
            let into = into
                .map(|host| world.get::<Citizen>(host).unwrap().0);
            (number, into)
        })
        .collect::<Vec<_>>();
    gone.sort();
    let mut archived = hamlet
        .archived()
        .iter()
        .map(|a| (a.resident.id, a.merge.as_ref().map(|m| m.kept.id)))
        .collect::<Vec<_>>();
    archived.sort();
    assert_eq!(gone, archived);

    let roads = world
        .query::<(&Road, &Traffic)>()
        .iter(world)
        .map(|(road, traffic)| (*road, *traffic))
        .collect::<Vec<_>>();
    let mut roads = roads
        .into_iter()
        .map(|(road, traffic)| {
            let (a, b) =
                (name(world, road.ends.0), name(world, road.ends.1));
            (
                hamlet_rules::hamlet::ends(&a, &b),
                traffic.messages,
                traffic.last_from.map(|from| name(world, from)),
            )
        })
        .collect::<Vec<_>>();
    roads.sort();
    let mut told = census
        .roads
        .iter()
        .map(|r| (r.ends.clone(), r.messages, r.last_from.clone()))
        .collect::<Vec<_>>();
    told.sort();
    assert_eq!(roads, told);

    let report = hamlet.report("");
    let mut accounts = world
        .query::<&Account>()
        .iter(world)
        .map(|a| a.0.clone())
        .collect::<Vec<_>>();
    accounts.sort_by(|a, b| a.name.cmp(&b.name));
    let mut lines = report.agents.clone();
    lines.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(accounts, lines);
    let treasury =
        world.query::<&Treasury>().single(world).unwrap().0.clone();
    assert_eq!(treasury.hamlet, report.hamlet);
    let works = world.query::<&Works>().single(world).unwrap();
    assert_eq!(works.0, hamlet.work());
}

#[test]
fn entities_follow_the_record_through_every_kind_of_change() {
    let home = Scratch::new("agents-step");
    std::fs::write(limits::path(&home), "models = [\"small\"]\n")
        .unwrap();
    std::fs::write(
        hamlet_model::catalogue::path(&home),
        "[[model]]\nid = \"small\"\nallowed = true\n",
    )
    .unwrap();
    let hamlet = Hamlet::new(home.to_path_buf());
    let mut app = app(&home);
    in_step(&mut app, &hamlet);
    let step = |app: &mut App| {
        // Nobody told the app: it sees the files change.
        app.update();
        in_step(app, &hamlet);
    };

    hamlet.create("scout", "Looks things up.", "test").unwrap();
    hamlet
        .create("librarian", "Keeps the list.", "test")
        .unwrap();
    hamlet.create("clerk", "Files things.", "test").unwrap();
    step(&mut app);
    hamlet
        .build_road("scout", "librarian", "test", "t")
        .unwrap();
    step(&mut app);
    hamlet.carried("scout", "librarian", "t").unwrap();
    hamlet.carried(MAYOR, "clerk", "t").unwrap();
    step(&mut app);
    hamlet.choose_model("scout", "small", "test").unwrap();
    step(&mut app);
    let world = app.world_mut();
    let scout = named(world, "scout").unwrap();
    let account = &world.get::<Account>(scout).unwrap().0;
    assert_eq!(account.model, "small");

    hamlet.remove("clerk", "Not needed.", Who::Mayor).unwrap();
    step(&mut app);
    assert!(named(app.world_mut(), "clerk").is_none());
    hamlet
        .merge(
            "librarian",
            "scout",
            "Both.",
            "Overlap.",
            Who::Mayor,
            None,
        )
        .unwrap();
    step(&mut app);
    let world = app.world_mut();
    let librarian = named(world, "librarian").unwrap();
    assert_eq!(world.get::<MergedInto>(scout).unwrap().0, librarian);
    assert!(world.get::<Account>(scout).is_none());

    // The same entity comes back: an agent is its number.
    hamlet
        .restore("scout", None, "test", Who::Chief("alice"))
        .unwrap();
    hamlet
        .restore("clerk", Some("filer"), "test", Who::Chief("alice"))
        .unwrap();
    step(&mut app);
    let world = app.world_mut();
    assert_eq!(named(world, "scout"), Some(scout));
    assert!(world.get::<MergedInto>(scout).is_none());
    assert!(world.get::<Absorbed>(librarian).is_none());
    assert!(named(world, "filer").is_some());

    // The office moves: the marker follows it, and nobody is
    // another entity for it.
    let first = mayor(app.world_mut()).unwrap();
    hamlet.set_mayor("scout", "chief:alice", "test").unwrap();
    hamlet.carried(MAYOR, FOUNDER, "t").unwrap();
    step(&mut app);
    let world = app.world_mut();
    assert_eq!(mayor(world), Some(scout));
    assert_eq!(named(world, MAYOR), Some(scout));
    assert_eq!(named(world, FOUNDER), Some(first));
    assert!(world.get::<Mayor>(first).is_none());
    hamlet.set_mayor(FOUNDER, "chief:alice", "test").unwrap();
    step(&mut app);
    assert_eq!(mayor(app.world_mut()), Some(first));

    let project = Scratch::new("agents-step-project");
    hamlet.work_on(Some(&project), "test").unwrap();
    hamlet
        .set_mode(hamlet_rules::work::Mode::Auto, "test")
        .unwrap();
    step(&mut app);

    // A restart reads the same hamlet.
    let mut again = self::app(&home);
    in_step(&mut again, &hamlet);
    let count = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Citizen>().iter(world).count()
    };
    assert_eq!(count(&mut again), count(&mut app));
}

#[test]
fn a_message_is_told_once_and_not_for_what_was_there_at_the_start() {
    #[derive(Resource, Default)]
    struct Heard(Vec<(String, String)>);

    let home = Scratch::new("agents-carried");
    let hamlet = Hamlet::new(home.to_path_buf());
    hamlet.create("scout", "Looks things up.", "test").unwrap();
    hamlet.carried(MAYOR, "scout", "t").unwrap();
    let mut app = app(&home);
    app.init_resource::<Heard>().add_observer(
        |carried: On<Carried>,
         roads: Query<&Road>,
         names: Query<&Name>,
         mut heard: ResMut<Heard>| {
            let road = roads.get(carried.road).unwrap();
            assert!(road.has(carried.from));
            let other = if road.ends.0 == carried.from {
                road.ends.1
            } else {
                road.ends.0
            };
            let name = |e| names.get(e).unwrap().as_str().to_string();
            heard.0.push((name(carried.from), name(other)));
        },
    );
    app.update();
    assert!(app.world().resource::<Heard>().0.is_empty());

    hamlet.carried("scout", MAYOR, "t").unwrap();
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<Heard>().0,
        [("scout".to_string(), FOUNDER.to_string())]
    );

    // A road built and used before the app next looks.
    hamlet.create("clerk", "Files things.", "test").unwrap();
    hamlet.build_road("scout", "clerk", "test", "t").unwrap();
    hamlet.carried("clerk", "scout", "t").unwrap();
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<Heard>().0[1..],
        [("clerk".to_string(), "scout".to_string())]
    );
}

#[test]
fn every_hamlet_of_the_realm_is_followed_and_the_roads_between_them()
{
    let home = Scratch::new("agents-realm");
    let mut app = app(&home);
    let realm = Realm::new(home.to_path_buf());
    let first = realm.hamlet(HOME).unwrap();
    first.create("scout", "Looks things up.", "test").unwrap();
    let origin = realm::Origin::Human {
        user: "alice".into(),
    };
    let mill =
        realm.found("mill", "miller", "Grinds.", origin).unwrap();
    mill.create("grinder", "Turns the stone.", "test").unwrap();
    // Nobody told the app: it sees the files change.
    app.update();
    app.update();
    let world = app.world_mut();
    let miller = mayor_of(world, "mill").unwrap();
    assert_eq!(named(world, "miller"), Some(miller));
    assert_eq!(hamlet_of(world, miller), Some("mill"));
    let grinder = named(world, "grinder").unwrap();
    let scout = named(world, "scout").unwrap();
    let founder = mayor(world).unwrap();
    assert_ne!(founder, miller);
    let joined = |world: &mut World, a: Entity, b: Entity| {
        world
            .query::<(&Road, &Traffic)>()
            .iter(world)
            .find(|(road, _)| road.has(a) && road.has(b))
            .map(|(_, traffic)| traffic.messages)
    };
    // Every mayor has a road to every other.
    assert_eq!(joined(world, founder, miller), Some(0));
    assert_eq!(joined(world, miller, grinder), Some(0));
    assert_eq!(joined(world, scout, grinder), None);

    realm.agree_road(HOME, "scout", "grinder", "r").unwrap();
    realm.agree_road("mill", "scout", "grinder", "r").unwrap();
    realm.carried("scout", "grinder", HOME).unwrap();
    app.update();
    app.update();
    let world = app.world_mut();
    assert_eq!(joined(world, scout, grinder), Some(1));

    // An agent that moves is a new agent of the hamlet it joins.
    realm.agree_move(HOME, "scout", "mill", &[], "r").unwrap();
    realm.agree_move("mill", "scout", "mill", &[], "r").unwrap();
    app.update();
    app.update();
    let world = app.world_mut();
    let moved = named(world, "scout").unwrap();
    assert_ne!(moved, scout);
    assert_eq!(hamlet_of(world, moved), Some("mill"));
    assert!(world.get_entity(scout).is_err());
    assert_eq!(joined(world, miller, moved), Some(0));

    // A removed hamlet goes with its agents and its roads.
    realm.remove("mill", "alice", "test").unwrap();
    app.update();
    app.update();
    let world = app.world_mut();
    assert!(mayor_of(world, "mill").is_none());
    assert!(named(world, "grinder").is_none());
    assert_eq!(world.query::<&Road>().iter(world).count(), 0);
    assert_eq!(world.query::<&Settlement>().iter(world).count(), 1);
}

#[test]
fn a_change_to_the_limits_reaches_the_treasury() {
    let home = Scratch::new("agents-limits");
    let mut app = app(&home);
    let caps = |app: &mut App| {
        let world = app.world_mut();
        let treasury =
            world.query::<&Treasury>().single(world).unwrap();
        (treasury.0.hamlet.caps.cost, treasury.0.problem.clone())
    };
    assert_eq!(caps(&mut app).0, Some(5.0));
    std::fs::write(limits::path(&home), "[budget]\ncost = 7.0\n")
        .unwrap();
    app.update();
    assert_eq!(caps(&mut app).0, Some(7.0));
    // A mistake in the file is shown, not hidden.
    std::fs::write(limits::path(&home), "[budget\n").unwrap();
    // The file's time may not have moved within one tick.
    for _ in 0..200 {
        app.update();
        if caps(&mut app).1.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(caps(&mut app).1.is_some());
}
