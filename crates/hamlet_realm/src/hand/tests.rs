use std::time::Duration;

use bevy::time::TimeUpdateStrategy;
use hamlet_client::ClientPlugin;
use hamlet_client::realm::HamletId;
use hamlet_protocol::client as protocol;
use hamlet_testing::realm;

use super::*;
use crate::generator::society::AgentId;
use crate::generator::testing::{rehearse, stand_in};
use crate::keeper::Keeper;
use crate::pick::PickSettings;
use crate::play::{Play, Played};
use crate::sky::CameraSettings;
use crate::testing::receive_state;
use crate::{Clock, bubble};

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
                    Thing::Agent(agent),
                    ChildOf(robot),
                    Pickable::default(),
                    Transform::default(),
                ))
                .id();
            agents.push(agent);
            robots.push(robot);
            boxes.push(hit);
        }
        let mut cast = Cast {
            app,
            agents,
            robots,
            boxes,
        };
        cast.run(0.5);
        cast
    }

    /// Runs `secs` of frames. Returns every motion the first robot
    /// was seen in.
    fn run(&mut self, secs: f32) -> Vec<Act> {
        let mut acts = Vec::new();
        for _ in 0..(secs * 60.0).round() as usize {
            self.app.world_mut().resource_mut::<Clock>().t +=
                1.0 / 60.0;
            self.app.update();
            let act = self.act(0);
            if !acts.contains(&act) {
                acts.push(act);
            }
        }
        acts
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

    fn set(&mut self) -> Mut<'_, PlaySettings> {
        self.app.world_mut().resource_mut::<PlaySettings>()
    }

    /// The deeds played since this was last asked, the selecting
    /// that every click does aside.
    fn played_at(&mut self) -> Vec<Play> {
        let mut log = self.app.world_mut().resource_mut::<Log>();
        let mut played = std::mem::take(&mut log.played);
        played.retain(|what| *what != Play::Select);
        played
    }

    fn wishes(&mut self) -> Vec<Wish> {
        let mut log = self.app.world_mut().resource_mut::<Log>();
        std::mem::take(&mut log.wishes)
    }

    fn hover(&mut self, thing: Option<Thing>) {
        self.app.world_mut().resource_mut::<Hovered>().0 = thing;
    }
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
    let house = Thing::House(cast.agents[0]);
    cast.hover(Some(house));
    let acts = cast.run(0.3);
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

/// The client and the orders on `cast`, with letters walked on
/// `runtime`'s blocking threads.
fn with_gifts(cast: &mut Cast, runtime: &tokio::runtime::Runtime) {
    cast.app
        .add_plugins(ClientPlugin { server: None })
        .insert_resource(hamlet_client::Tokio(
            runtime.handle().clone(),
        ))
        .add_message::<crate::moving::SetDown>()
        .add_plugins(crate::orders::plugin);
}

/// The requests queued once the dropped files are walked off the
/// frame.
fn walked(cast: &mut Cast) -> Vec<protocol::ClientMessage> {
    use hamlet_client::outbox::Outbox;
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while cast
        .app
        .world()
        .resource::<Outbox>()
        .iter()
        .next()
        .is_none()
        && std::time::Instant::now() < until
    {
        std::thread::sleep(Duration::from_millis(1));
        cast.app.update();
    }
    cast.app
        .world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .collect()
}

#[test]
fn four_files_dropped_together_send_one_manifest_with_four_roots() {
    use hamlet_client::realm::{AgentNumber, Deeds, LivesIn};
    let dir = hamlet_testing::Scratch::new("hand-four-files");
    let mut cast = Cast::new(1);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    with_gifts(&mut cast, &runtime);
    cast.app.insert_resource(Deeds(vec!["give".into()]));
    let hamlet =
        cast.app.world_mut().spawn(HamletId("home".into())).id();
    cast.app.world_mut().entity_mut(cast.agents[0]).insert((
        AgentNumber(0),
        Name::new("founder"),
        LivesIn(hamlet),
    ));
    cast.hover(Some(Thing::House(cast.agents[0])));
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
    let messages = walked(&mut cast);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
            files,
            to,
            ..
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
fn a_file_dropped_on_the_founder_begins_a_protocol_gift_without_a_house()
 {
    use hamlet_testing::Scratch;

    let dir = Scratch::new("hand-founder-file");
    let path = dir.join("list.txt");
    std::fs::write(&path, "Dune").unwrap();
    let mut cast = Cast::new(1);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    with_gifts(&mut cast, &runtime);
    let mut state = realm::state();
    state.deeds = realm::deeds(&["give"]);
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
    let messages = walked(&mut cast);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
            to,
            files,
            ..
        }),
    ] = &messages[..]
    else {
        panic!("one manifest: {messages:?}");
    };
    assert_eq!(to.agent, "founder");
    assert_eq!(
        files,
        &[protocol::GiveFile {
            path: "list.txt".into(),
            size: 4
        }]
    );
}
