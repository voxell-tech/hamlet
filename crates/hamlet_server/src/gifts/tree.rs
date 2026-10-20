//! Private gift stages on this machine, reached through opened
//! directories.

use std::ffi::OsString;
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;

use anyhow::{Result, ensure};
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, mkdirat, openat, statat,
    unlinkat,
};

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
