//! Private gift stages and no-follow delivery through opened
//! directories.

use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use anyhow::{Result, ensure};
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, RenameFlags, mkdirat,
    openat, renameat_with, statat, unlinkat,
};

use super::rules::MAX_BYTES;

pub(super) fn directory(
    parent: &File,
    name: &str,
    create: bool,
) -> Result<File> {
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

fn names(dir: &File) -> Result<Vec<OsString>> {
    let mut names = Vec::new();
    for entry in Dir::read_from(dir)? {
        let entry = entry?;
        if entry.file_name().to_bytes() != b"."
            && entry.file_name().to_bytes() != b".."
        {
            names.push(
                std::ffi::OsStr::from_bytes(
                    entry.file_name().to_bytes(),
                )
                .to_owned(),
            );
        }
    }
    Ok(names)
}

fn clear(dir: &File) -> Result<()> {
    dir.set_permissions(std::fs::Permissions::from_mode(0o700))?;
    for name in names(dir)? {
        let info = statat(dir, &name, AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(info.st_mode)
            == FileType::Directory
        {
            let child = File::from(openat(
                dir,
                &name,
                OFlags::RDONLY
                    | OFlags::DIRECTORY
                    | OFlags::NOFOLLOW
                    | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            clear(&child)?;
            unlinkat(dir, &name, AtFlags::REMOVEDIR)?;
        } else {
            unlinkat(dir, &name, AtFlags::empty())?;
        }
    }
    Ok(())
}

pub(super) fn sweep(root: &File) -> Result<()> {
    clear(&directory(root, ".gifts", true)?)
}

pub(super) struct Stage {
    parent: File,
    name: String,
    pub dir: File,
}

impl Stage {
    pub fn new(root: &File) -> Result<Self> {
        let parent = directory(root, ".gifts", true)?;
        let name = format!("gift-{}", uuid::Uuid::new_v4());
        mkdirat(&parent, name.as_str(), Mode::from_raw_mode(0o700))?;
        let dir = directory(&parent, &name, false)?;
        Ok(Self { parent, name, dir })
    }

    pub fn file(&self, path: &str, create: bool) -> Result<File> {
        ensure!(
            path.len() < 4096,
            "gift path exceeds the file system limit"
        );
        let mut parts = path.split('/').peekable();
        let mut dir = self.dir.try_clone()?;
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                let flags = if create {
                    OFlags::CREATE | OFlags::EXCL
                } else {
                    OFlags::APPEND
                };
                let file = File::from(openat(
                    &dir,
                    part,
                    OFlags::WRONLY
                        | OFlags::NOFOLLOW
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC
                        | flags,
                    Mode::from_raw_mode(0o644),
                )?);
                ensure!(
                    file.metadata()?.is_file(),
                    "gift stage is not a file"
                );
                return Ok(file);
            }
            dir = directory(&dir, part, create)?;
        }
        anyhow::bail!("empty staged path")
    }

    pub fn deliver(
        &self,
        shelf: &File,
        roots: &[String],
        number: bool,
    ) -> Result<Vec<String>> {
        let mut targets = Vec::new();
        for root in roots {
            let mut target = root.clone();
            let mut count = 1;
            while present(shelf, &target)? {
                ensure!(number, "gift target already exists");
                count += 1;
                let split = root
                    .rfind('.')
                    .filter(|i| *i > 0)
                    .unwrap_or(root.len());
                target = format!(
                    "{}-{count}{}",
                    &root[..split],
                    &root[split..]
                );
            }
            targets.push(target);
        }
        let mut delivered = Vec::new();
        let result = (|| {
            for (root, target) in roots.iter().zip(&targets) {
                let identity = statat(
                    &self.dir,
                    root.as_str(),
                    AtFlags::SYMLINK_NOFOLLOW,
                )?;
                renameat_with(
                    &self.dir,
                    root.as_str(),
                    shelf,
                    target.as_str(),
                    RenameFlags::NOREPLACE,
                )?;
                delivered.push((target.clone(), identity));
            }
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(error) = result {
            for (root, (target, identity)) in
                roots.iter().zip(&delivered).rev()
            {
                let current = statat(
                    shelf,
                    target.as_str(),
                    AtFlags::SYMLINK_NOFOLLOW,
                )?;
                ensure!(
                    current.st_dev == identity.st_dev
                        && current.st_ino == identity.st_ino,
                    "gift rollback target changed"
                );
                renameat_with(
                    shelf,
                    target.as_str(),
                    &self.dir,
                    root.as_str(),
                    RenameFlags::NOREPLACE,
                )?;
            }
            return Err(error);
        }
        Ok(targets)
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if clear(&self.dir)
            .and_then(|()| {
                unlinkat(
                    &self.parent,
                    self.name.as_str(),
                    AtFlags::REMOVEDIR,
                )?;
                Ok(())
            })
            .is_err()
        {
            eprintln!("hamlet: could not discard a gift.");
        }
    }
}

pub(super) fn shelf(
    house: &File,
    name: &str,
    roots: &[String],
) -> Result<File> {
    let shelf = directory(house, name, true)?;
    for root in roots {
        match statat(&shelf, root.as_str(), AtFlags::SYMLINK_NOFOLLOW)
        {
            Ok(info) => ensure!(
                FileType::from_raw_mode(info.st_mode)
                    != FileType::Symlink,
                "linked gift target"
            ),
            Err(error) if error == rustix::io::Errno::NOENT => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(shelf)
}

pub(super) fn present(dir: &File, name: &str) -> Result<bool> {
    match statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => Ok(true),
        Err(error) if error == rustix::io::Errno::NOENT => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn paper(
    shelf: &File,
    name: &str,
) -> Result<bool, &'static str> {
    if statat(shelf, name, AtFlags::SYMLINK_NOFOLLOW).is_ok_and(|s| {
        FileType::from_raw_mode(s.st_mode) == FileType::Symlink
    }) {
        return Err(
            "A skill gift cannot include links or special files."
        );
    }
    Ok(directory(shelf, name, false).is_ok_and(|dir| {
        statat(&dir, "SKILL.md", AtFlags::SYMLINK_NOFOLLOW).is_ok_and(
            |s| {
                FileType::from_raw_mode(s.st_mode)
                    == FileType::RegularFile
            },
        )
    }))
}

pub(super) fn copy(
    source: &File,
    stage: &Stage,
    name: &str,
) -> Result<(), &'static str> {
    let mut entries = 0;
    let mut bytes = 0;
    copy_entry(source, &stage.dir, name, &mut entries, &mut bytes)
}

fn copy_entry(
    source: &File,
    target: &File,
    name: &str,
    entries: &mut usize,
    bytes: &mut u64,
) -> Result<(), &'static str> {
    const REFUSED: &str = super::rules::DELIVERY;
    const UNSAFE: &str =
        "A skill gift cannot include links or special files.";
    const SIZE: &str = "The skill gift is larger than 32 MB.";
    *entries += 1;
    if *entries > 4096 {
        return Err("A skill gift takes at most 4096 files and \
                    folders at once.");
    }
    let info = statat(source, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| REFUSED)?;
    match FileType::from_raw_mode(info.st_mode) {
        FileType::Directory => {
            let source =
                directory(source, name, false).map_err(|_| UNSAFE)?;
            let target =
                directory(target, name, true).map_err(|_| REFUSED)?;
            for entry in
                Dir::read_from(&source).map_err(|_| REFUSED)?
            {
                let entry = entry.map_err(|_| REFUSED)?;
                let name = entry
                    .file_name()
                    .to_str()
                    .map_err(|_| REFUSED)?;
                if name == "." || name == ".." {
                    continue;
                }
                copy_entry(&source, &target, name, entries, bytes)?;
            }
            target
                .set_permissions(std::fs::Permissions::from_mode(
                    info.st_mode & 0o777,
                ))
                .map_err(|_| REFUSED)?;
        }
        FileType::RegularFile => {
            let file = File::from(
                openat(
                    source,
                    name,
                    OFlags::RDONLY
                        | OFlags::NOFOLLOW
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| UNSAFE)?,
            );
            let meta = file.metadata().map_err(|_| REFUSED)?;
            if !meta.is_file() {
                return Err(UNSAFE);
            }
            if meta.len() > MAX_BYTES - *bytes {
                return Err(SIZE);
            }
            let mut output = File::from(
                openat(
                    target,
                    name,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o600),
                )
                .map_err(|_| REFUSED)?,
            );
            *bytes += std::io::copy(
                &mut file.take(MAX_BYTES - *bytes + 1),
                &mut output,
            )
            .map_err(|_| REFUSED)?;
            if *bytes > MAX_BYTES {
                return Err(SIZE);
            }
            output.flush().map_err(|_| REFUSED)?;
            output
                .set_permissions(std::fs::Permissions::from_mode(
                    meta.mode() & 0o777,
                ))
                .map_err(|_| REFUSED)?;
        }
        _ => return Err(UNSAFE),
    }
    Ok(())
}
