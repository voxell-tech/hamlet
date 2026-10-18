use bevy::mesh::VertexAttributeValues;
use bevy::transform::TransformPlugin;
use hamlet_testing::Scratch;

use super::mascot::{Doing, Pose, crest};
use super::wickstep::Rune;
use super::*;
use crate::Clock;
use crate::generator::Passage;
use crate::generator::society::{AgentId, Society};
use crate::generator::testing::{rehearse, stand_in};
use crate::hamlet::Housed;
use crate::keeper::Act;
use crate::palette::Palette;
use crate::play::{Play, Played};
use crate::sky::Quality;

/// The deeds the world was told of.
#[derive(Resource, Default)]
struct Log(Vec<Play>);

fn collect(mut played: MessageReader<Played>, mut log: ResMut<Log>) {
    log.0.extend(played.read().map(|played| played.what));
}

/// A world of one agent with its robot at its porch and one chief
/// at home, with nothing drawn.
struct Cast {
    app: App,
    chief: Entity,
    agent: Entity,
    robot: Entity,
}

impl Cast {
    fn new() -> Self {
        let mut app = App::new();
        rehearse(&mut app);
        app.add_plugins(TransformPlugin)
            .init_resource::<Society>()
            .init_resource::<Quality>()
            .init_resource::<Log>()
            .add_message::<Played>()
            .add_systems(PostUpdate, collect);
        app.add_plugins(plugin);
        let world = app.world_mut();
        let agent = world.spawn(Housed(AgentId(1))).id();
        let robot = stand_in(world, AgentId(1), 1);
        let chief = arrive(world, "wren", Look::default());
        let mut cast = Cast {
            app,
            chief,
            agent,
            robot,
        };
        cast.run(0.2);
        cast
    }

    fn run(&mut self, secs: f32) {
        for _ in 0..(secs * 60.0).round() as usize {
            self.app.world_mut().resource_mut::<Clock>().t +=
                1.0 / 60.0;
            self.app.update();
        }
    }

    fn settled(&self) -> Settled {
        *self.app.world().get::<Settled>(self.chief).unwrap()
    }

    fn mascot(&self) -> &Mascot {
        let mascot = self.settled().mascot;
        self.app.world().get::<Mascot>(mascot).unwrap()
    }

    fn at(&self, entity: Entity) -> Vec3 {
        self.app
            .world()
            .get::<Transform>(entity)
            .unwrap()
            .translation
    }

    /// The mascot's place at home.
    fn waystone(&self) -> Vec3 {
        let set = self.app.world().resource::<ChiefSettings>();
        let stone = place::spots(set)[0].at;
        let frame = self
            .app
            .world()
            .get::<Transform>(self.settled().place)
            .unwrap();
        frame.transform_point(Vec3::new(stone.x, 0.0, stone.y))
    }

    fn talk_to(&mut self, agent: Option<Entity>) {
        self.app
            .world_mut()
            .get_mut::<Beside>(self.chief)
            .unwrap()
            .0 = agent;
    }

    /// Runs frames until the mascot stands lit at `stay`. Returns
    /// the seconds that took.
    fn until(&mut self, stay: Stay) -> f32 {
        for frame in 0..600 {
            let mascot = self.mascot();
            if mascot.stay == stay && !mascot.stepping() {
                return frame as f32 / 60.0;
            }
            self.run(1.0 / 60.0);
        }
        panic!("the mascot never came to {stay:?}");
    }
}

#[test]
fn talking_to_an_agent_sends_the_mascot_beside_it_by_wickstep() {
    let mut cast = Cast::new();
    let mascot = cast.settled().mascot;
    assert_eq!(cast.mascot().stay, Stay::Home);
    assert!(
        cast.at(mascot).xz().distance(cast.waystone().xz()) < 0.1
    );

    cast.talk_to(Some(cast.agent));
    cast.run(2.0 / 60.0);
    assert!(matches!(cast.mascot().phase, Phase::Out { .. }));
    let took = cast.until(Stay::With(cast.agent));
    assert!(took < 2.0, "{took}");

    // It stands by the robot's side, on the ground.
    let set = cast.app.world().resource::<ChiefSettings>().clone();
    let reach = Vec2::new(set.beside, set.before).length();
    let off = cast.at(mascot).xz().distance(cast.at(cast.robot).xz());
    assert!((off - reach).abs() < set.slack + 0.05, "{off}");
    let ground = cast
        .app
        .world()
        .resource::<Land>()
        .height(cast.at(mascot).xz());
    assert!((cast.at(mascot).y - ground).abs() < 1e-3);
    // Its agent waves at it as it comes.
    let greeting = cast.app.world().get::<Passage>(cast.robot);
    assert_eq!(greeting.map(Passage::act), Some(Act::Wave));
    cast.run(0.5);
    assert_eq!(cast.mascot().doing, Doing::Attend);
    assert_eq!(
        cast.app.world().resource::<Log>().0,
        [Play::WickOut, Play::WickIn]
    );

    // The circles and the motes are cleared away after it.
    cast.run(1.5);
    let world = cast.app.world_mut();
    assert_eq!(world.query::<&Rune>().iter(world).count(), 0);
    assert_eq!(
        world.query::<&wickstep::Mote>().iter(world).count(),
        0
    );
}

#[test]
fn talking_to_nobody_sends_the_mascot_home() {
    let mut cast = Cast::new();
    let mascot = cast.settled().mascot;
    cast.talk_to(Some(cast.agent));
    cast.until(Stay::With(cast.agent));
    cast.talk_to(None);
    let took = cast.until(Stay::Home);
    assert!(took < 2.0, "{took}");
    assert!(
        cast.at(mascot).xz().distance(cast.waystone().xz()) < 0.1
    );

    // An agent that is gone from the world is not stood by.
    cast.talk_to(Some(cast.agent));
    cast.until(Stay::With(cast.agent));
    cast.app.world_mut().entity_mut(cast.robot).despawn();
    cast.app.world_mut().entity_mut(cast.agent).despawn();
    cast.until(Stay::Home);
}

#[test]
fn the_mascot_follows_an_agent_that_strays() {
    let mut cast = Cast::new();
    let mascot = cast.settled().mascot;
    cast.talk_to(Some(cast.agent));
    cast.until(Stay::With(cast.agent));
    let set = cast.app.world().resource::<ChiefSettings>().clone();
    let reach = Vec2::new(set.beside, set.before).length();
    let apart = |cast: &Cast| {
        cast.at(mascot).xz().distance(cast.at(cast.robot).xz())
    };

    // The robot keeps to its porch here, so the two are parted by
    // moving the mascot. A few steps off: it walks after its agent.
    let shift = |cast: &mut Cast, by: Vec3| {
        let world = cast.app.world_mut();
        world.get_mut::<Transform>(mascot).unwrap().translation += by;
    };
    shift(&mut cast, Vec3::X * 2.5);
    cast.run(0.1);
    assert_eq!(cast.mascot().doing, Doing::Walk);
    assert!(!cast.mascot().stepping());
    cast.run(2.5);
    assert!((apart(&cast) - reach).abs() < set.slack + 0.05);

    // Further, but no further than across a town: it still walks,
    // and hurries.
    shift(&mut cast, Vec3::X * (set.restep + 3.0));
    cast.run(0.1);
    assert_eq!(cast.mascot().doing, Doing::Walk);
    assert!(!cast.mascot().stepping());

    // A long way off: it steps after it.
    shift(&mut cast, Vec3::X * (set.afoot + 3.0));
    cast.run(0.1);
    assert!(cast.mascot().stepping());
    cast.until(Stay::With(cast.agent));
    assert!((apart(&cast) - reach).abs() < set.slack + 0.05);
}

#[test]
fn at_home_the_mascot_goes_about_its_grounds() {
    let mut cast = Cast::new();
    let mascot = cast.settled().mascot;
    let set = cast.app.world().resource::<ChiefSettings>().clone();
    let frame = *cast
        .app
        .world()
        .get::<Transform>(cast.settled().place)
        .unwrap();
    let mut seen = vec![];
    let mut furthest = 0.0f32;
    for _ in 0..(90.0 * 60.0) as usize {
        let before = cast.at(mascot);
        cast.run(1.0 / 60.0);
        furthest = furthest.max(before.distance(cast.at(mascot)));
        let doing = cast.mascot().doing;
        if !seen.contains(&doing) {
            seen.push(doing);
        }
        // It never leaves its wall.
        let out =
            cast.at(mascot).xz().distance(frame.translation.xz());
        assert!(out < set.grounds, "{out}");
    }
    assert!(seen.contains(&Doing::Walk), "{seen:?}");
    assert!(seen.len() >= 4, "{seen:?}");
    // On foot it never moves further than a step in a frame.
    assert!(furthest < set.walk_speed / 60.0 + 0.02, "{furthest}");
    assert_eq!(cast.mascot().stay, Stay::Home);
}

#[test]
fn a_still_shows_the_mascot_there_with_no_magic() {
    let mut cast = Cast::new();
    cast.app.world_mut().resource_mut::<Clock>().frozen = true;
    cast.talk_to(Some(cast.agent));
    cast.app.update();
    assert_eq!(cast.mascot().stay, Stay::With(cast.agent));
    assert!(!cast.mascot().stepping());
    cast.app.update();
    let world = cast.app.world_mut();
    assert_eq!(world.query::<&Rune>().iter(world).count(), 0);
    assert!(world.resource::<Log>().0.is_empty());
}

#[test]
fn a_chief_that_leaves_takes_its_place_and_its_mascot() {
    let mut cast = Cast::new();
    let settled = cast.settled();
    cast.app.world_mut().entity_mut(cast.chief).despawn();
    cast.run(0.1);
    let world = cast.app.world();
    assert!(world.get_entity(settled.place).is_err());
    assert!(world.get_entity(settled.mascot).is_err());
}

fn finite(mesh: &Mesh) -> bool {
    [Mesh::ATTRIBUTE_POSITION, Mesh::ATTRIBUTE_NORMAL]
        .into_iter()
        .all(|attribute| match mesh.attribute(attribute) {
            Some(VertexAttributeValues::Float32x3(values)) => {
                !values.is_empty()
                    && values.iter().flatten().all(|v| v.is_finite())
            }
            _ => false,
        })
}

#[test]
fn every_look_builds_finite_meshes() {
    let pal = Palette::default();
    let set = ChiefSettings::default();
    assert_eq!(Look::all().count(), 800);
    // The light is no part of a mesh.
    for look in Look::all().filter(|l| l.light == Light::default()) {
        let parts = mascot::parts(&pal, look);
        let held = !parts.item.is_empty();
        assert_eq!(held, look.holds != Holds::Nothing, "{look:?}");
        assert!(parts.cloak.tris() > 50 && parts.hat.tris() > 20);
        for mb in parts.each() {
            assert!(finite(&mb.build()), "{look:?}");
        }
        assert!(crest(look.hat).is_finite());
    }
    assert!(finite(&mascot::wick().build()));
    for cloak in Cloak::ALL {
        let look = Look {
            cloak,
            ..Look::default()
        };
        let built = place::build(&pal, &set, look, |at| at.x * 0.01);
        assert!(built.body.tris() > 1000);
        assert!(finite(&built.body.build()));
        assert!(finite(&built.panes.build()));
        assert!(built.lights.orb.0.is_finite());
    }
}

#[test]
fn every_pose_is_finite_and_a_wickstep_ends_as_it_began() {
    let doings = [
        Doing::Idle,
        Doing::Walk,
        Doing::Read,
        Doing::Tend,
        Doing::Gaze,
        Doing::Attend,
        Doing::Wave,
    ];
    let whole = |pose: &Pose| {
        [pose.lift, pose.girth, pose.height, pose.lean, pose.glow]
            .iter()
            .all(|v| v.is_finite())
            && pose.wick.is_finite()
            && pose.hands.iter().all(|hand| hand.is_finite())
    };
    for doing in doings {
        for step in 0..400 {
            let t = step as f32 * 0.037;
            let pose = Pose::of(doing, t);
            assert!(whole(&pose), "{doing:?} {t}");
            assert!((0.0..=1.0).contains(&pose.blink));
            // Neither end of a Wickstep jumps.
            let long = 0.5;
            let out = Phase::Out { began: t, long };
            let into = Phase::In { began: t, long };
            assert_eq!(pose.stepping(out, t), pose);
            let gone = pose.stepping(out, t + long);
            assert!(
                gone.girth.abs() < 1e-4 && gone.glow.abs() < 1e-4
            );
            let lands = pose.stepping(into, t + long);
            assert!((lands.girth - pose.girth).abs() < 1e-3);
            assert!((lands.height - pose.height).abs() < 1e-3);
            for part in 0..=10 {
                let now = t + long * part as f32 / 10.0;
                assert!(whole(&pose.stepping(out, now)));
                assert!(whole(&pose.stepping(into, now)));
            }
        }
    }
}

#[test]
fn a_look_is_kept_and_read_back() {
    let home = Scratch::new("chief-kept-look");
    // Nothing kept: the first look.
    assert_eq!(Look::read(&home, "wren"), Look::default());
    for look in Look::all().step_by(37) {
        look.write(&home, "wren").unwrap();
        assert_eq!(Look::read(&home, "wren"), look);
    }
    // Each chief has a file of its own, whatever its name.
    let other = Look {
        hat: Hat::Wreath,
        ..Look::default()
    };
    other.write(&home, "a/b c").unwrap();
    assert_eq!(Look::read(&home, "a/b c"), other);
    assert_ne!(Look::read(&home, "wren"), other);
    assert!(
        Look::file(&home, "a/b c").starts_with(home.join("chiefs"))
    );

    // A file that names less, or names what is unknown.
    std::fs::write(Look::file(&home, "wren"), "hat = \"crown\"\n")
        .unwrap();
    let read = Look::read(&home, "wren");
    assert_eq!(read.hat, Hat::Crown);
    assert_eq!(read.cloak, Cloak::default());
    std::fs::write(Look::file(&home, "wren"), "hat = \"helmet\"\n")
        .unwrap();
    assert_eq!(Look::read(&home, "wren"), Look::default());
}

#[test]
fn a_place_set_down_elsewhere_stands_there_and_is_remembered() {
    let mut cast = Cast::new();
    let was = cast.at(cast.settled().place).xz();
    let to = was + Vec2::new(40.0, 25.0);
    let chief = cast.chief;
    cast.app.world_mut().entity_mut(chief).insert(Stands(to));
    cast.run(0.2);
    let now = cast.at(cast.settled().place).xz();
    assert!(now.distance(to) < 0.01, "{now} {to}");
    // The ground it holds went with it.
    let society = cast.app.world().resource::<Society>();
    assert_eq!(society.claims.len(), 1);
    assert_eq!(society.claims[0].at, to);

    let home = Scratch::new("chief-kept-place");
    assert_eq!(Stands::read(&home, "wren"), None);
    Stands(to).write(&home, "wren").unwrap();
    assert_eq!(Stands::read(&home, "wren"), Some(Stands(to)));
}

#[test]
fn a_chief_keeps_a_site_of_its_own_beside_the_first_town() {
    let mut cast = Cast::new();
    let land = cast.app.world().resource::<Land>().clone();
    let spacing = land.set.land.site_spacing;
    let first = land.sites.iter().find(|s| s.buildable).unwrap().at;
    let place = cast.at(cast.settled().place).xz();
    assert!(place.distance(first) > spacing * 0.5);
    assert!(place.distance(first) < spacing * 1.5);
    assert_eq!(place::stand(&land, &[]), Some(place));

    // A second chief stands somewhere else.
    let other =
        arrive(cast.app.world_mut(), "robin", Look::default());
    cast.run(0.1);
    let settled = *cast.app.world().get::<Settled>(other).unwrap();
    let second = cast.at(settled.place).xz();
    assert!(second.distance(place) > spacing * 0.5);

    // No town is ever founded on either, and the first town still
    // takes the first site.
    let mut land = land;
    let mut society = cast.app.world_mut().resource_mut::<Society>();
    let mut towns = vec![];
    for _ in 0..12 {
        let town = society.found_town(&mut land);
        let site = society.town(town).unwrap().site;
        towns.push(land.sites[site].at);
    }
    assert_eq!(towns[0], first);
    for town in towns {
        assert!(town.distance(place) > 1.0);
        assert!(town.distance(second) > 1.0);
    }
}
