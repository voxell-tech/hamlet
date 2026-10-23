use bevy::app::{App, PostUpdate};
use bevy::ecs::hierarchy::ChildOf;
use bevy::picking::backend::HitData;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::window::Window;
use bevy_fynix::Theme;
use hamlet_assets::Tuning;

use super::*;
use crate::theme::HamletTheme;

/// The alpha of every pixel of a picture.
fn alphas(image: &Image) -> Vec<u8> {
    let data = image.data.as_ref().unwrap();
    data.chunks(4).map(|pixel| pixel[3]).collect::<Vec<_>>()
}

fn pictures() -> Vec<(Mode, usize, Image)> {
    let inks = inks(&HamletTheme::designed());
    Mode::all()
        .into_iter()
        .flat_map(|mode| {
            (0..frames(mode)).map(move |frame| {
                (mode, frame, face(mode, frame, &inks))
            })
        })
        .collect::<Vec<_>>()
}

#[test]
fn modes_differ_in_shape_and_not_only_in_colour() {
    let shapes = pictures()
        .into_iter()
        .map(|(mode, _, image)| {
            let solid = alphas(&image)
                .into_iter()
                .map(|alpha| alpha > 127)
                .collect::<Vec<_>>();
            (mode, solid)
        })
        .collect::<Vec<_>>();
    for (i, (mode, solid)) in shapes.iter().enumerate() {
        for (other, theirs) in &shapes[i + 1..] {
            if mode == other {
                continue;
            }
            let apart = solid
                .iter()
                .zip(theirs)
                .filter(|(a, b)| a != b)
                .count();
            assert!(apart > 60, "{mode:?} and {other:?}: {apart}");
        }
    }
}

#[test]
fn the_hot_spot_is_on_the_picture() {
    for (mode, frame, image) in pictures() {
        let (x, y) = hot(mode);
        let alpha = alphas(&image)[y as usize * side() + x as usize];
        assert_eq!(alpha, 255, "{mode:?} {frame}");
    }
}

#[test]
fn no_picture_is_cut_by_the_edge_of_its_square() {
    for (mode, frame, image) in pictures() {
        let alphas = alphas(&image);
        let last = side() - 1;
        for i in 0..side() {
            for at in
                [i, last * side() + i, i * side(), i * side() + last]
            {
                assert!(alphas[at] < 40, "{mode:?} {frame}");
            }
        }
    }
}

#[test]
fn each_fact_gives_way_to_the_ones_before_it() {
    let agent = Thing::Resident(Entity::PLACEHOLDER);
    let panel = Facts {
        asked: Some(SystemCursorIcon::Pointer),
        over: Some(agent),
        grip: Some(Grip::Ready),
        claimed: true,
        ..Facts::default()
    };
    assert_eq!(Mode::choose(&panel), Mode::Press);
    let sliding = Facts {
        steer: Steer::Slide,
        ..panel
    };
    assert_eq!(Mode::choose(&sliding), Mode::Slide);
    let turning = Facts {
        steer: Steer::Turn,
        ..panel
    };
    assert_eq!(Mode::choose(&turning), Mode::Turn);
    let carrying = Facts {
        grip: Some(Grip::Robot),
        ..sliding
    };
    assert_eq!(Mode::choose(&carrying), Mode::Carry);
    let busy = Facts {
        busy: true,
        ..carrying
    };
    assert_eq!(Mode::choose(&busy), Mode::Wait);
    // With none of those, the world decides.
    let bare = Facts::default();
    assert_eq!(Mode::choose(&bare), Mode::Point);
    let asked_plain = Facts {
        asked: Some(SystemCursorIcon::Default),
        ..bare
    };
    assert_eq!(Mode::choose(&asked_plain), Mode::Point);
    let over = Facts {
        over: Some(Thing::Library),
        ..bare
    };
    assert_eq!(Mode::choose(&over), Mode::Pick);
    let claimed = Facts {
        claimed: true,
        ..over
    };
    assert_eq!(Mode::choose(&claimed), Mode::Press);
    let liftable = Facts {
        over: Some(agent),
        grip: Some(Grip::Ready),
        ..bare
    };
    assert_eq!(Mode::choose(&liftable), Mode::Grab);
}

/// An app with a window and the cursors, and nothing of the
/// world.
fn app() -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<Assets<Image>>()
        .insert_resource(Theme(HamletTheme::designed()))
        .init_resource::<Time<Real>>()
        .add_systems(PostUpdate, dress);
    draw(app.world_mut());
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    (app, window)
}

fn shown(app: &App, window: Entity) -> (Handle<Image>, (u16, u16)) {
    match app.world().get::<CursorIcon>(window) {
        Some(CursorIcon::Custom(CustomCursor::Image(image))) => {
            (image.handle.clone(), image.hotspot)
        }
        other => {
            panic!("the system's cursor is shown: {other:?}")
        }
    }
}

fn picture(app: &App, mode: Mode) -> Handle<Image> {
    app.world()
        .resource::<Cursors>()
        .at(mode, 0.0, false)
        .unwrap()
        .clone()
}

#[test]
fn a_cursor_that_holds_is_not_set_again() {
    let (mut app, window) = app();
    let set = |app: &App| {
        app.world()
            .entity(window)
            .get_change_ticks::<CursorIcon>()
            .unwrap()
            .changed
    };
    app.update();
    let first = set(&app);
    app.update();
    assert_eq!(set(&app), first);
}

#[test]
fn the_world_shows_through_the_cursor() {
    let (mut app, window) = app();
    app.insert_resource(Hovered(Some(Thing::Library)))
        .init_resource::<Steer>();
    app.update();
    assert_eq!(shown(&app, window).0, picture(&app, Mode::Pick));

    // A drag is only believed while its button is down.
    app.insert_resource(Steer::Turn)
        .init_resource::<ButtonInput<MouseButton>>();
    app.update();
    assert_eq!(shown(&app, window).0, picture(&app, Mode::Pick));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    app.update();
    assert_eq!(shown(&app, window).0, picture(&app, Mode::Turn));
}

#[test]
fn a_node_over_the_world_that_takes_a_click_shows_it() {
    let (mut app, window) = app();
    app.add_plugins(bevy_fynix::CursorPlugin)
        .init_resource::<HoverMap>()
        .add_observer(lend);
    let tag = app.world_mut().spawn(Pressable).id();
    let check = app.world_mut().spawn(ChildOf(tag)).id();
    // The mouse is on the check, which hides the world's surface
    // from it, so nothing of the world is hovered.
    let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
    app.world_mut()
        .resource_mut::<HoverMap>()
        .entry(PointerId::Mouse)
        .or_default()
        .insert(check, hit);
    app.update();
    assert_eq!(shown(&app, window).0, picture(&app, Mode::Press));
}
