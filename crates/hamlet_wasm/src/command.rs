//! The commands, menu lines, context menu items, inspector tabs,
//! settings pages and shortcuts of an installed plugin, as entities
//! linked to it.

use bevy::prelude::*;
use hamlet_agents_api::tools::AgentTool;
use hamlet_sdk::capability::{Capability, Granted};
use hamlet_sdk::command::Command;
use hamlet_sdk::context::ContextItem;
use hamlet_sdk::inspector::InspectorSection;
use hamlet_sdk::keys::Key;
use hamlet_sdk::menu::MenuEntry;
use hamlet_sdk::plugin::{ContributedBy, Entry, Exports, PluginInfo};
use hamlet_sdk::settings::SettingsPage;
use hamlet_sdk::tree::id::dock_id;

use crate::host::Offers;

/// What a plugin adds that this module spawns.
pub type Contributed = Or<(
    With<Command>,
    With<MenuEntry>,
    With<ContextItem>,
    With<InspectorSection>,
    With<SettingsPage>,
    With<Key>,
    With<AgentTool>,
)>;

/// Spawns the commands `offers` declares that the plugin `id` lists
/// in its `exports`, and the menu lines, context menu items and
/// shortcuts that run them, as contributions of `plugin`. The others
/// are dropped with a warning, since nothing outside the lists is
/// reachable.
pub fn spawn(
    plugin: Entity,
    info: &PluginInfo,
    granted: &Granted,
    exports: &Exports,
    offers: &Offers,
    commands: &mut Commands,
) {
    let id = info.id.as_str();
    for tool in &offers.tools {
        commands.spawn((
            AgentTool {
                plugin: id.into(),
                label: info.name.clone(),
                name: tool.name.clone(),
                description: tool.description.clone(),
                fields: tool.fields.clone(),
                hamlets: granted
                    .scope(Capability::AgentTool)
                    .to_vec(),
                live: granted.holds(Capability::AgentTool),
            },
            ContributedBy(plugin),
        ));
    }
    let mut entry = Entry::default();
    let mut kept = Vec::new();
    for offered in &offers.commands {
        if exports.commands.contains(&offered.id) {
            entry.forwarded(
                &offered.id,
                &offered.title,
                offered.icon,
            );
            kept.push(offered.id.as_str());
        } else {
            warn!(
                "plugin {id}: the command `{}` is not in its \
                 exports, dropped",
                offered.id
            );
        }
    }
    let known = |command: &str, what: &str| {
        let known = kept.contains(&command);
        if !known {
            warn!(
                "plugin {id}: the {what} runs `{command}`, which it \
                 did not declare, dropped"
            );
        }
        known
    };
    for placed in &offers.menus {
        if known(&placed.command, "menu line") {
            entry.menu_item(
                placed.menu,
                placed.section,
                &placed.label,
                &placed.command,
                None,
            );
        }
    }
    for targeted in &offers.contexts {
        if known(&targeted.command, "context item") {
            entry.context_item(
                &targeted.target,
                targeted.section,
                &targeted.label,
                &targeted.command,
                None,
            );
        }
    }
    for tabbed in &offers.tabs {
        entry.inspector_section(
            &tabbed.target,
            &tabbed.title,
            &dock_id(id, &tabbed.panel),
        );
    }
    for paged in &offers.pages {
        entry.settings_page(
            &paged.title,
            paged.icon,
            &dock_id(id, &paged.panel),
        );
    }
    for chorded in &offers.keys {
        if known(&chorded.command, "shortcut") {
            entry.key(chorded.chord, &chorded.command);
        }
    }
    entry.spawn(plugin, commands);
}

/// Makes what `plugin` contributes the commands, menu lines and
/// shortcuts of `offers` that it lists in its `exports`, given the
/// `held` ones it has.
pub fn replace(
    plugin: Entity,
    info: &PluginInfo,
    granted: &Granted,
    exports: &Exports,
    held: &[Entity],
    offers: &Offers,
    commands: &mut Commands,
) {
    for entity in held {
        commands.entity(*entity).despawn();
    }
    spawn(plugin, info, granted, exports, offers, commands);
}

/// Keeps the tools of a plugin live only while the chief allows it
/// to give them.
pub fn follow(
    plugins: Query<&Granted>,
    mut tools: Query<(&mut AgentTool, &ContributedBy)>,
) {
    for (mut tool, by) in &mut tools {
        let live = plugins.get(by.0).is_ok_and(|granted| {
            granted.holds(Capability::AgentTool)
        });
        if tool.live != live {
            tool.live = live;
        }
    }
}

/// The commands, menu lines and shortcuts among `contributions`,
/// from `query`.
pub fn held(
    contributions: impl IntoIterator<Item = Entity>,
    query: &Query<(), Contributed>,
) -> Vec<Entity> {
    contributions
        .into_iter()
        .filter(|entity| query.contains(*entity))
        .collect()
}
