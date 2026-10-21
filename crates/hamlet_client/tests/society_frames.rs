use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_client::chats::{Chat, Entry, Transcript};
use hamlet_client::realm::*;
use hamlet_client::{ClientPlugin, receive};
use hamlet_protocol as protocol;

#[derive(Resource, Default)]
struct SeenEvents {
    letters: usize,
    ballots: usize,
    library: usize,
}

fn replay(text: &str, mut inspect: impl FnMut(&mut World)) -> App {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None })
        .init_resource::<SeenEvents>()
        .add_observer(
            |_: On<Carried>, mut seen: ResMut<SeenEvents>| {
                seen.letters += 1
            },
        )
        .add_observer(|_: On<Voted>, mut seen: ResMut<SeenEvents>| {
            seen.ballots += 1
        })
        .add_observer(
            |_: On<Consulted>, mut seen: ResMut<SeenEvents>| {
                seen.library += 1
            },
        );
    for line in text.lines() {
        let frame =
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        let world = app.world_mut();
        if frame["from"] == "client" {
            let message = serde_json::from_value::<
                protocol::ClientMessage,
            >(frame["message"].clone())
            .unwrap();
            if let protocol::ClientMessage::Open(open) = message
                && !world
                    .query::<&Chat>()
                    .iter(world)
                    .any(|chat| chat.0 == open.chat)
            {
                world.spawn(Chat(open.chat));
            }
        } else {
            receive(
                world,
                serde_json::from_value(frame["message"].clone())
                    .unwrap(),
            );
        }
        app.update();
        inspect(app.world_mut());
    }
    app
}

fn entity(world: &mut World, number: u64) -> Entity {
    world
        .query::<(Entity, &Citizen)>()
        .iter(world)
        .find(|(_, citizen)| citizen.0 == number)
        .unwrap()
        .0
}

#[test]
fn recorded_society_carries_letters_and_questions_with_their_authors()
{
    let mut app = replay(
        include_str!("../../hamlet_protocol/frames/society.jsonl"),
        |_| {},
    );
    let world = app.world_mut();
    assert_eq!(world.query::<&Citizen>().iter(world).count(), 4);
    assert_eq!(world.query::<&Road>().iter(world).count(), 4);
    assert!(world.resource::<SeenEvents>().letters >= 2);
    let scout = entity(world, 1);
    assert!(
        world
            .get::<PeerChats>(scout)
            .unwrap()
            .0
            .iter()
            .any(|chat| chat.kind
                == protocol::ConversationViewKind::Questions
                && chat.count == 1)
    );
    assert!(world.query::<&Transcript>().iter(world).flat_map(Transcript::items).any(|item| matches!(&item.entry, Entry::Agent {name, text} if name == "smith@home" && text.contains("Hinge ready"))));
}

#[test]
fn recorded_leaving_restores_the_same_agent_then_resolves_its_merge_host()
 {
    let mut removed = false;
    let mut restored = false;
    let mut app = replay(
        include_str!("../../hamlet_protocol/frames/leaving.jsonl"),
        |world| {
            let scout = world
                .query::<(Entity, &Citizen)>()
                .iter(world)
                .find(|(_, c)| c.0 == 1)
                .map(|(e, _)| e);
            if let Some(scout) = scout {
                if world.get::<Left>(scout).is_some()
                    && world.get::<MergedInto>(scout).is_none()
                {
                    removed = true;
                }
                if removed && world.get::<Left>(scout).is_none() {
                    restored = true;
                }
            }
        },
    );
    assert!(removed && restored);
    let world = app.world_mut();
    let scout = entity(world, 1);
    let smith = entity(world, 2);
    assert!(world.get::<Left>(scout).is_some());
    assert_eq!(world.get::<MergedInto>(scout).unwrap().0, smith);
    assert!(world.get::<Lived>(scout).unwrap().until.is_some());
}

#[test]
fn recorded_vote_casts_ballots_and_moves_the_office() {
    let mut app = replay(
        include_str!("../../hamlet_protocol/frames/vote.jsonl"),
        |_| {},
    );
    let world = app.world_mut();
    let smith = named(world, "smith").unwrap();
    assert!(world.get::<Mayor>(smith).is_some());
    assert_eq!(world.resource::<SeenEvents>().ballots, 3);
    let votes = world.query::<&Votes>().single(world).unwrap();
    assert_eq!(votes.0.len(), 1);
    assert!(!votes.0[0].open);
    assert!(votes.0[0].report.contains("Passed: 3 yes of 3"));
}

#[test]
fn recorded_realm_places_a_second_town_builds_a_road_and_closes_then_erases_it()
 {
    let mut placed = false;
    let mut built = false;
    let mut closed = false;
    let mut app = replay(
        include_str!("../../hamlet_protocol/frames/realm.jsonl"),
        |world| {
            placed |= world
                .query::<&Spot>()
                .iter(world)
                .any(|spot| spot.0 == Some((60.0, 80.0)));
            built |= world.query::<&Road>().iter(world).any(|road| {
                world
                    .get::<LivesIn>(road.ends.0)
                    .zip(world.get::<LivesIn>(road.ends.1))
                    .is_some_and(|(a, b)| a.0 != b.0)
            });
            closed |= world.get_resource::<Closed>().is_some_and(
                |closed| closed.0.iter().any(|h| h.id == "mill"),
            );
        },
    );
    assert!(placed && built && closed);
    let world = app.world_mut();
    assert!(world.resource::<Closed>().0.is_empty());
    assert_eq!(world.query::<&Settlement>().iter(world).count(), 1);
    assert!(
        !world
            .query::<&Name>()
            .iter(world)
            .any(|name| name.as_str() == "miller")
    );
}

#[test]
fn recorded_library_consults_then_replaces_the_shelf_on_shelving_and_unshelving()
 {
    let mut shelved = false;
    let app = replay(
        include_str!("../../hamlet_protocol/frames/library.jsonl"),
        |world| {
            shelved |= world.get_resource::<Skills>().is_some_and(
                |skills| {
                    skills.0.iter().any(|s| s.name == "map-iron")
                },
            );
        },
    );
    assert!(shelved);
    assert!(app.world().resource::<SeenEvents>().library > 0);
    let shelf = &app.world().resource::<Skills>().0;
    assert_eq!(shelf.len(), 8);
    assert!(!shelf.iter().any(|s| s.name == "map-iron"));
}
