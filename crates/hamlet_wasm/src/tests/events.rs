use bevy::app::App;

use super::tree::{PANEL, items, tree};
use crate::event::{Act, Interaction};
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::trees::{Drawn, PluginTrees};

#[test]
fn a_click_reaches_the_plugin_and_its_answer_changes_the_panel() {
    let scratch = Scratch::new("click");
    let mut app = home::app(
        &scratch,
        &[Plugin {
            id: "probe",
            wasm: fixture::component(&fixture::drawing(
                &items("0 refreshes"),
                &items("1 refresh"),
            )),
        }],
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
