//! Swapping in the new version of a plugin that is a panel.

use std::path::Path;

use bevy::prelude::*;
use hamlet_openui::{Source as Program, data};
use hamlet_sdk::plugin::{Contributions, Status};

use super::{Case, renew};
use crate::install::{Installed, PANEL, read_capped};
use crate::load;
use crate::reload::keep;

/// Reads the program in `file` and has the panel drawn from it. A
/// panel keeps its state. Where the program has a mistake the panel
/// says so itself.
pub(super) fn swap(
    case: &Case,
    installed: &Installed,
    file: &Path,
    commands: &mut Commands,
) -> Result<(), String> {
    let cap = case.limits.panel_bytes as u64;
    let text = read_capped(file, cap)
        .map_err(|e| format!("{}: {e}", file.display()))?
        .ok_or_else(|| format!("{PANEL} has more than {cap} bytes"))
        .and_then(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| format!("{PANEL} is not UTF-8"))
        })?;
    let panel = load::panel(&installed.manifest, file.to_path_buf());
    renew(commands, case.plugin, installed, case.choices.clone());
    commands.entity(case.plugin).insert(panel.clone());
    match case.status {
        Status::Disabled => return Ok(()),
        Status::Failed => {
            commands
                .entity(case.plugin)
                .despawn_related::<Contributions>()
                .insert(Status::Starting);
            data::bring_up(
                case.plugin,
                &case.info.id,
                &panel,
                commands,
            );
        }
        _ => {
            for (entity, _) in &case.held {
                commands
                    .entity(*entity)
                    .insert(Program(text.clone()));
            }
        }
    }
    commands
        .entity(case.plugin)
        .insert(keep::remember(case.folder, case.limits));
    Ok(())
}
