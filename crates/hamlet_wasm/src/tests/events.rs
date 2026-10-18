use bevy::app::App;
use hamlet_sdk::tree::event::{Act, Interaction};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};

use super::tree::{PANEL, items, tree};
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};

#[test]
fn a_click_reaches_the_plugin_and_its_answer_changes_the_panel() {
    let scratch = Scratch::new("click");
    let mut app = home::app(
        &scratch,
        &[Plugin::new(
            "probe",
            fixture::component(&fixture::drawing(
                &items("0 refreshes"),
                &items("1 refresh"),
            )),
        )],
    );
    app.update();
    let shows = |app: &App, count: &str| {
        app.world()
            .resource::<PluginTrees>()
            .shows(PANEL, &Drawn::Tree(tree(count, false)))
    };
    assert!(shows(&app, "0 refreshes"));

    Interaction::send(app.world_mut(), PANEL, "go", Act::Click);
    app.update();

    assert!(shows(&app, "1 refresh"));
}
