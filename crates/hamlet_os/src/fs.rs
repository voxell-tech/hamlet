//! Directories held open, and what is reached from them one name at a
//! time without following a link at that name.

mod error;
mod mode;
mod name;
pub mod path;
mod sys;
mod walk;

use std::fs::File;
use std::io;
use std::path::Path;

pub use self::error::{SystemError, system_error};
pub use self::mode::Mode;
pub use self::name::{BadName, Name, TreePath};
use self::sys::{Os, System as _};
pub use self::walk::{
    Follow, MAX_LINKS, Reached, Unreached, cased, reach,
};

/// An open directory. Every operation below it is anchored to it, so
/// replacing one of its ancestors with a link changes nothing it
/// reaches.
#[derive(Debug)]
pub struct Directory(<Os as sys::System>::Handle);

/// What a name is, a link itself rather than what it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
    Link,
    /// A FIFO, a socket or a device.
    Special,
}

/// Which file a name is: two names with one id are one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId {
    volume: u64,
    index: u64,
}

/// A name's own status.
#[derive(Clone, Copy, Debug)]
pub struct Stat {
    pub kind: Kind,
    pub size: u64,
    pub mode: Mode,
    /// How many names the file has.
    pub links: u64,
    pub id: FileId,
}

/// An open regular file.
#[derive(Debug)]
pub struct Regular {
    file: File,
    stat: Stat,
}

/// The refusal [`Directory::file`] gives a name that is not a regular
/// file, with the name's kind.
#[derive(Debug, thiserror::Error)]
#[error("not a regular file")]
pub struct NotRegular(pub Kind);

impl NotRegular {
    /// The refusal `error` carries, where it carries one.
    pub fn of(error: &io::Error) -> Option<&Self> {
        error.get_ref()?.downcast_ref()
    }
}

impl From<NotRegular> for io::Error {
    fn from(refusal: NotRegular) -> Self {
        let kind = match refusal.0 {
            Kind::Directory => io::ErrorKind::IsADirectory,
            Kind::File | Kind::Link | Kind::Special => {
                io::ErrorKind::Other
            }
        };
        Self::new(kind, refusal)
    }
}

/// How [`Directory::file`] opens a name. Every open is non-blocking,
/// so a FIFO fails at once rather than waiting for its other end.
#[derive(Clone, Copy, Debug)]
pub enum Open {
    Read,
    /// To write at its end.
    Append,
    /// A new file. Any name already there fails with `AlreadyExists`.
    New(Mode),
    /// Emptied, or made where it is missing.
    Truncate(Mode),
    /// To write at its end, made where it is missing.
    Extend(Mode),
}

/// What [`Directory::parent`] does with a missing directory on the
/// way.
#[derive(Clone, Copy, Debug)]
pub enum Missing {
    Fail,
    Make(Mode),
}

/// What [`Directory::make`] does when the directory is already there.
#[derive(Clone, Copy, Debug)]
pub enum Exists {
    Fail,
    Open,
}

/// Whether a rename may replace a name at its target.
#[derive(Clone, Copy, Debug)]
pub enum Replace {
    /// Fails with `AlreadyExists` and changes nothing.
    Never,
    Always,
}

/// What [`Directory::remove`] removes.
#[derive(Clone, Copy, Debug)]
pub enum Removing {
    /// Anything but a directory. A link is removed itself.
    File,
    /// An empty directory.
    Directory,
}

/// Whether [`Directory::open`] follows a link at the last name of its
/// path.
#[derive(Clone, Copy, Debug)]
pub enum LastLink {
    Follow,
    Refuse,
}

impl Directory {
    /// The directory at `path`.
    pub fn open(path: &Path, last: LastLink) -> io::Result<Self> {
        Os::open_path(path, last).map(Self)
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Os::duplicate(&self.0).map(Self)
    }

    pub fn status(&self) -> io::Result<Stat> {
        Os::status(&self.0)
    }

    /// The status of `name`, or `None` where nothing is.
    pub fn stat(&self, name: &Name) -> io::Result<Option<Stat>> {
        found(Os::stat_at(&self.0, name))
    }

    /// The directory `name`. Anything else there fails, a link to a
    /// directory included.
    pub fn child(&self, name: &Name) -> io::Result<Self> {
        Os::open_directory_at(&self.0, name).map(Self)
    }

    /// Makes the directory `name` with `mode`, and opens it.
    pub fn make(
        &self,
        name: &Name,
        mode: Mode,
        exists: Exists,
    ) -> io::Result<Self> {
        Os::make_directory_at(&self.0, name, mode, exists).map(Self)
    }

    /// The directory holding the last name of `path`, reached from
    /// this one, and that name.
    pub fn parent<'a>(
        &self,
        path: &'a TreePath,
        missing: Missing,
    ) -> io::Result<(Self, &'a Name)> {
        let (folders, last) = path.split_last();
        let mut dir = self.try_clone()?;
        for name in folders {
            dir = match missing {
                Missing::Fail => dir.child(name)?,
                Missing::Make(mode) => {
                    dir.make(name, mode, Exists::Open)?
                }
            };
        }
        Ok((dir, last))
    }

    /// The regular file `name`. A name that opens as anything else
    /// fails with a [`NotRegular`].
    pub fn file(
        &self,
        name: &Name,
        how: Open,
    ) -> io::Result<Regular> {
        let (file, stat) = Os::open_file_at(&self.0, name, how)?;
        match stat.kind {
            Kind::File => Ok(Regular { file, stat }),
            kind => Err(NotRegular(kind).into()),
        }
    }

    pub(crate) fn read_link(
        &self,
        name: &Name,
    ) -> io::Result<String> {
        Os::read_link_at(&self.0, name)
    }

    pub fn set_mode(&self, mode: Mode) -> io::Result<()> {
        Os::set_mode(&self.0, mode)
    }

    /// Renames `name` here to `to_name` in `to`.
    pub fn rename(
        &self,
        name: &Name,
        to: &Self,
        to_name: &Name,
        replace: Replace,
    ) -> io::Result<()> {
        Os::rename_at(&self.0, name, &to.0, to_name, replace)
    }

    pub fn remove(
        &self,
        name: &Name,
        removing: Removing,
    ) -> io::Result<()> {
        Os::remove_at(&self.0, name, removing)
    }

    /// The names in this directory, but `.` and `..`, each read as
    /// it is taken.
    pub fn listing(
        &self,
    ) -> io::Result<impl Iterator<Item = io::Result<Name>>> {
        Os::names(&self.0)
    }

    /// Every name in this directory, but `.` and `..`.
    pub fn names(&self) -> io::Result<Vec<Name>> {
        self.listing()?.collect()
    }

    /// Makes the directory's entries durable.
    pub fn sync(&self) -> io::Result<()> {
        Os::sync_directory(&self.0)
    }

    /// The real path of this directory, which a walk reached as
    /// `path`.
    pub(crate) fn real(&self, path: &str) -> io::Result<String> {
        Os::real_of(&self.0, path)
    }
}

impl Regular {
    pub fn stat(&self) -> &Stat {
        &self.stat
    }

    /// Whether the file has no other name.
    pub fn is_lone(&self) -> bool {
        self.stat.links == 1
    }

    pub fn set_mode(&self, mode: Mode) -> io::Result<()> {
        Os::set_file_mode(&self.file, mode)
    }

    pub fn file(&self) -> &File {
        &self.file
    }

    pub fn into_file(self) -> File {
        self.file
    }
}

/// The status of the file at `path`, a link itself rather than what
/// it names, or `None` where nothing is. Searching the folders on the
/// way is enough, as for `lstat`.
pub fn stat(path: &Path) -> io::Result<Option<Stat>> {
    found(Os::lstat(path))
}

fn found(stat: io::Result<Stat>) -> io::Result<Option<Stat>> {
    match stat {
        Ok(stat) => Ok(Some(stat)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use hamlet_testing::Scratch;
    use hamlet_testing::links::{hard_link, symlink};

    use super::*;

    fn name(text: &str) -> Name {
        Name::new(text).unwrap()
    }

    #[test]
    fn a_link_at_the_last_name_is_refused() {
        let scratch = Scratch::new("os-link");
        fs::create_dir(scratch.join("folder")).unwrap();
        fs::write(scratch.join("file"), "outside").unwrap();
        symlink("folder", scratch.join("to-folder")).unwrap();
        symlink("file", scratch.join("to-file")).unwrap();
        let dir =
            Directory::open(&scratch, LastLink::Refuse).unwrap();
        assert!(dir.child(&name("to-folder")).is_err());
        assert!(dir.file(&name("to-file"), Open::Read).is_err());
        assert_eq!(
            dir.stat(&name("to-file")).unwrap().unwrap().kind,
            Kind::Link
        );
        assert!(
            Directory::open(
                &scratch.join("to-folder"),
                LastLink::Refuse
            )
            .is_err()
        );
        dir.remove(&name("to-file"), Removing::File).unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join("file")).unwrap(),
            "outside"
        );
    }

    #[test]
    fn a_hard_linked_file_is_not_lone() {
        let scratch = Scratch::new("os-hard-link");
        fs::write(scratch.join("file"), "kept").unwrap();
        let dir =
            Directory::open(&scratch, LastLink::Refuse).unwrap();
        assert!(
            dir.file(&name("file"), Open::Read).unwrap().is_lone()
        );
        hard_link(scratch.join("file"), scratch.join("second"))
            .unwrap();
        assert!(
            !dir.file(&name("file"), Open::Read).unwrap().is_lone()
        );
        assert_eq!(
            dir.stat(&name("second")).unwrap().unwrap().links,
            2
        );
    }

    #[test]
    fn a_no_replace_rename_keeps_the_target() {
        let scratch = Scratch::new("os-rename");
        fs::write(scratch.join("new"), "new").unwrap();
        fs::write(scratch.join("old"), "old").unwrap();
        let dir =
            Directory::open(&scratch, LastLink::Refuse).unwrap();
        let refused = dir
            .rename(&name("new"), &dir, &name("old"), Replace::Never)
            .unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(scratch.join("old")).unwrap(),
            "old"
        );
        dir.rename(&name("new"), &dir, &name("old"), Replace::Always)
            .unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join("old")).unwrap(),
            "new"
        );
    }

    #[test]
    fn a_folder_opened_as_a_file_is_not_regular() {
        let scratch = Scratch::new("os-folder-file");
        fs::create_dir(scratch.join("folder")).unwrap();
        let dir =
            Directory::open(&scratch, LastLink::Refuse).unwrap();
        let refused =
            dir.file(&name("folder"), Open::Read).unwrap_err();
        assert!(matches!(
            NotRegular::of(&refused),
            Some(NotRegular(Kind::Directory))
        ));
        assert_eq!(refused.kind(), io::ErrorKind::IsADirectory);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_open_fails_at_once() {
        let scratch = Scratch::new("os-fifo");
        hamlet_testing::fifo(&scratch.join("pipe"));
        let dir =
            Directory::open(&scratch, LastLink::Refuse).unwrap();
        for how in
            [Open::Read, Open::Append, Open::Truncate(Mode::PRIVATE)]
        {
            assert!(dir.file(&name("pipe"), how).is_err(), "{how:?}");
        }
        let read = dir.file(&name("pipe"), Open::Read).unwrap_err();
        assert!(matches!(
            NotRegular::of(&read),
            Some(NotRegular(Kind::Special))
        ));
        assert_eq!(
            dir.stat(&name("pipe")).unwrap().unwrap().kind,
            Kind::Special
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_store_refuses_a_junction() {
        let scratch = Scratch::new("os-junction");
        fs::create_dir(scratch.join("folder")).unwrap();
        fs::write(scratch.join("folder/file"), "inside").unwrap();
        hamlet_testing::links::junction(
            scratch.join("folder"),
            scratch.join("junction"),
        )
        .unwrap();
        let dir =
            Directory::open(&scratch, LastLink::Refuse).unwrap();
        assert_eq!(
            dir.stat(&name("junction")).unwrap().unwrap().kind,
            Kind::Link
        );
        assert!(dir.child(&name("junction")).is_err());
        assert!(dir.file(&name("junction"), Open::Read).is_err());
        assert!(
            Directory::open(
                &scratch.join("junction"),
                LastLink::Refuse
            )
            .is_err()
        );
        dir.remove(&name("junction"), Removing::File).unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join("folder/file")).unwrap(),
            "inside"
        );
    }

    #[cfg(windows)]
    #[test]
    fn names_that_differ_only_in_case_collide() {
        let scratch = Scratch::new("os-case");
        let dir =
            Directory::open(&scratch, LastLink::Refuse).unwrap();
        dir.file(&name("Notes.md"), Open::New(Mode::PRIVATE))
            .unwrap();
        let made = dir
            .file(&name("notes.md"), Open::New(Mode::PRIVATE))
            .unwrap_err();
        assert_eq!(made.kind(), io::ErrorKind::AlreadyExists);
        dir.file(&name("other"), Open::New(Mode::PRIVATE)).unwrap();
        let renamed = dir
            .rename(
                &name("other"),
                &dir,
                &name("NOTES.MD"),
                Replace::Never,
            )
            .unwrap_err();
        assert_eq!(renamed.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(dir.names().unwrap().len(), 2);
    }
}
