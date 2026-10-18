use bevy::prelude::*;
use bevy_fynix::shortcut::{Chord, Mods};
use hamlet_sdk::capability::{Capability, Choice, Decide, Subject};
use hamlet_sdk::command::kind;
use hamlet_sdk::context::ContextItems;
use hamlet_sdk::keys::Keys;
use hamlet_sdk::menu::Menus;

use super::permissions::petitions;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};

const EXPORTS: &str = "[exports]\ncommands = [\"go\"]\n";
const ALL: &[&str] = &["ui.menu", "ui.keys", "ui.context"];

fn far() -> Plugin {
    let module = fixture::contributing();
    Plugin::new("far", fixture::component(&module))
        .saying(EXPORTS)
        .asking(ALL)
}

/// The commands of the menu lines, the shortcuts and the context
/// menu items that show.
fn shown(app: &App) -> (Vec<String>, Vec<String>, Vec<String>) {
    let menus = &app.world().resource::<Menus>().0;
    let keys = &app.world().resource::<Keys>().0;
    let contexts = &app.world().resource::<ContextItems>().0;
    (
        menus.iter().map(|line| line.command.clone()).collect(),
        keys.iter().map(|bound| bound.command.clone()).collect(),
        contexts.iter().map(|line| line.command.clone()).collect(),
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
fn what_a_plugin_declares_shows_only_while_its_capability_is_allowed()
{
    let scratch = Scratch::new("contributions");
    let mut app = home::app(&scratch, &[far()]);
    app.update();

    // Undecided: nothing shows, and the chief is asked for all three.
    assert_eq!(shown(&app), (vec![], vec![], vec![]));
    let asked = petitions(&mut app)
        .into_iter()
        .filter_map(|(_, subject)| match subject {
            Subject::Use { capability, .. } => Some(capability),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(asked.contains(&Capability::UiMenu), "{asked:?}");
    assert!(asked.contains(&Capability::UiKeys), "{asked:?}");
    assert!(asked.contains(&Capability::UiContext), "{asked:?}");

    decide(&mut app, Capability::UiMenu, Choice::Allow);
    decide(&mut app, Capability::UiKeys, Choice::Allow);
    decide(&mut app, Capability::UiContext, Choice::Allow);
    let go = vec!["far/go".to_string()];
    assert_eq!(shown(&app), (go.clone(), go.clone(), go));
    let chord = app.world().resource::<Keys>().0[0].chord;
    let mods = Mods::PRIMARY;
    assert_eq!(
        chord,
        Chord {
            key: KeyCode::KeyG,
            mods
        }
    );
    let item = &app.world().resource::<ContextItems>().0[0];
    assert_eq!(item.target, kind::AGENT);

    // Revoked, they go at once, and one denied shows nothing.
    decide(&mut app, Capability::UiMenu, Choice::Deny);
    assert_eq!(shown(&app).0, Vec::<String>::new());
    assert_eq!(shown(&app).1.len(), 1);
    decide(&mut app, Capability::UiKeys, Choice::Deny);
    decide(&mut app, Capability::UiContext, Choice::Deny);
    assert_eq!(shown(&app), (vec![], vec![], vec![]));
}

#[test]
fn a_plugin_that_was_allowed_before_shows_its_line_at_once() {
    let scratch = Scratch::new("contributions_allowed");
    let mut app = home::app(&scratch, &[far().granted(ALL)]);
    app.update();
    let go = vec!["far/go".to_string()];
    assert_eq!(shown(&app), (go.clone(), go.clone(), go));
}
