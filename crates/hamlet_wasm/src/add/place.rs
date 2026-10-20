//! Copying a plugin's files into the install folder: into a folder of
//! the host's own there first, read again as the scan reads a plugin,
//! then moved into the plugin's place.

use std::io::Read;
use std::path::Path;

use hamlet_sdk::capability::ReadNative;

use super::check::Plan;
use crate::install::{self, manifest_text};
use crate::limits::Limits;

/// The folder of the install folder where a plugin's files are put
/// together before they take their place.
const STAGE: &str = ".adding";

/// Copies the files of `plan` into the install folder `plugins`,
/// where they take the place of what is there under the plugin's id.
/// The copy is read as the scan reads a plugin before it takes its
/// place, and must be the plugin and version the chief was asked
/// about.
pub fn place(
    plan: &Plan,
    plugins: &Path,
    limits: &Limits,
    native: Option<ReadNative>,
) -> Result<(), String> {
    let stage = plugins.join(STAGE).join(&plan.id);
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| {
        format!("the install folder cannot be written: {e}")
    })?;
    let placed = gather(plan, &stage, limits, native)
        .and_then(|()| put(plan, &stage, &plugins.join(&plan.id)));
    let _ = std::fs::remove_dir_all(&stage);
    let _ = std::fs::remove_dir(plugins.join(STAGE));
    placed
}

/// Copies the files of `plan` into `stage` and reads them as a
/// plugin.
fn gather(
    plan: &Plan,
    stage: &Path,
    limits: &Limits,
    native: Option<ReadNative>,
) -> Result<(), String> {
    for (name, from, most) in &plan.files {
        copy(name, from, &stage.join(name), *most)?;
    }
    let (id, version) = match (plan.native, native) {
        (true, Some(read)) => {
            let text = manifest_text(stage, limits)
                .map_err(|why| why.to_string())?;
            let contents = read(stage, &text, true)?;
            (contents.id, contents.version)
        }
        (true, None) => {
            return Err(
                "this Hamlet does not run native plugins".into()
            );
        }
        (false, _) => {
            let read = install::read(stage, limits)
                .map_err(|rejected| rejected.why.to_string())?;
            (read.manifest.id, read.manifest.version)
        }
    };
    match (id == plan.id, version == plan.version) {
        (true, true) => Ok(()),
        _ => Err("the folder changed while it was added".into()),
    }
}

/// Moves the files in `stage` to `dest`: the whole folder where there
/// is none, and each file over the one there otherwise, the manifest
/// last.
fn put(plan: &Plan, stage: &Path, dest: &Path) -> Result<(), String> {
    let moved = |e: std::io::Error| {
        format!("the files cannot take their place: {e}")
    };
    match std::fs::symlink_metadata(dest) {
        Err(_) => std::fs::rename(stage, dest).map_err(moved),
        Ok(meta) if meta.is_dir() => {
            for (name, ..) in &plan.files {
                std::fs::rename(stage.join(name), dest.join(name))
                    .map_err(moved)?;
            }
            Ok(())
        }
        Ok(_) => Err(format!(
            "plugins/{} is a link or not a folder; remove it by hand",
            plan.id
        )),
    }
}

/// Copies the file `from` to `to`, refused where it is a link, not a
/// plain file or over `most` bytes.
fn copy(
    name: &str,
    from: &Path,
    to: &Path,
    most: u64,
) -> Result<(), String> {
    let unread = |_| format!("{name} cannot be read");
    let meta = std::fs::symlink_metadata(from).map_err(unread)?;
    if meta.file_type().is_symlink() {
        return Err(format!("{name} is a link"));
    }
    if !meta.is_file() {
        return Err(format!("{name} is not a plain file"));
    }
    let over = |bytes: u64| {
        format!("{name} has {bytes} bytes, the most is {most}")
    };
    if meta.len() > most {
        return Err(over(meta.len()));
    }
    let opened = std::fs::File::open(from).map_err(unread)?;
    if !same(&meta, &opened.metadata().map_err(unread)?) {
        return Err(format!("{name} changed while it was read"));
    }
    let mut out = std::fs::File::create(to)
        .map_err(|e| format!("{name} cannot be written: {e}"))?;
    // No further than the cap, in case it grew since.
    let copied = std::io::copy(&mut opened.take(most + 1), &mut out)
        .map_err(unread)?;
    match copied > most {
        true => Err(over(copied)),
        false => out
            .sync_all()
            .map_err(|e| format!("{name} cannot be written: {e}")),
    }
}

/// Whether two looks at a file are of the same file: what was looked
/// at by its path, and what was opened.
fn same(
    looked: &std::fs::Metadata,
    opened: &std::fs::Metadata,
) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        opened.is_file()
            && looked.dev() == opened.dev()
            && looked.ino() == opened.ino()
    }
    #[cfg(not(unix))]
    {
        opened.is_file() && looked.len() == opened.len()
    }
}
