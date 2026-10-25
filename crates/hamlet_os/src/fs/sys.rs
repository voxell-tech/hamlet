#[cfg(unix)]
mod unix;

use std::fmt::Debug;
use std::fs::File;
use std::io;
use std::path::Path;

#[cfg(unix)]
pub(crate) use self::unix::Unix as Os;
use super::{
    Exists, LastLink, Mode, Name, Open, Removing, Replace, Stat,
};

/// Every call that takes a [`Name`] reaches that one name relative to
/// the handle and follows no link at it.
pub(crate) trait System {
    type Handle: Debug + Send;

    fn open_path(
        path: &Path,
        last: LastLink,
    ) -> io::Result<Self::Handle>;
    fn duplicate(dir: &Self::Handle) -> io::Result<Self::Handle>;
    fn status(dir: &Self::Handle) -> io::Result<Stat>;
    fn stat_at(dir: &Self::Handle, name: &Name) -> io::Result<Stat>;
    /// Refuses anything but a directory, on the opened handle.
    fn open_directory_at(
        dir: &Self::Handle,
        name: &Name,
    ) -> io::Result<Self::Handle>;
    fn make_directory_at(
        dir: &Self::Handle,
        name: &Name,
        mode: Mode,
        exists: Exists,
    ) -> io::Result<Self::Handle>;
    /// Opens `name` whatever it is, with the status of the opened
    /// handle.
    fn open_file_at(
        dir: &Self::Handle,
        name: &Name,
        how: Open,
    ) -> io::Result<(File, Stat)>;
    fn set_mode(dir: &Self::Handle, mode: Mode) -> io::Result<()>;
    fn set_file_mode(file: &File, mode: Mode) -> io::Result<()>;
    fn rename_at(
        from: &Self::Handle,
        name: &Name,
        to: &Self::Handle,
        to_name: &Name,
        replace: Replace,
    ) -> io::Result<()>;
    fn remove_at(
        dir: &Self::Handle,
        name: &Name,
        removing: Removing,
    ) -> io::Result<()>;
    fn names(
        dir: &Self::Handle,
    ) -> io::Result<impl Iterator<Item = io::Result<Name>>>;
    fn sync_directory(dir: &Self::Handle) -> io::Result<()>;
}
