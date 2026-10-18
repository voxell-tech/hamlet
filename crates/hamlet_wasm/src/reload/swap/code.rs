//! Swapping in the new version of a plugin of code.

use std::path::Path;

use bevy::prelude::*;
use hamlet_sdk::capability::Granted;
use hamlet_sdk::plugin::{self as sdk, PluginInfo, Status};

use super::{Case, renew};
use crate::draw::Dirty;
use crate::inbox::Inbox;
use crate::install::Installed;
use crate::reload::keep;
use crate::start::{self, Built, Setting, Source};
use crate::stop::Failure;
use crate::{panel, run};

/// Builds the version in `file` and runs its `entry` and a draw of
/// each panel it declares. Where all of it works, the version takes
/// the running one's place, its panels staying where they are in the
/// dock. A plugin the chief switched off only takes the new manifest.
pub(super) fn swap(
    case: &Case,
    installed: &Installed,
    file: &Path,
    setting: Option<&Setting>,
    commands: &mut Commands,
) -> Result<(), String> {
    if case.status == Status::Disabled {
        renew(commands, case.plugin, installed, case.choices.clone());
        return Ok(());
    }
    let setting = setting.ok_or("there is no engine")?;
    let manifest = sdk::Manifest::from(&installed.manifest);
    let info = PluginInfo::from(&manifest);
    let granted = Granted::of(&installed.asks, &case.choices);
    let source = Source(file.to_path_buf());
    let Built {
        mut instance,
        panels,
        wants,
    } = start::build(&info, &source, &granted, setting)
        .map_err(|fault| fault.to_string())?;
    for declared in &panels {
        instance
            .panel_view(setting.limits, &declared.id)
            .map_err(|fault| fault.to_string())?;
    }
    run::want(commands, case.plugin, &wants);
    let shown = panel::replace(
        case.plugin,
        &info.id,
        &case.held,
        &panels,
        commands,
    );
    renew(commands, case.plugin, installed, case.choices.clone());
    commands
        .entity(case.plugin)
        .insert((
            instance,
            Status::Running,
            Inbox::default(),
            Dirty::of(shown),
            keep::remember(case.folder),
        ))
        .remove::<Failure>();
    Ok(())
}
