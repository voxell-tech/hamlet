use bevy::prelude::*;
use hamlet_realm_api::overlay::Overlays;
use hamlet_sdk::capability::{Capability, Choice, Decide, Subject};

use super::acts::{does, open, probe};
use super::permissions::petitions;
use crate::fixture::home::Scratch;
use crate::interface::exports::hamlet::plugin::plugin::{
    Anchor, Intent, Mark, MarkShape, Paint,
};
use crate::limits::Limits;

fn marks(count: usize, paint: &str) -> Vec<Intent> {
    let mark = Mark {
        anchor: Anchor::Agent("Juniper".into()),
        x: 0.0,
        y: -20.0,
        shape: MarkShape::Circle(4.0),
        paint: Paint::Palette(paint.into()),
    };
    vec![Intent::Overlay(vec![mark; count])]
}

fn drawn(app: &App) -> Vec<(String, usize)> {
    let overlays = &app.world().resource::<Overlays>().0;
    overlays
        .iter()
        .map(|overlay| (overlay.by.clone(), overlay.marks.len()))
        .collect()
}

#[test]
fn an_overlay_is_kept_within_its_caps_until_it_is_taken_back() {
    let scratch = Scratch::new("overlay_kept");
    let mut app = open(
        &scratch,
        probe()
            .asking(&["world.overlay"])
            .granted(&["world.overlay"]),
    );
    let most = Limits::default().overlay_marks;

    does(&mut app, marks(2, "lamp"));
    assert_eq!(drawn(&app), [("probe".into(), 2)]);

    does(&mut app, marks(most + 1, "lamp"));
    does(&mut app, marks(1, "no-such-colour"));
    assert_eq!(drawn(&app), [("probe".into(), 2)]);

    app.world_mut().trigger(Decide {
        plugin: "probe".into(),
        capability: Capability::WorldOverlay,
        target: None,
        choice: Choice::Deny,
        this_run: false,
    });
    app.update();
    assert!(drawn(&app).is_empty());
}

#[test]
fn an_overlay_waits_for_the_chief_and_none_takes_it_away() {
    let scratch = Scratch::new("overlay_asked");
    let mut app = open(
        &scratch,
        probe()
            .asking(&["world.overlay"])
            .asking_next_time(&["world.overlay"]),
    );

    does(&mut app, marks(1, "lamp"));
    assert!(drawn(&app).is_empty());
    assert!(matches!(
        &petitions(&mut app)[..],
        [(
            _,
            Subject::Use {
                capability: Capability::WorldOverlay,
                ..
            }
        )]
    ));

    let scratch = Scratch::new("overlay_cleared");
    let mut app = open(
        &scratch,
        probe()
            .asking(&["world.overlay"])
            .granted(&["world.overlay"]),
    );
    does(&mut app, marks(1, "lamp"));
    does(&mut app, marks(0, "lamp"));
    assert!(drawn(&app).is_empty());
}
