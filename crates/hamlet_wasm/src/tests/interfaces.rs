use hamlet_sdk::plugin::Status;

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

#[test]
fn a_plugin_that_runs_out_of_the_callers_budget_stops_and_the_caller_is_told()
 {
    let scratch = Scratch::new("interfaces_budget");
    let mut app =
        home::app(&scratch, &[caller(), target(fixture::spin())]);

    let lines = said(&mut app, 8);

    assert!(
        lines.contains(&(
            "caller".into(),
            "`target` ran out of the call's fuel".into()
        )),
        "{lines:?}"
    );
    let mut plugins = app
        .world_mut()
        .query::<(&hamlet_sdk::plugin::PluginInfo, &Status)>();
    assert!(plugins.iter(app.world()).any(|(info, status)| {
        info.id == "target" && *status == Status::Failed
    }));
}
