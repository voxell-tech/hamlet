//! The chief's deeds by hand in the world, carried to the core as the
//! commands a chief could type. A letter is put in the agent's inbox
//! and the agent is told. The panels say each [`Decree`] and
//! [`Tell`] in the chief's chat.

use std::path::Path;

use bevy::prelude::*;
use hamlet_core::agents::{self, LivesIn, Settlement};
use hamlet_core::core::HomeDir;
use hamlet_rules::gifts::{self, Offer};
use hamlet_rules::hamlet::{HOME, Hamlet};

use crate::chief::{Player, Stands};
use crate::generator::society::Place;
use crate::hamlet::{hamlet_of, town_of};
use crate::hand::{self, Wish};
use crate::library::Stood;
use crate::moving::{self, SetDown};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, grant);
}

/// A chief's command for the hamlet `hamlet`, as the chief would
/// type it.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Decree {
    pub hamlet: String,
    pub text: String,
}

/// Words from the chief to `agent`, in its own chat.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Tell {
    pub agent: Entity,
    pub text: String,
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

/// Gives a chief's command to the hamlet of `agent`, carried out by
/// the time this returns.
pub fn decree(world: &mut World, agent: Entity, text: String) {
    let hamlet =
        agents::hamlet_of(world, agent).unwrap_or(HOME).to_string();
    world.trigger(Decree { hamlet, text });
    world.flush();
}

/// Brings back `agent`, which was removed or merged away.
pub fn restore(world: &mut World, agent: Entity) {
    let text = format!("/restore {}", agents::name_of(world, agent));
    decree(world, agent, text);
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
pub fn introduce(world: &mut World, a: Entity, b: Entity) {
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
pub fn offers(world: &World, agent: Entity) -> Vec<Offer> {
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
    world.trigger(Tell {
        agent,
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
            let Some(hamlet) = hamlet_of(world, town) else {
                return;
            };
            let Some(name) = world
                .get::<Settlement>(hamlet)
                .map(|settlement| settlement.name.clone())
            else {
                return;
            };
            let (x, z) = (set.to.x, set.to.y);
            world.trigger(Decree {
                text: format!("/move-hamlet {name} {x:.1} {z:.1}"),
                hamlet: name,
            });
            world.flush();
        }
        Place::Chief => {
            let player = world
                .query_filtered::<Entity, With<Player>>()
                .iter(world)
                .next();
            if let Some(chief) = player {
                world.entity_mut(chief).insert(Stands(set.to));
            }
        }
        Place::Library => {
            if let Some(home) = world.get_resource::<HomeDir>() {
                let file = Stood::file(&home.0.root);
                let _ = Stands(set.to).write_at(&file);
            }
            world.resource_mut::<Stood>().0 = Some(set.to);
        }
    }
}

/// Carries out everything the hand asked for.
fn grant(
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
    use hamlet_core::agents::AgentsPlugin;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::Sited;
    use crate::play::Played;

    /// Every decree and every telling, as `(hamlet or agent, text)`.
    #[derive(Resource, Default)]
    struct Heard(Vec<(String, String)>);

    /// A home with two agents, and panels that carry out the chief's
    /// commands and nothing else.
    fn app(tag: &str) -> (Scratch, App, Hamlet) {
        let dir = Scratch::new(&format!("world-orders-{tag}"));
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
            |decree: On<Decree>,
             mut heard: ResMut<Heard>,
             mut commands: Commands| {
                let Decree { hamlet, text } = decree.event().clone();
                heard.0.push((hamlet.clone(), text.clone()));
                commands.queue(move |world: &mut World| {
                    let home = world
                        .query::<&Settlement>()
                        .iter(world)
                        .find(|kept| kept.name == hamlet)
                        .map(Settlement::record);
                    if let Some(home) = home {
                        let _ = home.decree("alice", &text);
                    }
                    agents::sync(world);
                });
            },
        )
        .add_observer(
            |tell: On<Tell>,
             names: Query<&Name>,
             mut heard: ResMut<Heard>| {
                let name = names.get(tell.agent).unwrap().to_string();
                heard.0.push((name, tell.text.clone()));
            },
        );
        (dir, app, hamlet)
    }

    fn grant_all(world: &mut World) {
        world.run_system_cached(grant).unwrap();
    }

    fn agent(app: &mut App, name: &str) -> Entity {
        agents::named(app.world_mut(), name).unwrap()
    }

    fn heard(app: &mut App) -> Vec<(String, String)> {
        std::mem::take(&mut app.world_mut().resource_mut::<Heard>().0)
    }

    #[test]
    fn a_town_set_down_goes_to_the_record_as_a_chiefs_command() {
        use hamlet_rules::chief;
        use hamlet_rules::realm::Realm;

        use crate::generator::society::TownId;

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
        assert_eq!(
            said,
            [(HOME.into(), "/move-hamlet home 12.5 -40.0".into())]
        );
        let realm = Realm::new(dir.to_path_buf());
        let done = chief::command(&realm, HOME, "alice", &said[0].1);
        assert!(done.unwrap().is_ok());
        assert_eq!(realm.hamlets()[0].at, Some((12.5, -40.0)));
        grant_all(app.world_mut());
        assert!(heard(&mut app).is_empty());
    }

    #[test]
    fn a_wish_for_a_road_builds_it_in_the_record() {
        let (_dir, mut app, hamlet) = app("road");
        let (a, b) =
            (agent(&mut app, "scout"), agent(&mut app, "librarian"));
        app.world_mut().write_message(Wish::Road { a, b });
        grant_all(app.world_mut());
        assert_eq!(
            heard(&mut app),
            [(HOME.into(), "/road scout librarian".into())]
        );
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
        assert_eq!(said[0].0, "scout");
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
