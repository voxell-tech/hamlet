//! Bounded regular files reached through opened directories.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use anyhow::{Result, ensure};
use hamlet_workspace_wire::{Code, TreeEntry, TreeKind};
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, mkdirat, openat, statat,
    unlinkat,
};
use sha2::{Digest, Sha256};

use crate::workspace::Failure;

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
    scan(root, "", &mut files, &mut left, &mut remaining, false)?;
    Ok(files)
}

/// The library's files, without its hidden ones, within `cap` bytes
/// and `count` entries.
pub(crate) fn library(
    root: &File,
    cap: u64,
    count: usize,
) -> Result<BTreeMap<String, Entry>> {
    let mut files = BTreeMap::new();
    scan(root, "", &mut files, &mut { cap }, &mut { count }, true)?;
    Ok(files)
}

/// What `path` names, a link itself rather than what it names, or
/// `None` where nothing is.
pub(crate) fn kind(
    root: &File,
    path: &str,
) -> Result<Option<TreeKind>> {
    let (dir, name) = parent(root, path, false)?;
    match statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(info) => {
            Ok(Some(match FileType::from_raw_mode(info.st_mode) {
                FileType::Directory => TreeKind::Directory,
                FileType::RegularFile => TreeKind::File,
                FileType::Symlink => TreeKind::Link,
                _ => TreeKind::Special,
            }))
        }
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Makes the folder `path`, which must be new.
pub(crate) fn mkdir(root: &File, path: &str) -> Result<()> {
    let (dir, name) = parent(root, path, false)?;
    mkdirat(&dir, name.as_str(), Mode::from_raw_mode(0o700))?;
    Ok(())
}

/// Sets the permission bits of the file or folder `path`, or of
/// `root` itself for `.`.
pub(crate) fn set_mode(
    root: &File,
    path: &str,
    mode: u32,
) -> Result<()> {
    let permissions = std::fs::Permissions::from_mode(mode & 0o777);
    if path == "." {
        return Ok(root.set_permissions(permissions)?);
    }
    let (dir, name) = parent(root, path, false)?;
    let info =
        statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
    let target = match FileType::from_raw_mode(info.st_mode) {
        FileType::Directory => directory(&dir, &name, false)?,
        FileType::RegularFile => File::from(openat(
            &dir,
            name.as_str(),
            OFlags::RDONLY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::empty(),
        )?),
        _ => {
            anyhow::bail!("Links and special files cannot be copied.")
        }
    };
    target.set_permissions(permissions)?;
    Ok(())
}

/// Every entry of the tree, `.` first, with each regular file's
/// SHA-256, within `cap` bytes and `count` entries. A file with
/// other names is special.
pub(crate) fn entries(
    root: &File,
    cap: u64,
    count: usize,
) -> Result<Vec<TreeEntry>> {
    let mut entries = vec![TreeEntry {
        path: ".".into(),
        kind: TreeKind::Directory,
        size: 0,
        mode: root.metadata()?.mode() & 0o777,
        sha256: None,
    }];
    listed(root, "", &mut entries, &mut { cap }, count)?;
    Ok(entries)
}

/// The entries under `dir`, each folder's listed while it alone is
/// open, so a wide tree holds few descriptors.
fn listed(
    dir: &File,
    prefix: &str,
    entries: &mut Vec<TreeEntry>,
    left: &mut u64,
    count: usize,
) -> Result<()> {
    for item in Dir::read_from(dir)? {
        let item = item?;
        let name = item.file_name().to_str()?.to_owned();
        if [".", ".."].contains(&name.as_str()) {
            continue;
        }
        if entries.len() >= count {
            return Err(Failure::refused(
                Code::TooMany,
                "The tree has too many entries.",
            )
            .into());
        }
        let path = format!("{prefix}{name}");
        let info =
            statat(dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        let kind = match FileType::from_raw_mode(info.st_mode) {
            FileType::Directory => TreeKind::Directory,
            FileType::RegularFile if info.st_nlink == 1 => {
                TreeKind::File
            }
            FileType::Symlink => TreeKind::Link,
            _ => TreeKind::Special,
        };
        let (size, sha256) = if kind == TreeKind::File {
            let bytes = read_file(dir, &name, *left)?;
            *left -= bytes.len() as u64;
            (
                bytes.len() as u64,
                Some(format!("{:x}", Sha256::digest(&bytes))),
            )
        } else {
            (0, None)
        };
        let mode = permission_bits(info.st_mode);
        entries.push(TreeEntry {
            path: path.clone(),
            kind,
            size,
            mode,
            sha256,
        });
        if kind == TreeKind::Directory {
            let child = directory(dir, &name, false)?;
            listed(
                &child,
                &format!("{path}/"),
                entries,
                left,
                count,
            )?;
        }
    }
    Ok(())
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
    if bytes.len() as u64 > cap {
        return Err(Failure::refused(
            Code::TooLarge,
            "The file is too large.",
        )
        .into());
    }
    Ok(bytes)
}

/// The folder of society snapshots under the houses' root.
const SNAPSHOTS: &str = ".society-files";

/// The files of the snapshot `name`, read as [`read`] reads a tree.
pub(crate) fn read_snapshot(
    root: &File,
    name: &str,
    cap: u64,
    count: usize,
) -> Result<BTreeMap<String, Entry>> {
    read(
        &directory(&directory(root, SNAPSHOTS, false)?, name, false)?,
        cap,
        count,
    )
}

/// Erases the snapshot `name`.
pub(crate) fn erase_snapshot(root: &File, name: &str) -> Result<()> {
    erase(&directory(root, SNAPSHOTS, false)?, name)
}

pub(crate) fn sweep_snapshots(
    root: &File,
    kept: &[String],
) -> Result<()> {
    let snapshots = directory(root, SNAPSHOTS, true)?;
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
    let snapshots = directory(root, SNAPSHOTS, true)?;
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
    hidden: bool,
) -> Result<()> {
    for entry in Dir::read_from(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_str()?;
        if name == "."
            || name == ".."
            || hidden && name.starts_with('.')
        {
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
                false,
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
    use rustix::fs::{RenameFlags, renameat_with};
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
    file.set_permissions(std::fs::Permissions::from_mode(
        entry.mode & 0o777,
    ))?;
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
    match statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(info) => ensure!(
            FileType::from_raw_mode(info.st_mode)
                == FileType::RegularFile,
            "The file to replace is not a regular file."
        ),
        Err(rustix::io::Errno::NOENT) => {}
        Err(error) => return Err(error.into()),
    }
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

/// Erases the file or folder `path`.
pub(crate) fn erase_path(root: &File, path: &str) -> Result<()> {
    let (dir, name) = parent(root, path, false)?;
    let info =
        statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
    if FileType::from_raw_mode(info.st_mode) == FileType::Directory {
        erase(&dir, &name)
    } else {
        Ok(unlinkat(&dir, name.as_str(), AtFlags::empty())?)
    }
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

/// The permission bits of `mode`, a `u16` on macOS and a `u32` on
/// Linux.
fn permission_bits(mode: impl Into<u32>) -> u32 {
    mode.into() & 0o777
}
