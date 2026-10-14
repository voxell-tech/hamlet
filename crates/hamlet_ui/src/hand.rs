//! The chief's deeds by hand in the world, carried to the core as the
//! commands a chief could type. A letter is put in the agent's inbox
//! and the agent is told.

use std::path::Path;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageReader;
use bevy::ecs::name::Name;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_core::agents::{self, LivesIn, Mayor, Settlement};
use hamlet_core::session::{SessionKey, UserMessage};
use hamlet_rules::gifts::{self, Offer};
use hamlet_rules::hamlet::{HOME, Hamlet};
use hamlet_world::chief::Stands;
use hamlet_world::generator::society::Place;
use hamlet_world::hamlet::{hamlet_of, town_of};
use hamlet_world::hand::{self, Wish};
use hamlet_world::library::Stood;
use hamlet_world::moving::{self, SetDown};

use crate::chat;
use crate::me::Me;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, grant);
}

fn name(world: &World, agent: Entity) -> Option<String> {
    world
        .get::<Name>(agent)
        .map(|name| name.as_str().to_string())
}

/// The record of the hamlet `agent` lives in.
fn record(world: &World, agent: Entity) -> Option<Hamlet> {
    let lives = world.get::<LivesIn>(agent)?;
    Some(world.get::<Settlement>(lives.0)?.record())
}

/// Gives a chief's command to the hamlet of `agent`.
pub(super) fn decree(world: &mut World, agent: Entity, text: String) {
    let hamlet =
        agents::hamlet_of(world, agent).unwrap_or(HOME).to_string();
    chat::decree(world, &hamlet, text);
}

/// Builds a road between two agents at the chief's word.
fn road(world: &mut World, a: Entity, b: Entity) {
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/road {a_name} {b_name}"));
    }
}

/// Takes up the road between two agents at the chief's word.
pub fn part(world: &mut World, a: Entity, b: Entity) {
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/unroad {a_name} {b_name}"));
    }
}

/// Has two agents greet each other and builds a road between them.
pub(crate) fn introduce(world: &mut World, a: Entity, b: Entity) {
    hand::greet(world, a, b);
    road(world, a, b);
}

/// Gives `agent` the skill of `offer`, and shows it arrive if the
/// core agreed.
pub fn give(world: &mut World, agent: Entity, offer: &Offer) {
    let Some(name) = name(world, agent) else {
        return;
    };
    let skill = &offer.skill;
    let text = match &offer.from {
        Some(from) => format!("/give {name} {skill} from {from}"),
        None => format!("/give {name} {skill}"),
    };
    decree(world, agent, text);
    let has = record(world, agent)
        .and_then(|hamlet| hamlet.house(&name).ok())
        .is_some_and(|home| home.skills().join(skill).is_dir());
    if has {
        hand::gifted(world, agent);
    }
}

/// The skills `agent` could be given.
pub(crate) fn offers(world: &World, agent: Entity) -> Vec<Offer> {
    name(world, agent)
        .zip(record(world, agent))
        .map(|(name, hamlet)| hamlet.offers(&name))
        .unwrap_or_default()
}

/// Puts the file at `path` in the inbox of `agent` and tells it, as
/// a task from the chief.
fn letter(world: &mut World, agent: Entity, path: &Path) {
    let (Some(name), Some(hamlet)) =
        (name(world, agent), record(world, agent))
    else {
        return;
    };
    let at = match hamlet.hand(&name, path) {
        Ok(at) => at,
        Err(e) => {
            eprintln!("hamlet: could not hand over a file: {e:#}");
            return;
        }
    };
    let user = world.resource::<Me>().user.clone();
    let key = if world.get::<Mayor>(agent).is_some() {
        SessionKey::with_mayor("app", &user, hamlet.name())
    } else {
        SessionKey::with_agent("app", &user, &name)
    };
    world.trigger(UserMessage {
        key,
        user,
        text: gifts::letter(&at),
    });
    world.flush();
}

/// Takes the town of `hamlet` in hand, to be set down with a click
/// on the ground.
pub fn carry(world: &mut World, hamlet: Entity) {
    if let Some(town) = town_of(world, hamlet) {
        moving::pick_up(world, Place::Town(town));
    }
}

/// Records the place a chief set something down. The world moves it
/// when the record says so.
fn set_down(world: &mut World, set: SetDown) {
    match set.place {
        Place::Town(town) => {
            let name = hamlet_of(world, town).and_then(|hamlet| {
                Some(world.get::<Settlement>(hamlet)?.name.clone())
            });
            if let Some(name) = name {
                let (x, z) = (set.to.x, set.to.y);
                chat::send(
                    world,
                    format!("/move-hamlet {name} {x:.1} {z:.1}"),
                );
            }
        }
        Place::Chief => crate::chief::set_home(world, set.to),
        Place::Library => {
            let home = world
                .resource::<hamlet_core::core::HomeDir>()
                .0
                .root
                .clone();
            let _ = Stands(set.to).write_at(&Stood::file(&home));
            world.resource_mut::<Stood>().0 = Some(set.to);
        }
    }
}

/// Carries out everything the hand asked for.
pub(crate) fn grant(
    mut set: MessageReader<SetDown>,
    mut wishes: MessageReader<Wish>,
    mut commands: Commands,
) {
    for set in set.read().copied() {
        commands.queue(move |world: &mut World| set_down(world, set));
    }
    for wish in wishes.read().cloned() {
        commands.queue(move |world: &mut World| match wish {
            Wish::Road { a, b } => road(world, a, b),
            Wish::Letter { to, path } => letter(world, to, &path),
        });
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::observer::On;
    use bevy::ecs::query::With;
    use bevy::ecs::resource::Resource;
    use bevy::ecs::system::ResMut;
    use bevy::math::Vec2;
    use hamlet_core::agents::AgentsPlugin;
    use hamlet_testing::Scratch;
    use hamlet_world::play::Played;

    use super::*;

    /// Every message that reached the core, with its session.
    #[derive(Resource, Default)]
    struct Heard(Vec<(SessionKey, String)>);

    /// A home with two agents, a chat with the mayor, and a core
    /// that carries out the chief's commands and nothing else.
    fn app(tag: &str) -> (Scratch, App, Hamlet) {
        let dir = Scratch::new(&format!("ui-hand-{tag}"));
        let hamlet = Hamlet::new(dir.to_path_buf());
        hamlet
            .create("librarian", "Keeps the list.", "test")
            .unwrap();
        hamlet.create("scout", "Looks things up.", "test").unwrap();
        let mut app = App::new();
        app.add_plugins(AgentsPlugin {
            home: dir.to_path_buf(),
        })
        .init_resource::<Heard>()
        .add_message::<Wish>()
        .add_message::<SetDown>()
        .add_message::<Played>()
        .add_observer(
            |said: On<UserMessage>,
             mut heard: ResMut<Heard>,
             mut commands: Commands| {
                heard.0.push((said.key.clone(), said.text.clone()));
                let said = said.event().clone();
                commands.queue(move |world: &mut World| {
                    let home = world
                        .query::<&Settlement>()
                        .iter(world)
                        .next()
                        .map(Settlement::record);
                    if let Some(home) = home {
                        let _ = home.decree(&said.user, &said.text);
                    }
                    agents::sync(world);
                });
            },
        );
        let mayor = agents::mayor(app.world_mut()).unwrap();
        app.insert_resource(Me::of(mayor, mayor, "alice", None));
        (dir, app, hamlet)
    }

    fn grant_all(world: &mut World) {
        world.run_system_cached(grant).unwrap();
    }

    fn agent(app: &mut App, name: &str) -> Entity {
        agents::named(app.world_mut(), name).unwrap()
    }

    #[test]
    fn a_town_set_down_goes_to_the_record_as_a_chiefs_command() {
        use hamlet_rules::chief;
        use hamlet_rules::realm::Realm;
        use hamlet_world::generator::society::TownId;
        use hamlet_world::hamlet::Sited;

        let (dir, mut app, _) = app("move");
        let world = app.world_mut();
        let home = world
            .query_filtered::<Entity, With<Settlement>>()
            .single(world)
            .unwrap();
        world.entity_mut(home).insert(Sited(TownId(4)));
        world.write_message(SetDown {
            place: Place::Town(TownId(4)),
            to: Vec2::new(12.5, -40.0),
        });
        // A place nobody keeps a record of asks nothing.
        world.write_message(SetDown {
            place: Place::Town(TownId(9)),
            to: Vec2::ZERO,
        });
        grant_all(world);
        let said = heard(&mut app);
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].1, "/move-hamlet home 12.5 -40.0");
        let realm = Realm::new(dir.to_path_buf());
        let done = chief::command(&realm, HOME, "alice", &said[0].1);
        assert!(done.unwrap().is_ok());
        assert_eq!(realm.hamlets()[0].at, Some((12.5, -40.0)));
        grant_all(app.world_mut());
        assert!(heard(&mut app).is_empty());
    }

    fn heard(app: &mut App) -> Vec<(SessionKey, String)> {
        std::mem::take(&mut app.world_mut().resource_mut::<Heard>().0)
    }

    #[test]
    fn a_wish_for_a_road_builds_it_in_the_record() {
        let (_dir, mut app, hamlet) = app("road");
        let (a, b) =
            (agent(&mut app, "scout"), agent(&mut app, "librarian"));
        app.world_mut().write_message(Wish::Road { a, b });
        grant_all(app.world_mut());
        let said = heard(&mut app);
        assert_eq!(said.len(), 1);
        assert_eq!(
            said[0].0,
            SessionKey::with_mayor("app", "alice", HOME)
        );
        assert_eq!(said[0].1, "/road scout librarian");
        assert!(hamlet.has_road("scout", "librarian"));
        // The road is in the world as an entity.
        let world = app.world_mut();
        let joined = world
            .query::<&agents::Road>()
            .iter(world)
            .any(|road| road.has(a) && road.has(b));
        assert!(joined);
        // It was asked for once.
        grant_all(app.world_mut());
        assert!(heard(&mut app).is_empty());

        // Taken up again, it leaves the record and the world.
        part(app.world_mut(), a, b);
        assert_eq!(heard(&mut app)[0].1, "/unroad scout librarian");
        assert!(!hamlet.has_road("scout", "librarian"));
        app.update();
        let world = app.world_mut();
        let joined = world
            .query::<&agents::Road>()
            .iter(world)
            .any(|road| road.has(a) && road.has(b));
        assert!(!joined);
    }

    #[test]
    fn a_deed_goes_to_the_hamlet_of_the_agent_it_is_about() {
        use hamlet_rules::realm::{Origin, Realm};

        use crate::world_menu::Deed;

        let (dir, mut app, _) = app("deed");
        let user = "alice".to_string();
        let mill = Realm::new(dir.to_path_buf())
            .found(
                "mill",
                "miller",
                "Grinds.",
                Origin::Human { user },
            )
            .unwrap();
        mill.create("carder", "Cards wool.", "test").unwrap();
        agents::sync(app.world_mut());
        let carder = agent(&mut app, "carder");
        // The chat is with the first hamlet's mayor.
        Deed::Remove(carder).run(app.world_mut());
        let said = heard(&mut app);
        assert_eq!(
            said[0].0,
            SessionKey::with_mayor("app", "alice", "mill")
        );
        assert_eq!(said[0].1, "/remove carder");
    }

    #[test]
    fn the_chat_stays_true_to_its_agent_as_offices_change_and_agents_go()
     {
        use hamlet_core::core::HomeDir;

        let (dir, mut app, hamlet) = app("reseat");
        app.insert_resource(HomeDir(hamlet_home::Home {
            root: dir.to_path_buf(),
            workspace: dir.join("workspace"),
        }));
        let scout = agent(&mut app, "scout");
        let world = app.world_mut();
        assert!(crate::me::seat(world, scout));
        assert_eq!(
            world.resource::<Me>().with.as_deref(),
            Some("scout")
        );
        // The agent it is with takes the office.
        hamlet.set_mayor("scout", "chief:alice", "x").unwrap();
        agents::sync(world);
        crate::me::reseat(world);
        let me = world.resource::<Me>();
        assert_eq!((me.agent, me.with.as_deref()), (scout, None));
        assert_eq!(
            me.key,
            SessionKey::with_mayor("app", "alice", HOME)
        );
        // One that is gone from the world gives way to the mayor.
        let librarian = agents::named(world, "librarian").unwrap();
        assert!(crate::me::seat(world, librarian));
        world.despawn(librarian);
        crate::me::reseat(world);
        assert_eq!(world.resource::<Me>().agent, scout);
    }

    #[test]
    fn a_letter_is_put_in_the_inbox_and_the_agent_is_told() {
        let (dir, mut app, hamlet) = app("letter");
        let scout = agent(&mut app, "scout");
        let path = dir.join("list.txt");
        std::fs::write(&path, "Dune").unwrap();
        app.world_mut()
            .write_message(Wish::Letter { to: scout, path });
        grant_all(app.world_mut());
        let inbox =
            hamlet.house("scout").unwrap().workspace.join("inbox");
        assert_eq!(
            std::fs::read_to_string(inbox.join("list.txt")).unwrap(),
            "Dune"
        );
        let said = heard(&mut app);
        assert_eq!(said.len(), 1);
        assert_eq!(
            said[0].0,
            SessionKey::with_agent("app", "alice", "scout")
        );
        assert!(
            said[0].1.contains("`inbox/list.txt`"),
            "{}",
            said[0].1
        );

        // A file that is not there tells nobody anything.
        let path = dir.join("missing.txt");
        app.world_mut()
            .write_message(Wish::Letter { to: scout, path });
        grant_all(app.world_mut());
        assert!(heard(&mut app).is_empty());
    }

    #[test]
    fn a_skill_on_offer_is_copied_into_the_house() {
        let (_dir, mut app, hamlet) = app("give");
        let made =
            hamlet.house("scout").unwrap().skills().join("maps");
        std::fs::create_dir_all(&made).unwrap();
        std::fs::write(
            made.join("SKILL.md"),
            "---\nname: maps\ndescription: x\n---\n",
        )
        .unwrap();
        let librarian = agent(&mut app, "librarian");
        let offered = offers(app.world(), librarian);
        assert_eq!(
            offered,
            [Offer {
                skill: "maps".into(),
                from: Some("scout".into()),
            }]
        );
        give(app.world_mut(), librarian, &offered[0]);
        assert_eq!(
            heard(&mut app)[0].1,
            "/give librarian maps from scout"
        );
        let got = hamlet.house("librarian").unwrap().skills();
        assert!(got.join("maps/SKILL.md").is_file());
        assert!(offers(app.world(), librarian).is_empty());
    }
}
