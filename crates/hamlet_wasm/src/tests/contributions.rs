use bevy::prelude::*;
use bevy_fynix::shortcut::{Chord, Mods};
use hamlet_sdk::capability::{Capability, Choice, Decide, Subject};
use hamlet_sdk::keys::Keys;
use hamlet_sdk::menu::Menus;

use super::permissions::petitions;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};

const EXPORTS: &str = "[exports]\ncommands = [\"go\"]\n";
const BOTH: &[&str] = &["ui.menu", "ui.keys"];

fn far() -> Plugin {
    let module = fixture::contributing();
    Plugin::new("far", fixture::component(&module))
        .saying(EXPORTS)
        .asking(BOTH)
}

fn shown(app: &App) -> (Vec<String>, Vec<String>) {
    let menus = &app.world().resource::<Menus>().0;
    let keys = &app.world().resource::<Keys>().0;
    (
        menus.iter().map(|line| line.command.clone()).collect(),
        keys.iter().map(|bound| bound.command.clone()).collect(),
    )
}

fn decide(app: &mut App, capability: Capability, choice: Choice) {
    app.world_mut().trigger(Decide {
        plugin: "far".into(),
        capability,
        target: None,
        choice,
        this_run: false,
    });
    app.world_mut().flush();
    app.update();
}

#[test]
fn what_a_plugin_declares_shows_only_while_ui_menu_and_ui_keys_are_allowed()
 {
    let scratch = Scratch::new("contributions");
    let mut app = home::app(&scratch, &[far()]);
    app.update();

    // Undecided: nothing shows, and the chief is asked for both.
    assert_eq!(shown(&app), (vec![], vec![]));
    let asked = petitions(&mut app)
        .into_iter()
        .filter_map(|(_, subject)| match subject {
            Subject::Use { capability, .. } => Some(capability),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(asked.contains(&Capability::UiMenu), "{asked:?}");
    assert!(asked.contains(&Capability::UiKeys), "{asked:?}");

    decide(&mut app, Capability::UiMenu, Choice::Allow);
    decide(&mut app, Capability::UiKeys, Choice::Allow);
    let go = vec!["far/go".to_string()];
    assert_eq!(shown(&app), (go.clone(), go));
    let chord = app.world().resource::<Keys>().0[0].chord;
    let mods = Mods::PRIMARY;
    assert_eq!(
        chord,
        Chord {
            key: KeyCode::KeyG,
            mods
        }
    );

    // Revoked, they go at once, and one denied shows nothing.
    decide(&mut app, Capability::UiMenu, Choice::Deny);
    assert_eq!(shown(&app).0, Vec::<String>::new());
    assert_eq!(shown(&app).1.len(), 1);
    decide(&mut app, Capability::UiKeys, Choice::Deny);
    assert_eq!(shown(&app), (vec![], vec![]));
}

#[test]
fn a_plugin_that_was_allowed_before_shows_its_line_at_once() {
    let scratch = Scratch::new("contributions_allowed");
    let mut app = home::app(&scratch, &[far().granted(BOTH)]);
    app.update();
    let go = vec!["far/go".to_string()];
    assert_eq!(shown(&app), (go.clone(), go));
}
