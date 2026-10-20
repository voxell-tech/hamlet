//! The last good version of a plugin: a copy of its files in its
//! folder, made when a version starts, and put back on request.

use std::path::Path;

use bevy::prelude::*;
use hamlet_sdk::plugin::PluginInfo;
use hamlet_sdk::reload::{Reload, Rollback};

use super::watch::Digest;
use super::{Stamp, Watch};
use crate::install::{FILES, MANIFEST, cap, read_capped};
use crate::limits::Limits;

/// The folder of the copy, in the plugin's folder.
pub const FOLDER: &str = ".last-good";

/// Copies the files of the version in `folder` that started to its
/// last good copy, each read within its cap. The plugin has nothing
/// to roll back to, and no note.
pub fn remember(folder: &Path, limits: &Limits) -> Reload {
    let last = folder.join(FOLDER);
    for file in FILES {
        let to = last.join(file);
        let bytes = match read_capped(
            &folder.join(file),
            cap(file, limits),
        ) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                warn!(
                    "{}: not kept, it is over its cap",
                    to.display()
                );
                let _ = std::fs::remove_file(&to);
                continue;
            }
            Err(_) => {
                let _ = std::fs::remove_file(&to);
                continue;
            }
        };
        if read_capped(&to, cap(file, limits))
            .is_ok_and(|kept| kept.as_ref() == Some(&bytes))
        {
            continue;
        }
        let written = std::fs::create_dir_all(&last)
            .and_then(|()| std::fs::write(&to, bytes));
        if let Err(why) = written {
            warn!("{}: not kept: {why}", to.display());
        }
    }
    Reload::default()
}

/// The note for a version in `folder` that did not start, `why`. The
/// plugin can roll back where the last good copy is another version.
pub fn trouble(
    folder: &Path,
    why: String,
    limits: &Limits,
) -> Reload {
    Reload {
        note: Some(why),
        rollback: differs(folder, limits),
    }
}

/// Whether there is a last good copy in `folder` that is not the
/// files there. A file over its cap differs.
pub fn differs(folder: &Path, limits: &Limits) -> bool {
    let last = folder.join(FOLDER);
    let read = |path: &Path, file: &str| {
        read_capped(path, cap(file, limits)).ok()
    };
    last.join(MANIFEST).is_file()
        && FILES.iter().any(|file| {
            let here = read(&folder.join(file), file);
            here.as_ref().is_some_and(Option::is_none)
                || here != read(&last.join(file), file)
        })
}

/// Puts the last good copy in the place of the files of the plugin
/// the chief named, and has them looked at at once.
pub fn rollback(
    rollback: On<Rollback>,
    mut plugins: Query<(&PluginInfo, &mut Watch)>,
) {
    for (info, mut watch) in &mut plugins {
        if info.id != rollback.plugin {
            continue;
        }
        match restore(&watch.folder) {
            Ok(()) => {
                info!(
                    "plugin {}: back to its last good version",
                    info.id
                );
                watch.stamp = Stamp::default();
                watch.digest = Digest::default();
            }
            Err(why) => {
                warn!("plugin {}: no roll back: {why}", info.id)
            }
        }
    }
}

fn restore(folder: &Path) -> Result<(), String> {
    let last = folder.join(FOLDER);
    if !last.join(MANIFEST).is_file() {
        return Err("it has no last good version".into());
    }
    for file in FILES {
        let to = folder.join(file);
        let from = last.join(file);
        match from.is_file() {
            true => std::fs::copy(&from, &to).map(drop),
            false => std::fs::remove_file(&to).or_else(|e| {
                match e.kind() == std::io::ErrorKind::NotFound {
                    true => Ok(()),
                    false => Err(e),
                }
            }),
        }
        .map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(())
}
