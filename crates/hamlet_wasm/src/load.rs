//! Scanning the install folder at startup: the plugins `grants.toml`
//! records as installed are brought up, and the others wait for the
//! chief.

use bevy::prelude::*;
use hamlet_core::core::HomeDir;
use hamlet_openui::data::{self, DataPanel};
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{
    AskForMore, AskToInstall, Choices, Granted, Record, Standing,
    Store, Uninstalled, standing,
};
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{
    self as sdk, Blocked, PluginInfo, Status, needs,
};
use hamlet_sdk::tree::glyph;

use crate::engine::Host;
use crate::inbox::Ticks;
use crate::install::{self, Body, Installed};
use crate::limits::Limits;
use crate::reload::{Watch, keep};
use crate::start::{self, Setting, Source};

/// The icon of a data plugin's panel when its manifest names none.
const DATA_ICON: Glyph = Glyph::Book;

/// Scans the install folder. A plugin that `grants.toml` records as
/// installed is brought up as an entity. Any other is held as an
/// entity with nothing of it compiled or read, and the chief is asked
/// to install it unless it was declined as it is. A plugin of code
/// needs the engine; one of a panel does not.
pub fn installed(
    home: Res<HomeDir>,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    mut commands: Commands,
) {
    let setting = host.as_deref().map(|host| Setting {
        host,
        limits: &limits,
        agents: &agents,
        papers: &papers,
    });
    let folder = home.0.plugins();
    let mut store = Store::open(&folder);
    let mut found = Vec::new();
    for scanned in install::scan(&folder) {
        match scanned {
            Ok(installed) => {
                let id = &installed.manifest.id;
                let standing = standing(
                    store.get(id),
                    &installed.manifest.version,
                    &installed.asks,
                );
                found.push((installed, standing));
            }
            Err(rejected) => warn!("{rejected}"),
        }
    }
    let held = unmet(&found);
    for (installed, standing) in found {
        let id = installed.manifest.id.clone();
        let version = &installed.manifest.version;
        match standing {
            Standing::Installed { choices, fresh } => {
                store.set(
                    &id,
                    Record::installed(version, &choices)
                        .scoped(&installed.asks),
                );
                let because =
                    held.iter().find(|(held, _)| *held == id);
                match (&installed.body, &setting, because) {
                    (Body::Wasm(_), _, Some((_, why))) => {
                        warn!("plugin {id}: not started, {why}");
                        keep_off(
                            installed,
                            choices,
                            why,
                            &mut commands,
                        )
                    }
                    (Body::Wasm(_), Some(setting), None) => spawn(
                        setting,
                        installed,
                        choices,
                        &mut commands,
                    ),
                    (Body::Wasm(_), None, None) => {
                        warn!("plugin {id}: no engine, not loaded")
                    }
                    (Body::Data(_), ..) => {
                        spawn_data(installed, choices, &mut commands)
                    }
                }
                if !fresh.is_empty() {
                    commands.trigger(AskForMore {
                        plugin: id,
                        capabilities: fresh,
                    });
                }
            }
            Standing::Declined => {
                info!("plugin {id}: declined, not loaded");
                hold(installed, &mut commands);
            }
            Standing::Unknown => {
                info!("plugin {id}: not installed, not loaded");
                hold(installed, &mut commands);
                commands.trigger(AskToInstall { plugin: id });
            }
        }
    }
    commands.insert_resource(store);
}

/// The installed plugins of code that cannot start, each with the
/// need it lacks, given that those that can are the ones that do.
fn unmet(found: &[(Installed, Standing)]) -> Vec<(String, String)> {
    let listed = found
        .iter()
        .map(|(plugin, standing)| needs::Listed {
            id: &plugin.manifest.id,
            version: &plugin.manifest.version,
            needs: match plugin.body {
                Body::Wasm(_) => &plugin.manifest.needs,
                Body::Data(_) => &[],
            },
            installed: matches!(standing, Standing::Installed { .. }),
        })
        .collect::<Vec<_>>();
    needs::blocked(&listed)
        .into_iter()
        .map(|(id, why)| (id, why.to_string()))
        .collect()
}

/// Spawns the plugin entity of an installed plugin of code that is
/// held back, switched off for what it needs.
fn keep_off(
    installed: Installed,
    choices: Choices,
    why: &str,
    commands: &mut Commands,
) {
    let Body::Wasm(file) = &installed.body else {
        return;
    };
    let (plugin, ..) = plugin(&installed, choices, commands);
    commands.entity(plugin).insert((
        Status::Disabled,
        Source(file.clone()),
        Blocked(why.into()),
    ));
}

/// Spawns the plugin entity with what both kinds of plugin have.
fn plugin(
    installed: &Installed,
    choices: Choices,
    commands: &mut Commands,
) -> (Entity, PluginInfo, Granted) {
    let manifest = sdk::Manifest::from(&installed.manifest);
    let info = PluginInfo::from(&manifest);
    let granted = Granted::of(&installed.asks, &choices);
    let plugin = commands
        .spawn((
            info.clone(),
            manifest,
            installed.asks.clone(),
            choices,
            Status::Starting,
            Ticks::of(&installed.manifest),
            Watch::new(installed.body.folder()),
        ))
        .id();
    (plugin, info, granted)
}

/// Spawns the entity of a plugin that is not installed, with the
/// place of its files and nothing else of it.
fn hold(installed: Installed, commands: &mut Commands) {
    let (plugin, ..) =
        plugin(&installed, Choices::default(), commands);
    commands
        .entity(plugin)
        .insert((Status::Disabled, Uninstalled));
    match installed.body {
        Body::Wasm(file) => {
            commands.entity(plugin).insert(Source(file));
        }
        Body::Data(file) => {
            commands
                .entity(plugin)
                .insert(panel(&installed.manifest, file));
        }
    }
}

/// The panel of a data plugin.
pub(crate) fn panel(
    manifest: &crate::manifest::Manifest,
    file: std::path::PathBuf,
) -> DataPanel {
    let icon = manifest
        .icon
        .as_deref()
        .and_then(glyph::named)
        .unwrap_or(DATA_ICON);
    DataPanel {
        file,
        title: manifest.name.to_uppercase(),
        icon,
    }
}

fn spawn(
    setting: &Setting,
    installed: Installed,
    choices: Choices,
    commands: &mut Commands,
) {
    let Body::Wasm(file) = &installed.body else {
        return;
    };
    let (plugin, info, granted) =
        plugin(&installed, choices, commands);
    let source = Source(file.clone());
    commands.entity(plugin).insert(source.clone());
    start::bring_up(
        plugin,
        &info,
        &installed.manifest.exports,
        &source,
        &granted,
        setting,
        &[],
        commands,
    );
}

fn spawn_data(
    installed: Installed,
    choices: Choices,
    commands: &mut Commands,
) {
    let Body::Data(file) = &installed.body else {
        return;
    };
    let (plugin, info, _) = plugin(&installed, choices, commands);
    let panel = panel(&installed.manifest, file.clone());
    data::bring_up(plugin, &info.id, &panel, commands);
    let kept = keep::remember(&installed.body.folder());
    commands.entity(plugin).insert((panel, kept));
}
