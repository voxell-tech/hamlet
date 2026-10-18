use bevy::prelude::*;
use bevy_fynix::shortcut::{Chord, Mods};
use hamlet_sdk::capability::{Capability, Choice, Decide, Subject};
use hamlet_sdk::command::kind;
use hamlet_sdk::context::ContextItems;
use hamlet_sdk::inspector::InspectorSections;
use hamlet_sdk::keys::Keys;
use hamlet_sdk::menu::Menus;
use hamlet_sdk::settings::SettingsPages;

use super::permissions::petitions;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};

const EXPORTS: &str = "[exports]\ncommands = [\"go\"]\n";
const ALL: &[&str] = &[
    "ui.menu",
    "ui.keys",
    "ui.context",
    "ui.inspector",
    "ui.settings",
];
const CAPABILITIES: [Capability; 5] = [
    Capability::UiMenu,
    Capability::UiKeys,
    Capability::UiContext,
    Capability::UiInspector,
    Capability::UiSettings,
];

fn far() -> Plugin {
    let module = fixture::contributing();
    Plugin::new("far", fixture::component(&module))
        .saying(EXPORTS)
        .asking(ALL)
}

/// What shows of what the plugin declared: the commands of its menu
/// lines, shortcuts and context menu items, and the panels of its
/// inspector tabs and settings pages.
#[derive(PartialEq, Default, Debug)]
struct Shown {
    menus: Vec<String>,
    keys: Vec<String>,
    contexts: Vec<String>,
    tabs: Vec<String>,
    pages: Vec<String>,
}

impl Shown {
    fn of(app: &App) -> Self {
        let world = app.world();
        Self {
            menus: world
                .resource::<Menus>()
                .0
                .iter()
                .map(|line| line.command.clone())
                .collect(),
            keys: world
                .resource::<Keys>()
                .0
                .iter()
                .map(|bound| bound.command.clone())
                .collect(),
            contexts: world
                .resource::<ContextItems>()
                .0
                .iter()
                .map(|line| line.command.clone())
                .collect(),
            tabs: world
                .resource::<InspectorSections>()
                .0
                .iter()
                .map(|line| line.panel.clone())
                .collect(),
            pages: world
                .resource::<SettingsPages>()
                .0
                .iter()
                .map(|line| line.panel.clone())
                .collect(),
        }
    }

    /// Everything the plugin declared, shown.
    fn all() -> Self {
        let one = |name: &str| vec![name.to_string()];
        Self {
            menus: one("far/go"),
            keys: one("far/go"),
            contexts: one("far/go"),
            tabs: one("far/go"),
            pages: one("far/go"),
        }
    }
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

    // Undecided: nothing shows, and the chief is asked for each.
    assert_eq!(Shown::of(&app), Shown::default());
    let asked = petitions(&mut app)
        .into_iter()
        .filter_map(|(_, subject)| match subject {
            Subject::Use { capability, .. } => Some(capability),
            _ => None,
        })
        .collect::<Vec<_>>();
    for capability in CAPABILITIES {
        assert!(asked.contains(&capability), "{asked:?}");
    }

    for capability in CAPABILITIES {
        decide(&mut app, capability, Choice::Allow);
    }
    assert_eq!(Shown::of(&app), Shown::all());
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
    let tab = &app.world().resource::<InspectorSections>().0[0];
    assert_eq!(tab.title, "Go");

    // Revoked, they go at once, and one denied shows nothing.
    decide(&mut app, Capability::UiMenu, Choice::Deny);
    assert_eq!(Shown::of(&app).menus, Vec::<String>::new());
    assert_eq!(Shown::of(&app).keys.len(), 1);
    for capability in &CAPABILITIES[1..] {
        decide(&mut app, *capability, Choice::Deny);
    }
    assert_eq!(Shown::of(&app), Shown::default());
}

#[test]
fn a_plugin_that_was_allowed_before_shows_its_line_at_once() {
    let scratch = Scratch::new("contributions_allowed");
    let mut app = home::app(&scratch, &[far().granted(ALL)]);
    app.update();
    assert_eq!(Shown::of(&app), Shown::all());
}
