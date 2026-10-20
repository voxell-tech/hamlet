use hamlet_agents_api::oversight::Choice as Answer;
use hamlet_sdk::capability::Subject;
use hamlet_sdk::plugin::{PluginInfo, Status};

use super::permissions::{answer, petitions};
use super::tree::items;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::reload::Polling;

#[test]
fn a_plugin_folder_that_appears_while_the_app_runs_is_asked_about() {
    let scratch = Scratch::new("appears");
    let mut app = home::app(&scratch, &[]);
    app.world_mut().resource_mut::<Polling>().poll_ms = 0;
    app.update();

    let component = fixture::component(&fixture::drawing(
        &items("a"),
        &items("b"),
    ));
    home::put(
        &scratch,
        &[Plugin::new("probe", component).uninstalled()],
    );
    // Read once it is the same at two looks.
    app.update();
    assert!(petitions(&mut app).is_empty());
    app.update();
    let asked = petitions(&mut app);
    assert!(matches!(
        &asked[..],
        [(_, Subject::Install { plugin })] if plugin == "probe"
    ));

    answer(&mut app, asked[0].0, Answer::Allow);
    let mut plugins =
        app.world_mut().query::<(&PluginInfo, &Status)>();
    let (_, status) = plugins.single(app.world()).unwrap();
    assert_eq!(*status, Status::Running);
}
