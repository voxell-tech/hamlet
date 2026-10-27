use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use rustix::fs::{AtFlags, Dir, FileType, OFlags};
use rustix::io::Errno;

use super::System;
use crate::fs::{
    Exists, FileId, Kind, LastLink, Mode, Name, Open, Removing,
    Replace, Stat,
};

pub(crate) struct Unix;

const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

impl System for Unix {
    type Handle = OwnedFd;

    fn open_path(path: &Path, last: LastLink) -> io::Result<OwnedFd> {
        let flags = match last {
            LastLink::Follow => {
                DIRECTORY.difference(OFlags::NOFOLLOW)
            }
            LastLink::Refuse => DIRECTORY,
        };
        Ok(rustix::fs::open(path, flags, rustix::fs::Mode::empty())?)
    }

    fn lstat(path: &Path) -> io::Result<Stat> {
        Ok(stat_of(&rustix::fs::lstat(path)?))
    }

    fn duplicate(dir: &OwnedFd) -> io::Result<OwnedFd> {
        dir.try_clone()
    }

    fn status(dir: &OwnedFd) -> io::Result<Stat> {
        Ok(stat_of(&rustix::fs::fstat(dir)?))
    }

    fn stat_at(dir: &OwnedFd, name: &Name) -> io::Result<Stat> {
        let info = rustix::fs::statat(
            dir,
            name.as_os(),
            AtFlags::SYMLINK_NOFOLLOW,
        )?;
        Ok(stat_of(&info))
    }

    fn open_directory_at(
        dir: &OwnedFd,
        name: &Name,
    ) -> io::Result<OwnedFd> {
        Ok(rustix::fs::openat(
            dir,
            name.as_os(),
            DIRECTORY,
            rustix::fs::Mode::empty(),
        )?)
    }

    fn make_directory_at(
        dir: &OwnedFd,
        name: &Name,
        mode: Mode,
        exists: Exists,
    ) -> io::Result<OwnedFd> {
        match rustix::fs::mkdirat(dir, name.as_os(), raw(mode)) {
            Ok(()) => {}
            Err(Errno::EXIST) if matches!(exists, Exists::Open) => {}
            Err(error) => return Err(error.into()),
        }
        Self::open_directory_at(dir, name)
    }

    fn open_file_at(
        dir: &OwnedFd,
        name: &Name,
        how: Open,
    ) -> io::Result<(File, Stat)> {
        let (flags, mode) = match how {
            Open::Read => (OFlags::RDONLY, rustix::fs::Mode::empty()),
            Open::Append => (
                OFlags::WRONLY | OFlags::APPEND,
                rustix::fs::Mode::empty(),
            ),
            Open::New(mode) => (
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
                raw(mode),
            ),
            Open::Truncate(mode) => (
                OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC,
                raw(mode),
            ),
            Open::Extend(mode) => (
                OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND,
                raw(mode),
            ),
        };
        let fd = rustix::fs::openat(
            dir,
            name.as_os(),
            flags
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            mode,
        )?;
        let stat = stat_of(&rustix::fs::fstat(&fd)?);
        Ok((File::from(fd), stat))
    }

    fn read_link_at(
        dir: &OwnedFd,
        name: &Name,
    ) -> io::Result<String> {
        let link =
            rustix::fs::readlinkat(dir, name.as_os(), Vec::new())?;
        Ok(link.to_string_lossy().into_owned())
    }

    fn set_mode(dir: &OwnedFd, mode: Mode) -> io::Result<()> {
        Ok(rustix::fs::fchmod(dir, raw(mode))?)
    }

    fn set_file_mode(file: &File, mode: Mode) -> io::Result<()> {
        Ok(rustix::fs::fchmod(file, raw(mode))?)
    }

    fn rename_at(
        from: &OwnedFd,
        name: &Name,
        to: &OwnedFd,
        to_name: &Name,
        replace: Replace,
    ) -> io::Result<()> {
        match replace {
            Replace::Never => rustix::fs::renameat_with(
                from,
                name.as_os(),
                to,
                to_name.as_os(),
                rustix::fs::RenameFlags::NOREPLACE,
            )?,
            Replace::Always => {
                rustix::fs::renameat(
                    from,
                    name.as_os(),
                    to,
                    to_name.as_os(),
                )?;
            }
        }
        Ok(())
    }

    fn remove_at(
        dir: &OwnedFd,
        name: &Name,
        removing: Removing,
    ) -> io::Result<()> {
        let flags = match removing {
            Removing::File => AtFlags::empty(),
            Removing::Directory => AtFlags::REMOVEDIR,
        };
        Ok(rustix::fs::unlinkat(dir, name.as_os(), flags)?)
    }

    fn names(
        dir: &OwnedFd,
    ) -> io::Result<impl Iterator<Item = io::Result<Name>>> {
        Ok(Dir::read_from(dir)?.filter_map(|entry| match entry {
            Ok(entry) => {
                let name = entry.file_name().to_bytes();
                (name != b"." && name != b"..").then(|| {
                    Ok(Name::listed(
                        OsStr::from_bytes(name).to_owned(),
                    ))
                })
            }
            Err(error) => Some(Err(error.into())),
        }))
    }

    fn sync_directory(dir: &OwnedFd) -> io::Result<()> {
        Ok(rustix::fs::fsync(dir)?)
    }

    /// `path` resolved again, since a descriptor has no path.
    fn real_of(_: &OwnedFd, path: &str) -> io::Result<String> {
        Ok(crate::fs::cased(path))
    }
}

fn raw(mode: Mode) -> rustix::fs::Mode {
    rustix::fs::Mode::from_raw_mode(number(mode.bits()))
}

fn stat_of(info: &rustix::fs::Stat) -> Stat {
    Stat {
        kind: match FileType::from_raw_mode(info.st_mode) {
            FileType::Directory => Kind::Directory,
            FileType::RegularFile => Kind::File,
            FileType::Symlink => Kind::Link,
            _ => Kind::Special,
        },
        size: number(info.st_size),
        mode: Mode::new(number(info.st_mode)),
        links: number(info.st_nlink),
        id: FileId {
            // Signed on macOS. Through i128, every bit is kept.
            volume: i128::from(info.st_dev) as u64,
            index: number(info.st_ino),
        },
    }
}

/// `value` as `T`, for the numbers whose type differs between Linux
/// and macOS.
fn number<T: Default>(value: impl TryInto<T>) -> T {
    value.try_into().unwrap_or_default()
}
