//! Loading the approved plugins of the install folder at startup.

use bevy::prelude::*;
use hamlet_core::core::HomeDir;
use hamlet_sdk::plugin::{self as sdk, PluginInfo, Status};

use crate::engine::Host;
use crate::inbox::Inbox;
use crate::install::{self, Installed};
use crate::instance::Instance;
use crate::limits::Limits;
use crate::{approved, run};

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
    let started =
        Instance::from_file(&host.0, &installed.wasm, limits)
            .and_then(|mut instance| {
                let output = instance.entry(limits)?;
                output
                    .logs
                    .iter()
                    .for_each(|line| run::say(&info.id, line));
                Ok(instance)
            });
    match started {
        Ok(instance) => {
            commands.spawn((
                info,
                manifest,
                Status::Running,
                instance,
                Inbox::default(),
            ));
        }
        Err(fault) => {
            warn!("plugin {}: {fault}", info.id);
            commands.spawn((info, manifest, Status::Failed));
        }
    }
}
