//! Loading the approved plugins of the install folder at startup.

use bevy::prelude::*;
use hamlet_core::core::HomeDir;
use hamlet_openui::data::{self, DataPanel};
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::grant::Granted;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::plugin::{self as sdk, PluginInfo, Status};
use hamlet_sdk::tree::glyph;

use crate::approved::{self, Approval};
use crate::engine::Host;
use crate::install::{self, Body, Installed};
use crate::limits::Limits;
use crate::start::{self, Setting, Source};

/// The icon of a data plugin's panel when its manifest names none.
const DATA_ICON: Glyph = Glyph::Book;

/// Scans the install folder and loads the plugins `approved.toml`
/// names, each as an entity. A plugin that is not named is never
/// compiled. A plugin of code needs the engine; one of a panel does
/// not.
pub fn installed(
    home: Res<HomeDir>,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    mut commands: Commands,
) {
    let setting = host.as_deref().map(|host| Setting {
        host,
        limits: &limits,
        agents: &agents,
    });
    let folder = home.0.plugins();
    let approved = approved::list(&folder);
    for found in install::scan(&folder) {
        let installed = match found {
            Ok(installed) => installed,
            Err(rejected) => {
                warn!("{rejected}");
                continue;
            }
        };
        let id = &installed.manifest.id;
        let Some(approval) = approved.iter().find(|a| a.id == *id)
        else {
            info!("plugin {id}: installed, not approved, not loaded");
            continue;
        };
        match (&installed.body, &setting) {
            (Body::Wasm(_), Some(setting)) => {
                spawn(setting, installed, approval, &mut commands)
            }
            (Body::Wasm(_), None) => {
                warn!("plugin {id}: no engine, not loaded")
            }
            (Body::Data(_), _) => {
                spawn_data(installed, approval, &mut commands)
            }
        }
    }
}

/// Spawns the plugin entity with what both kinds of plugin have.
fn plugin(
    installed: &Installed,
    approval: &Approval,
    commands: &mut Commands,
) -> (Entity, PluginInfo, Granted) {
    let manifest = sdk::Manifest::from(&installed.manifest);
    let info = PluginInfo::from(&manifest);
    let granted = Granted::within(
        &info.id,
        &approval.grants,
        &installed
            .manifest
            .asks
            .iter()
            .map(|ask| ask.capability.as_str())
            .collect::<Vec<_>>(),
    );
    let plugin = commands
        .spawn((
            info.clone(),
            manifest,
            granted.clone(),
            Status::Starting,
        ))
        .id();
    (plugin, info, granted)
}

fn spawn(
    setting: &Setting,
    installed: Installed,
    approval: &Approval,
    commands: &mut Commands,
) {
    let Body::Wasm(file) = &installed.body else {
        return;
    };
    let (plugin, info, granted) =
        plugin(&installed, approval, commands);
    let source = Source(file.clone());
    commands.entity(plugin).insert(source.clone());
    start::bring_up(
        plugin, &info, &source, &granted, setting, commands,
    );
}

fn spawn_data(
    installed: Installed,
    approval: &Approval,
    commands: &mut Commands,
) {
    let Body::Data(file) = &installed.body else {
        return;
    };
    let (plugin, info, _) = plugin(&installed, approval, commands);
    let icon = installed
        .manifest
        .icon
        .as_deref()
        .and_then(glyph::named)
        .unwrap_or(DATA_ICON);
    let panel = DataPanel {
        file: file.clone(),
        title: installed.manifest.name.to_uppercase(),
        icon,
    };
    data::bring_up(plugin, &info.id, &panel, commands);
    commands.entity(plugin).insert(panel);
}
