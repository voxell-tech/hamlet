use bevy::camera::NormalizedRenderTarget;
use bevy::camera::primitives::Aabb;
use bevy::picking::backend::HitData;
use bevy::picking::backend::ray::{RayId, RayMap};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{Location, PointerId};

use super::camera::drag;
use super::*;
use crate::WorldCamera;
use crate::sky::CameraSettings;

#[test]
fn a_house_does_not_keep_the_pointer_from_the_agent_behind_it() {
    let mut app = App::new();
    app.init_resource::<HoverMap>()
        .init_resource::<RayMap>()
        .init_resource::<Hovered>()
        .add_systems(Update, hover);
    let world = app.world_mut();
    let agent = world.spawn_empty().id();
    let mut stand = |thing: Thing, z: f32| {
        let size = Vec3::ONE;
        world
            .spawn((
                thing,
                Aabb::from_min_max(-size / 2.0, size / 2.0),
                GlobalTransform::from_xyz(0.0, 0.0, z),
                Visibility::Hidden,
                Pickable::default(),
            ))
            .id()
    };
    let house = stand(Thing::ResidentHouse(agent), 0.0);
    let behind = stand(Thing::Resident(agent), -4.0);
    let surface = world.spawn(WorldSurface).id();
    let camera = world.spawn(WorldCamera).id();
    let hit = HitData::new(camera, 0.0, None, None);
    // As in the app: the mouse is on the surface, and the pointer
    // the panel forwards is on the nearest box alone.
    let forwarded = PointerId::Touch(1);
    let mut over = world.resource_mut::<HoverMap>();
    over.insert(
        PointerId::Mouse,
        [(surface, hit.clone())].into_iter().collect(),
    );
    over.insert(forwarded, [(house, hit)].into_iter().collect());
    world.resource_mut::<RayMap>().map.insert(
        RayId::new(camera, forwarded),
        Ray3d::new(Vec3::Z * 10.0, Dir3::NEG_Z),
    );
    app.update();
    assert_eq!(
        app.world().resource::<Hovered>().0,
        Some(Thing::Resident(agent))
    );

    // An agent held in the hand is skipped.
    app.world_mut().entity_mut(behind).remove::<Pickable>();
    app.update();
    assert_eq!(
        app.world().resource::<Hovered>().0,
        Some(Thing::ResidentHouse(agent))
    );

    // Nothing is hovered off the world.
    app.world_mut().resource_mut::<HoverMap>().clear();
    app.update();
    assert_eq!(app.world().resource::<Hovered>().0, None);
}

#[test]
fn the_view_stays_within_reach_of_something_to_look_at() {
    let camera = CameraSettings::default();
    let centre = Vec2::new(100.0, 0.0);
    // With nothing to hold on to, the middle is what counts.
    let free = Leash::default();
    let far = free.held(&camera, centre, Vec2::X * 9000.0);
    assert_eq!(far, Vec2::X * camera.pan);
    // With towns, it is the nearest of them, however far from
    // the middle it lies.
    let towns = Leash {
        zoom: None,
        anchors: vec![Vec2::ZERO, Vec2::new(5000.0, 300.0)],
    };
    let by = Vec2::new(4900.0, 280.0) - centre;
    assert_eq!(towns.held(&camera, centre, by), by);
    let past = Vec2::new(5000.0, 300.0 + 2.0 * camera.pan) - centre;
    let held = towns.held(&camera, centre, past);
    let stands = centre + held;
    assert!(
        (stands.distance(towns.anchors[1]) - camera.pan).abs() < 0.01
    );
    // The way from one town to another is open all along, and
    // so far to its side.
    let between = Vec2::new(2500.0, 150.0) - centre;
    assert_eq!(towns.held(&camera, centre, between), between);
    let aside = Vec2::new(2500.0, 150.0 + 3.0 * camera.pan);
    let held = centre + towns.held(&camera, centre, aside - centre);
    assert!(held.distance(aside) > camera.pan);
    assert!(
        (held.distance(Vec2::new(2500.0, 150.0)) - camera.pan).abs()
            < 40.0
    );
}

#[test]
fn a_drag_slides_the_ground_unless_it_was_taken() {
    let mut app = App::new();
    app.init_resource::<Orbit>()
        .init_resource::<Travel>()
        .init_resource::<Taken>()
        .init_resource::<Stance>()
        .init_resource::<Following>()
        .init_resource::<Leash>()
        .init_resource::<Centre>()
        .init_resource::<CameraSettings>()
        .init_resource::<Steer>()
        .add_observer(drag);
    let surface = app
        .world_mut()
        .spawn((WorldSurface, ComputedNode::default()))
        .id();
    let pull = |app: &mut App| {
        let at = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        let event = Drag {
            button: PointerButton::Primary,
            distance: Vec2::new(20.0, 0.0),
            delta: Vec2::new(20.0, 0.0),
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            at,
            event,
            surface,
        ));
        app.world().resource::<Orbit>().pan
    };
    let slid = pull(&mut app);
    assert_ne!(slid, Vec2::ZERO);
    assert_eq!(*app.world().resource::<Steer>(), Steer::Slide);

    // Something else holds the drag: the ground stays, and the
    // pointer's travel still counts against a click.
    app.world_mut().resource_mut::<Taken>().drag = true;
    assert_eq!(pull(&mut app), slid);
    assert_eq!(app.world().resource::<Travel>().0, 40.0);
}

#[test]
fn a_click_switches_clockwork_and_chooses_the_hamlet_of_a_bench() {
    #[derive(Resource, Default)]
    struct Switches(Vec<Entity>);

    let mut app = App::new();
    app.init_resource::<Hovered>()
        .init_resource::<Travel>()
        .init_resource::<Taken>()
        .init_resource::<CameraSettings>()
        .init_resource::<Selected>()
        .init_resource::<Also>()
        .init_resource::<Asked>()
        .init_resource::<Switches>()
        .add_observer(click)
        .add_observer(
            |switched: On<Switched>, mut all: ResMut<Switches>| {
                all.0.push(switched.hamlet);
            },
        );
    let world = app.world_mut();
    let hamlet = world.spawn_empty().id();
    let train = world.spawn(Thing::Clockwork(hamlet)).id();
    let bench = world.spawn(Thing::Workbench(hamlet)).id();
    let mut press = |on: Entity| {
        let at = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        let event = Click {
            button: PointerButton::Primary,
            hit: HitData::new(on, 0.0, None, None),
            duration: std::time::Duration::ZERO,
            count: 1,
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            at,
            event,
            on,
        ));
        app.world_mut().flush();
    };
    press(train);
    press(bench);
    assert_eq!(app.world().resource::<Switches>().0, [hamlet]);
    assert_eq!(
        app.world().resource::<Selected>().0,
        Some(Thing::Common(hamlet))
    );
}
