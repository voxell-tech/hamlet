//! Bounded regular files reached through opened directories.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;

use anyhow::{Result, ensure};
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, mkdirat, openat, statat,
    unlinkat,
};

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct Entry {
    pub bytes: Vec<u8>,
    pub mode: u32,
}

pub(crate) fn directory(
    parent: &File,
    name: &str,
    create: bool,
) -> Result<File> {
    ensure!(
        !name.is_empty()
            && ![".", ".."].contains(&name)
            && !name.contains(['/', '\\', '\0']),
        "invalid tree path"
    );
    if create
        && let Err(error) =
            mkdirat(parent, name, Mode::from_raw_mode(0o700))
        && error != rustix::io::Errno::EXIST
    {
        return Err(error.into());
    }
    Ok(openat(
        parent,
        name,
        OFlags::RDONLY
            | OFlags::DIRECTORY
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

pub(crate) fn read(
    root: &File,
    cap: u64,
    entries: usize,
) -> Result<BTreeMap<String, Entry>> {
    let mut files = BTreeMap::new();
    let mut left = cap;
    let mut remaining = entries;
    scan(root, "", &mut files, &mut left, &mut remaining)?;
    Ok(files)
}

pub(crate) fn read_file(
    root: &File,
    path: &str,
    cap: u64,
) -> Result<Vec<u8>> {
    let (dir, name) = parent(root, path, false)?;
    let file = File::from(openat(
        &dir,
        name.as_str(),
        OFlags::RDONLY
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1,
        "Not a regular unlinked file."
    );
    let mut bytes = Vec::new();
    file.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= cap, "The file is too large.");
    Ok(bytes)
}

pub(crate) fn sweep_snapshots(
    root: &File,
    kept: &[String],
) -> Result<()> {
    let snapshots = directory(root, ".society-files", true)?;
    for entry in Dir::read_from(&snapshots)? {
        let entry = entry?;
        let name = entry.file_name().to_str()?;
        if [".", ".."].contains(&name)
            || kept.iter().any(|kept| kept == name)
        {
            continue;
        }
        let info =
            statat(&snapshots, name, AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(info.st_mode)
            == FileType::Directory
        {
            erase(&snapshots, name)?;
        } else {
            unlinkat(&snapshots, name, AtFlags::empty())?;
        }
    }
    Ok(())
}

pub(crate) fn snapshot(
    root: &File,
    files: &BTreeMap<String, Entry>,
) -> Result<String> {
    let snapshots = directory(root, ".society-files", true)?;
    let name = uuid::Uuid::new_v4().to_string();
    let snapshot = directory(&snapshots, &name, true)?;
    for (path, file) in files {
        put(&snapshot, path, file)?;
    }
    snapshot.sync_all()?;
    snapshots.sync_all()?;
    root.sync_all()?;
    crate::fault::point("after_society_snapshot");
    Ok(name)
}

fn scan(
    dir: &File,
    prefix: &str,
    files: &mut BTreeMap<String, Entry>,
    left: &mut u64,
    remaining: &mut usize,
) -> Result<()> {
    for entry in Dir::read_from(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_str()?;
        if name == "." || name == ".." {
            continue;
        }
        ensure!(
            *remaining > 0,
            "The house has too many files; remove the agent instead."
        );
        *remaining -= 1;
        let path = format!("{prefix}{name}");
        let info = statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)?;
        match FileType::from_raw_mode(info.st_mode) {
            FileType::Directory => scan(
                &directory(dir, name, false)?,
                &format!("{path}/"),
                files,
                left,
                remaining,
            )?,
            FileType::RegularFile => {
                let file = File::from(openat(
                    dir,
                    name,
                    OFlags::RDONLY
                        | OFlags::NOFOLLOW
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )?);
                let metadata = file.metadata()?;
                ensure!(
                    metadata.is_file() && metadata.nlink() == 1,
                    "Links and special files cannot be merged."
                );
                let mut bytes = Vec::new();
                file.take(left.saturating_add(1))
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() as u64 <= *left,
                    "The house is too large; remove the agent \
                     instead."
                );
                *left -= bytes.len() as u64;
                files.insert(
                    path,
                    Entry {
                        bytes,
                        mode: metadata.mode() & 0o777,
                    },
                );
            }
            _ => anyhow::bail!(
                "Links and special files cannot be merged."
            ),
        }
    }
    Ok(())
}

fn parent(
    root: &File,
    path: &str,
    create: bool,
) -> Result<(File, String)> {
    let parts = path.split('/').collect::<Vec<_>>();
    ensure!(
        parts.iter().all(|p| !p.is_empty()
            && ![".", ".."].contains(p)
            && !p.contains(['\\', '\0'])),
        "invalid tree path"
    );
    let mut dir = root.try_clone()?;
    for part in &parts[..parts.len() - 1] {
        dir = directory(&dir, part, create)?;
    }
    Ok((dir, parts[parts.len() - 1].into()))
}

pub(crate) fn put(
    root: &File,
    path: &str,
    entry: &Entry,
) -> Result<()> {
    use rustix::fs::{RenameFlags, fchmod, renameat_with};
    let bytes = &entry.bytes;
    let (dir, name) = parent(root, path, true)?;
    let temporary = format!(".incoming-{}", uuid::Uuid::new_v4());
    let mut file = File::from(openat(
        &dir,
        temporary.as_str(),
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    file.write_all(bytes)?;
    fchmod(&file, Mode::from_raw_mode(entry.mode & 0o777))?;
    file.sync_all()?;
    let renamed = renameat_with(
        &dir,
        temporary.as_str(),
        &dir,
        name.as_str(),
        RenameFlags::NOREPLACE,
    );
    if renamed.is_err() {
        unlinkat(&dir, temporary.as_str(), AtFlags::empty())?;
    }
    match renamed {
        Ok(()) => dir.sync_all()?,
        Err(error) if error == rustix::io::Errno::EXIST => {
            let file = File::from(openat(
                &dir,
                name.as_str(),
                OFlags::RDONLY
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            let metadata = file.metadata()?;
            ensure!(
                metadata.is_file() && metadata.nlink() == 1,
                "The merge target is not a regular file."
            );
            let mut present = Vec::new();
            file.take(bytes.len() as u64 + 1)
                .read_to_end(&mut present)?;
            ensure!(
                &present == bytes,
                "The merge target changed; inspect the houses \
                 before retrying."
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(crate) fn remove(root: &File, path: &str) -> Result<()> {
    let (dir, name) = parent(root, path, false)?;
    unlinkat(&dir, name.as_str(), AtFlags::empty())?;
    Ok(())
}

pub(crate) fn replace(
    root: &File,
    path: &str,
    bytes: &[u8],
) -> Result<()> {
    let (dir, name) = parent(root, path, false)?;
    let temporary = format!(".incoming-{}", uuid::Uuid::new_v4());
    let mut file = File::from(openat(
        &dir,
        temporary.as_str(),
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    file.write_all(bytes)?;
    file.sync_all()?;
    rustix::fs::renameat(
        &dir,
        temporary.as_str(),
        &dir,
        name.as_str(),
    )?;
    dir.sync_all()?;
    Ok(())
}

pub(crate) fn publish(
    staging: &File,
    target: &File,
    name: &str,
    files: &BTreeMap<String, Entry>,
) -> Result<()> {
    let stage = directory(staging, name, true)?;
    for (path, entry) in files {
        put(&stage, path, entry)?;
    }
    stage.sync_all()?;
    match rustix::fs::renameat_with(
        staging,
        name,
        target,
        name,
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        Ok(()) => {
            staging.sync_all()?;
            target.sync_all()?;
        }
        Err(rustix::io::Errno::EXIST) => {
            let cap = files
                .values()
                .map(|entry| entry.bytes.len() as u64)
                .sum();
            ensure!(
                read(&directory(target, name, false)?, cap, 4096)?
                    == *files,
                "The published skill changed; inspect the library \
                 before retrying."
            );
            erase(staging, name)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(crate) fn erase(root: &File, name: &str) -> Result<()> {
    let dir = match directory(root, name, false) {
        Ok(dir) => dir,
        Err(error)
            if error.downcast_ref::<rustix::io::Errno>()
                == Some(&rustix::io::Errno::NOENT) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    super::clear(&dir)?;
    unlinkat(root, name, AtFlags::REMOVEDIR)?;
    Ok(())
}
