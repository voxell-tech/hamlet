//! What a plugin adds to the window besides panels: its commands,
//! its menu lines and its shortcuts, as lines under its name.

use bevy::ecs::entity::Entity;
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::system::{Query, Res, SystemParam};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use hamlet_sdk::capability::{Capability, Granted, allowed};
use hamlet_sdk::command::{Commands, full};
use hamlet_sdk::keys::{Key, Keys, Standing};
use hamlet_sdk::menu::{Menu, MenuEntry};
use hamlet_sdk::plugin::{Contributions, PluginInfo};

use crate::AnyView;
use crate::looks::note;

/// What the lines are read from.
#[derive(SystemParam)]
pub(super) struct Contributed<'w, 's> {
    held: Query<'w, 's, &'static Contributions>,
    menus: Query<'w, 's, &'static MenuEntry>,
    keys: Query<'w, 's, &'static Key>,
    commands: Res<'w, Commands>,
    bound: Res<'w, Keys>,
}

/// A shortcut, and whether it runs.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Chorded {
    pub words: String,
    pub flag: Option<String>,
}

/// What one plugin adds.
#[derive(Clone, PartialEq, Default, Debug)]
pub(super) struct Adds {
    pub commands: Vec<String>,
    pub menus: Vec<String>,
    pub keys: Vec<Chorded>,
}

/// What the plugin `plugin` adds. A line or a key that shows only
/// while a capability is allowed says it waits for it.
pub(super) fn of(
    plugin: Entity,
    info: &PluginInfo,
    granted: Option<&Granted>,
    from: &Contributed,
) -> Adds {
    let mine = from
        .held
        .get(plugin)
        .map(|held| held.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    let may = |capability| allowed(info, granted, capability);
    let titled = |command: &str| {
        let id = full(&info.id, command);
        from.commands
            .0
            .iter()
            .find(|listed| listed.id == id)
            .map_or_else(|| command.to_string(), |l| l.title.clone())
    };
    let mut commands = from
        .commands
        .0
        .iter()
        .filter(|listed| listed.owner == info.id)
        .map(|listed| listed.title.clone())
        .collect::<Vec<_>>();
    commands.sort();
    let mut menus = from
        .menus
        .iter_many(&mine)
        .map(|entry| {
            let held = match may(Capability::UiMenu) {
                true => "",
                false => " (waits for ui.menu)",
            };
            format!(
                "{} > {}{held}",
                menu_name(entry.menu),
                entry.label
            )
        })
        .collect::<Vec<_>>();
    menus.sort();
    let keys = from
        .keys
        .iter_many(&mine)
        .map(|key| {
            let words = format!(
                "{} runs {}",
                key.chord,
                titled(&key.command)
            );
            let standing = from
                .bound
                .0
                .iter()
                .find(|bound| {
                    bound.plugin == info.id
                        && bound.chord == key.chord
                })
                .map(|bound| bound.standing);
            let flag = match (may(Capability::UiKeys), standing) {
                (false, _) => {
                    Some("It waits for ui.keys".to_string())
                }
                (_, Some(Standing::Clash)) => Some(
                    "Another plugin has these keys, so neither runs"
                        .into(),
                ),
                (_, Some(Standing::Shadowed)) => Some(
                    "Hamlet uses these keys, so this one does not \
                     run"
                    .into(),
                ),
                _ => None,
            };
            Chorded { words, flag }
        })
        .collect();
    Adds {
        commands,
        menus,
        keys,
    }
}

fn menu_name(menu: Menu) -> &'static str {
    match menu {
        Menu::App => "HAMLET",
        Menu::View => "VIEW",
    }
}

/// The lines for `adds`, none where it adds nothing. A shortcut that
/// does not run has a line of its own in the colour of an error.
pub(super) fn lines(adds: &Adds) -> Vec<AnyView> {
    let mut views = Vec::new();
    if !adds.commands.is_empty() {
        views.push(
            note(format!("Commands: {}", adds.commands.join(", ")))
                .boxed(),
        );
    }
    if !adds.menus.is_empty() {
        views.push(
            note(format!("Menu: {}", adds.menus.join(", "))).boxed(),
        );
    }
    for key in &adds.keys {
        views.push(note(format!("Keys: {}", key.words)).boxed());
        if let Some(flag) = &key.flag {
            views.push(
                note(flag.clone()).tone(Tone::Critical).boxed(),
            );
        }
    }
    views
}
