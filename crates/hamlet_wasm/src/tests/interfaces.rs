use bevy::prelude::*;
use hamlet_sdk::command::Notice;
use hamlet_sdk::plugin::{Disable, Enable, PluginInfo, Status};

use super::interop::said;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};

const IMPORTS: &str = r#"
[[imports.interfaces]]
name = "echo"
[[imports.interfaces.functions]]
name = "say"
params = ["string"]
result = "string"
"#;
const EXPORTS: &str = r#"
ticks = false
[[exports.interfaces]]
name = "echo"
[[exports.interfaces.functions]]
name = "say"
params = ["string"]
result = "string"
"#;

/// `ping` written with postcard as a string.
const PING: &[u8] = b"\x04ping";

fn caller() -> Plugin {
    let module = fixture::invoking("target", "echo/say", PING);
    Plugin::new("caller", fixture::component(&module))
        .saying(IMPORTS)
        .allowed_for(&[("plugin.use", "target")])
}

fn target(module: String) -> Plugin {
    Plugin::new("target", fixture::component(&module))
        .saying(EXPORTS)
        .asking(&["plugin.export"])
        .granted(&["plugin.export"])
}

#[test]
fn a_function_runs_in_the_plugin_that_exports_it_and_its_result_comes_back()
 {
    let scratch = Scratch::new("interfaces_round");
    let mut app = home::app(
        &scratch,
        &[caller(), target(fixture::returning())],
    );

    let lines = said(&mut app, 8);

    let ping = String::from_utf8(PING.to_vec()).unwrap();
    assert!(lines.contains(&("caller".into(), ping)), "{lines:?}");
}

#[derive(Resource, Default)]
struct Told(Vec<String>);

/// Switches the plugin `id` off and on, as the chief does.
fn restart(app: &mut App, id: &str) {
    app.world_mut().trigger(Disable(id.into()));
    app.world_mut().flush();
    app.update();
    app.world_mut().trigger(Enable(id.into()));
    app.world_mut().flush();
}

#[test]
fn a_plugin_a_call_stopped_is_not_called_by_that_caller_until_the_chief_restarts_it()
 {
    let scratch = Scratch::new("interfaces_budget");
    let mut app =
        home::app(&scratch, &[caller(), target(fixture::spin())]);
    home::views(&mut app);
    app.init_resource::<Told>().add_observer(
        |notice: On<Notice>, mut told: ResMut<Told>| {
            told.0.push(notice.0.clone())
        },
    );
    let ran_out = (
        "caller".to_string(),
        "`target` ran out of the call's fuel".to_string(),
    );

    let lines = said(&mut app, 8);

    assert!(lines.contains(&ran_out), "{lines:?}");
    let told = &app.world().resource::<Told>().0;
    assert!(
        told.iter().any(|line| line.starts_with(
            "A call of caller stopped target: a call used up its \
             fuel"
        )),
        "{told:?}"
    );

    // The host starts the target again by itself, a frame being 50
    // milliseconds, and that lifts nothing.
    (0..80).for_each(|_| app.update());
    let mut plugins =
        app.world_mut().query::<(&PluginInfo, &Status)>();
    assert!(plugins.iter(app.world()).any(|(info, status)| {
        info.id == "target" && *status == Status::Running
    }));
    restart(&mut app, "caller");
    let lines = said(&mut app, 8);
    assert!(
        lines.iter().any(|(plugin, line)| plugin == "caller"
            && line.contains("until the chief restarts it")),
        "{lines:?}"
    );

    restart(&mut app, "target");
    restart(&mut app, "caller");
    let lines = said(&mut app, 8);
    assert!(lines.contains(&ran_out), "{lines:?}");
}
