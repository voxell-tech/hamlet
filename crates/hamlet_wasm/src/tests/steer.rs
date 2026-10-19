use hamlet_agents_api::tools::{Steered, Steering};
use hamlet_sdk::plugin::{PluginInfo, Status};

use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::limits::Limits;

fn steerer(id: &'static str, step: String) -> Plugin {
    Plugin::new(id, fixture::component(&step))
        .saying("ticks = false\n")
        .asking(&["agent.steer"])
        .granted(&["agent.steer"])
}

#[test]
fn a_plugin_that_runs_out_while_steering_changes_nothing_and_stops() {
    let scratch = Scratch::new("steer_spin");
    let mut app = home::app(
        &scratch,
        &[
            steerer("quiet", fixture::quiet()),
            steerer("spin", fixture::spin()),
            steerer("far", fixture::quiet())
                .limited_to(&[("agent.steer", &["Elsewhere"])]),
        ],
    );
    app.insert_resource(Limits {
        steer_fuel: 100_000,
        ..Limits::default().patient()
    });
    app.update();

    let mut point = Steering::at(
        "Juniper",
        "Home",
        1,
        Some(("bash".into(), "{}".into())),
    );
    app.world_mut().trigger_ref(&mut point);
    app.update();

    assert!(point.watched);
    assert!(
        matches!(
        &point.steers[..],
        [steer] if steer.plugin == "spin"
            && matches!(steer.steered, Steered::Failed(_))
        ),
        "{:?}",
        point.steers
    );
    let stopped = app
        .world_mut()
        .query::<(&PluginInfo, &Status)>()
        .iter(app.world())
        .filter(|(_, status)| **status == Status::Failed)
        .map(|(info, _)| info.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(stopped, ["spin"]);
}
