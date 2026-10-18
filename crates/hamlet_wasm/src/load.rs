//! Loading the approved plugins of the install folder at startup.

use bevy::prelude::*;
use hamlet_core::core::HomeDir;
use hamlet_sdk::plugin::{self as sdk, PluginInfo, Status};

use crate::approved;
use crate::engine::Host;
use crate::install::{self, Installed};
use crate::limits::Limits;
use crate::start::{self, Source};

/// Scans the install folder and loads the plugins `approved.toml`
/// names, each as an entity. A plugin that is not named is never
/// compiled.
pub fn installed(
    home: Res<HomeDir>,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    let folder = home.0.plugins();
    let approved = approved::ids(&folder);
    for found in install::scan(&folder) {
        let installed = match found {
            Ok(installed) => installed,
            Err(rejected) => {
                warn!("{rejected}");
                continue;
            }
        };
        let id = &installed.manifest.id;
        if !approved.contains(id) {
            info!("plugin {id}: installed, not approved, not loaded");
            continue;
        }
        spawn(&host, &limits, installed, &mut commands);
    }
}

fn spawn(
    host: &Host,
    limits: &Limits,
    installed: Installed,
    commands: &mut Commands,
) {
    let manifest = sdk::Manifest::from(&installed.manifest);
    let info = PluginInfo::from(&manifest);
    let plugin = commands
        .spawn((
            info.clone(),
            manifest,
            Source(installed.wasm.clone()),
            Status::Starting,
        ))
        .id();
    start::bring_up(
        plugin,
        &info,
        &installed.wasm,
        host,
        limits,
        commands,
    );
}
