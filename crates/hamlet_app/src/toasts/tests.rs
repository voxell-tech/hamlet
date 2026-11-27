use std::time::Duration;

use bevy::time::{TimeUpdateStrategy, Virtual};
use hamlet_client::outbox::Outbox;
use hamlet_client::waits::Choice;
use hamlet_protocol::client as protocol;

use super::*;
use crate::testing::{panel_app, press, words};

fn app_with_a_petition() -> App {
    let mut app = panel_app();
    app.add_plugins(plugin);
    // What waits at the first state is not new.
    let mut state = hamlet_testing::realm::state();
    state.waits.push(hamlet_testing::realm::wait("old"));
    hamlet_client::receive(
        app.world_mut(),
        ServerMessage::State(state.clone()),
    );
    app.update();
    state.revision += 1;
    state.waits.push(hamlet_testing::realm::wait("new"));
    hamlet_client::receive(
        app.world_mut(),
        ServerMessage::State(state),
    );
    app.update();
    bevy_fynix::mount::<HamletTheme>(app.world_mut(), stack());
    app.update();
    app
}

#[test]
fn a_new_petition_toasts_and_is_answered_in_place() {
    let mut app = app_with_a_petition();
    assert_eq!(app.world().resource::<Toasts>().shown.len(), 1);
    let shown = words(&mut app);
    assert!(shown.iter().any(|text| text == "ada"), "{shown:?}");
    // The command on a chip of its own, with no backticks.
    assert!(shown.iter().any(|text| text == "wants to run"));
    assert!(shown.iter().any(|text| text == "pwd"));
    press(&mut app, "Allow once");
    let allowed =
        app.world().resource::<Outbox>().iter().any(|message| {
            matches!(message, ClientMessage::Answer(answer)
                if answer.wait == "new" && answer.choice == Choice::Allow)
        });
    assert!(allowed);
    assert!(app.world().resource::<Toasts>().shown.is_empty());
}

#[test]
fn the_hands_verdict_toasts_with_a_way_to_undo_it() {
    let mut app = panel_app();
    app.add_plugins(plugin);
    let mut state = hamlet_testing::realm::state();
    state.settings.newcomers = protocol::Newcomers::Hand;
    let mut moss = hamlet_testing::realm::profile(3, "moss", &[]);
    moss.arrival = Arrival::Waiting;
    moss.house = None;
    state.profiles.push(moss);
    hamlet_client::receive(
        app.world_mut(),
        ServerMessage::State(state.clone()),
    );
    app.update();
    state.revision += 1;
    let moss = state.profiles.last_mut().unwrap();
    moss.arrival = Arrival::Housed;
    moss.arrival_by = Some(HAND.into());
    hamlet_client::receive(
        app.world_mut(),
        ServerMessage::State(state),
    );
    app.update();
    bevy_fynix::mount::<HamletTheme>(app.world_mut(), stack());
    app.update();
    let shown = words(&mut app);
    assert!(
        shown.iter().any(|text| text == "The Hand let a profile in"),
        "{shown:?}"
    );
    press(&mut app, "Undo");
    let undone =
        app.world().resource::<Outbox>().iter().any(|message| {
            matches!(message, ClientMessage::Say(say)
                if say.text == "/turn-away profile#3")
        });
    assert!(undone);
}

#[test]
fn a_toast_stays_under_the_pointer_and_fades_otherwise() {
    let mut app = app_with_a_petition();
    let step = Duration::from_secs_f32(STAYS + 2.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(step));
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(step);
    let pile = pile(&mut app);
    app.world_mut().entity_mut(pile).insert(Hovered(true));
    app.update();
    assert_eq!(app.world().resource::<Toasts>().shown.len(), 1);
    app.world_mut().entity_mut(pile).insert(Hovered(false));
    app.update();
    assert!(app.world().resource::<Toasts>().shown.is_empty());
}

/// The stack's node.
fn pile(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Pile>>()
        .single(world)
        .unwrap()
}

/// Toasts with `count` faults, the newest last.
fn faults(count: usize) -> Toasts {
    let mut toasts = Toasts::default();
    for n in 0..count {
        toasts.push(
            What::Notice {
                kind: StatusKind::Danger,
                head: "Fault".into(),
                line: n.to_string(),
            },
            0.0,
        );
    }
    toasts
}

/// The window with `toasts` stacked, a frame `step` long.
fn stacked(toasts: Toasts, step: Duration) -> App {
    let mut app = panel_app();
    app.add_plugins(plugin);
    app.insert_resource(toasts);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(step));
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(step);
    bevy_fynix::mount::<HamletTheme>(app.world_mut(), stack());
    app.update();
    app
}

/// Each toast's fold, oldest first.
fn folds(app: &App) -> Vec<f32> {
    let toasts = app.world().resource::<Toasts>();
    toasts.shown.iter().map(|toast| toast.pose.fold).collect()
}

#[test]
fn older_toasts_fold_behind_the_newest_until_the_stack_opens() {
    let mut toasts = faults(4);
    let places = |toasts: &Toasts| -> Vec<Place> {
        toasts
            .shown
            .iter()
            .map(|toast| toasts.place(toast.id))
            .collect()
    };
    assert_eq!(
        places(&toasts),
        [
            Place::Hidden,
            Place::Folded(2),
            Place::Folded(1),
            Place::Whole
        ]
    );
    toasts.open = true;
    assert_eq!(places(&toasts), [Place::Whole; 4]);
}

#[test]
fn toasts_ease_into_their_folds_and_out_of_them() {
    let mut app = stacked(faults(2), Duration::from_millis(16));
    app.update();
    let [older, newest] = folds(&app)[..] else {
        panic!("two toasts");
    };
    // On its way, not there at once.
    assert!(0.0 < older && older < 1.0, "{older}");
    assert_eq!(newest, 0.0);
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(folds(&app), [1.0, 0.0]);
    let pile = pile(&mut app);
    app.world_mut().entity_mut(pile).insert(Hovered(true));
    app.update();
    app.update();
    assert!(folds(&app)[0] < 1.0);
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(folds(&app), [0.0, 0.0]);
}

#[test]
fn with_less_motion_toasts_fold_at_once() {
    let mut app = stacked(faults(2), Duration::from_millis(16));
    app.insert_resource(ReducedMotion(true));
    app.update();
    assert_eq!(folds(&app), [1.0, 0.0]);
}

#[test]
fn an_open_stack_closes_all_its_toasts_at_once() {
    let mut app = stacked(faults(2), Duration::from_millis(16));
    let pile = pile(&mut app);
    app.world_mut().entity_mut(pile).insert(Hovered(true));
    app.update();
    assert!(app.world().resource::<Toasts>().open);
    press(&mut app, "Close all");
    assert!(app.world().resource::<Toasts>().shown.is_empty());
}

#[test]
fn an_update_stays_on_show_until_it_is_taken_or_closed() {
    let mut app = panel_app();
    app.add_plugins(plugin).init_resource::<updates::Updates>();
    let step = Duration::from_secs_f32(STAYS + FADES + 1.0);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(step));
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(step);
    let offer = Offer {
        version: hamlet_protocol::Version::new(0, 2, 0),
        ready: Ready::Page("https://example.com".into()),
    };
    let mut toasts = app.world_mut().resource_mut::<Toasts>();
    toasts.offer(offer.clone(), 0.0);
    toasts.offer(offer.clone(), 0.0);
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Toasts>().shown.len(), 1);
    let id = app.world().resource::<Toasts>().shown[0].id;
    dismiss(app.world_mut(), id);
    assert!(app.world().resource::<Toasts>().shown.is_empty());
    assert_eq!(
        app.world().resource::<updates::Updates>().declined,
        Some(offer.version)
    );
}

#[test]
fn a_page_that_does_not_open_gives_its_address() {
    let mut app = panel_app();
    app.add_plugins(plugin);
    app.world_mut().remove_resource::<Clipboard>();
    unopened(app.world_mut(), "https://example.com/v0.2.0");
    let toasts = app.world().resource::<Toasts>();
    assert!(matches!(
        &toasts.shown[..],
        [Toast { what: What::Notice { head, line, .. }, .. }]
            if head == "The page did not open"
                && line == "Open https://example.com/v0.2.0 in a browser."
    ));
}

#[test]
fn newer_toasts_make_way_around_an_update() {
    let offer = Offer {
        version: hamlet_protocol::Version::new(0, 2, 0),
        ready: Ready::Page("https://example.com".into()),
    };
    let mut toasts = Toasts::default();
    toasts.offer(offer.clone(), 0.0);
    for n in 0..MOST {
        toasts.note(
            StatusKind::Danger,
            "Fault".into(),
            n.to_string(),
            0.0,
        );
    }
    assert_eq!(toasts.shown.len(), MOST);
    assert_eq!(toasts.shown[0].what, What::Update(offer));
    assert!(matches!(
        &toasts.shown[1].what,
        What::Notice { line, .. } if line == "1"
    ));
}

#[test]
fn a_command_from_the_world_that_is_refused_toasts() {
    let mut app = panel_app();
    app.add_plugins(plugin).add_observer(chat::decreed);
    app.world_mut().trigger(hamlet_realm::orders::Decree {
        hamlet: None,
        text: "/move-house ada 1 1".into(),
    });
    app.world_mut().flush();
    let said = app
        .world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .filter_map(|message| match message {
            ClientMessage::Say(say) => Some(say),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [say] = &said[..] else {
        panic!("one command: {said:?}");
    };
    let refusal = serde_json::from_value(serde_json::json!({
        "type": "reply",
        "to": say.id,
        "kind": "error",
        "text": "the house would stand on another",
    }))
    .unwrap();
    hamlet_client::receive(app.world_mut(), refusal);
    let shown = &app.world().resource::<Toasts>().shown;
    assert!(matches!(
        &shown[..],
        [Toast { what: What::Notice { line, .. }, .. }]
            if line == "the house would stand on another"
    ));
}
