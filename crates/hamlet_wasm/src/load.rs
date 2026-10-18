//! Loading the approved plugins of the install folder at startup.

use bevy::prelude::*;
use hamlet_core::core::HomeDir;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::grant::Granted;
use hamlet_sdk::plugin::{self as sdk, PluginInfo, Status};

use crate::approved::{self, Approval};
use crate::engine::Host;
use crate::install::{self, Installed};
use crate::limits::Limits;
use crate::start::{self, Setting, Source};

/// Scans the install folder and loads the plugins `approved.toml`
/// names, each as an entity. A plugin that is not named is never
/// compiled.
pub fn installed(
    home: Res<HomeDir>,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    let setting = Setting {
        host: &host,
        limits: &limits,
        agents: &agents,
    };
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
        spawn(&setting, installed, approval, &mut commands);
    }
}

fn spawn(
    setting: &Setting,
    installed: Installed,
    approval: &Approval,
    commands: &mut Commands,
) {
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
    let source = Source(installed.wasm);
    let plugin = commands
        .spawn((
            info.clone(),
            manifest,
            source.clone(),
            granted.clone(),
            Status::Starting,
        ))
        .id();
    start::bring_up(
        plugin, &info, &source, &granted, setting, commands,
    );
}
