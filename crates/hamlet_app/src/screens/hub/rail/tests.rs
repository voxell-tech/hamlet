use hamlet_client::realm::{Busy, named};
use hamlet_protocol::client as protocol;

use super::*;
use crate::testing::panel_app;

fn names(app: &App, group: fn(&Group) -> bool) -> Vec<String> {
    sections(app.world())
        .into_iter()
        .filter(|section| group(&section.group))
        .flat_map(|section| section.lines)
        .map(|line| line.name)
        .collect()
}

#[test]
fn a_hire_keeps_its_place_in_its_hamlet_and_what_waits_is_on_top() {
    let mut app = panel_app();
    app.add_plugins(plugin);
    app.update();
    let ada = named(app.world_mut(), "hire#2").unwrap();
    let in_hamlet = |g: &Group| matches!(g, Group::Hamlet(_));
    assert_eq!(names(&app, in_hamlet), ["ada"]);
    assert!(
        names(&app, |g| *g == Group::Top)
            .contains(&"The Hand".into())
    );

    // Working does not move it.
    app.world_mut().entity_mut(ada).insert(Busy);
    assert_eq!(names(&app, in_hamlet), ["ada"]);

    let mut state = hamlet_testing::realm::state();
    state.waits.push(hamlet_testing::realm::wait("ask"));
    state.waits.push(hamlet_testing::realm::wait("again"));
    state.revision += 1;
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::State(state),
    );
    app.update();
    assert_eq!(names(&app, |g| *g == Group::Needs), ["ada"]);
    // One row for who asks, counted as every wait, as the
    // realm's page and the status bar count them.
    let needs = sections(app.world())
        .into_iter()
        .find(|section| section.group == Group::Needs)
        .unwrap();
    assert_eq!(needs.count, 2);
    assert_eq!(names(&app, in_hamlet), ["ada"]);
}
