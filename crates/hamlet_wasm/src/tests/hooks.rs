use hamlet_core::session::{Phase, TurnObserved};

use super::interop::said;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};

const HEARS: &str = "[imports]\nevents = [\"host/after-turn\"]\n";

fn watcher(id: &'static str) -> Plugin {
    Plugin::new(id, fixture::component(&fixture::listening()))
        .saying(HEARS)
        .asking(&["agent.hook"])
}

#[test]
fn a_turn_is_told_to_the_plugins_that_may_watch_its_hamlet() {
    let scratch = Scratch::new("hooks");
    let mut app = home::app(
        &scratch,
        &[
            watcher("near")
                .granted(&["agent.hook"])
                .limited_to(&[("agent.hook", &["Home"])]),
            watcher("far")
                .granted(&["agent.hook"])
                .limited_to(&[("agent.hook", &["Elsewhere"])]),
            watcher("unasked"),
        ],
    );
    app.update();

    app.world_mut().trigger(TurnObserved {
        phase: Phase::After,
        agent: "Juniper".into(),
        hamlet: "Home".into(),
        number: 3,
        tools: vec!["bash".into(), "edit".into()],
    });
    let lines = said(&mut app, 3);

    let told = "agent=Juniper\nhamlet=Home\nturn=3\ntools=bash,edit";
    assert_eq!(lines, [("near".to_string(), told.to_string())]);
}
