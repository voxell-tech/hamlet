use std::time::Duration;

use bevy::picking::backend::HitData;
use bevy::picking::pointer::PointerId;
use bevy::time::TimeUpdateStrategy;

use super::*;
use crate::generator::society::AgentId;
use crate::generator::testing::{rehearse, stand_in};
use crate::pick::PickSettings;
use crate::play::Played;
use crate::sky::CameraSettings;
use crate::{Clock, bubble};

/// The files the hand told of.
#[derive(Resource, Default)]
struct Log {
    files: Vec<PathBuf>,
}

fn collect(
    mut files: MessageReader<FileDropped>,
    mut log: ResMut<Log>,
) {
    log.files
        .extend(files.read().map(|dropped| dropped.path.clone()));
}

/// A world with a robot at its porch, its agent and the box a pointer
/// hits, and nothing drawn.
struct Cast {
    app: App,
    agent: Entity,
}

impl Cast {
    fn new() -> Self {
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
        .add_systems(PostUpdate, collect)
        .add_plugins(plugin);
        let world = app.world_mut();
        let agent = world.spawn(Housed(AgentId(1))).id();
        let robot = stand_in(world, AgentId(1), 1);
        world.spawn((
            Thing::Agent(agent),
            ChildOf(robot),
            Pickable::default(),
            Transform::default(),
        ));
        let mut cast = Cast { app, agent };
        cast.run(0.5);
        cast
    }

    fn run(&mut self, secs: f32) {
        for _ in 0..(secs * 60.0).round() as usize {
            self.app.world_mut().resource_mut::<Clock>().t +=
                1.0 / 60.0;
            self.app.update();
        }
    }

    fn drop_file(&mut self, path: &str) {
        self.app.world_mut().write_message(
            FileDragAndDrop::DroppedFile {
                window: Entity::PLACEHOLDER,
                path_buf: PathBuf::from(path),
            },
        );
    }

    fn told(&mut self) -> Vec<PathBuf> {
        let mut log = self.app.world_mut().resource_mut::<Log>();
        std::mem::take(&mut log.files)
    }
}

#[test]
fn a_file_goes_where_the_pointer_is_first_seen() {
    let mut cast = Cast::new();
    let panel = cast.app.world_mut().spawn(FileDrop).id();
    let field = cast.app.world_mut().spawn(ChildOf(panel)).id();

    // On a chat every file is its own, whatever it is.
    cast.app.world_mut().resource_mut::<HoverMap>().insert(
        PointerId::Mouse,
        [(field, HitData::new(Entity::PLACEHOLDER, 0.0, None, None))]
            .into(),
    );
    cast.drop_file("/desk/pear.PNG");
    cast.drop_file("/desk/list.txt");
    cast.run(0.3);
    assert_eq!(
        cast.told(),
        [
            PathBuf::from("/desk/pear.PNG"),
            PathBuf::from("/desk/list.txt")
        ]
    );

    // One that fell on a house, or on nothing, is nobody's.
    cast.app.world_mut().resource_mut::<HoverMap>().clear();
    cast.app.world_mut().resource_mut::<Hovered>().0 =
        Some(Thing::House(cast.agent));
    cast.drop_file("/desk/other.txt");
    let wait = cast.app.world().resource::<PlaySettings>().drop_wait;
    cast.run(wait + 0.5);
    assert!(cast.told().is_empty());
}
