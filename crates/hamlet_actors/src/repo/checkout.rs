//! A folder under the realm folder that holds a commit's files, as a
//! dream's or the library's checkout: reached through its open
//! directory one name at a time, never through a link.

use std::collections::BTreeSet;
use std::io::{ErrorKind, Read as _, Write as _};
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use hamlet_git::{Entry, blob_id};
use hamlet_os::fs::{
    Directory, Exists, Kind, LastLink, Missing, Mode, Name, Open,
    Removing, Replace, TreePath,
};

/// The largest file a checkout lists. Larger ones stay in the folder
/// alone.
const MOST_BYTES: u64 = 4 << 20;

/// The prefix of a file being written, which a listing leaves out.
const INCOMING: &str = ".incoming-";

/// `under` as a path of names below the realm folder.
fn tree(under: &Path) -> Result<TreePath> {
    let names = under
        .components()
        .map(|component| match component {
            Component::Normal(name) => {
                name.to_str().context("a folder's name is not UTF-8")
            }
            _ => bail!(
                "{} is not a folder below the realm folder",
                under.display()
            ),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(TreePath::parse(&names.join("/"))?)
}

/// The folder `under` the realm folder `home`, or `None` when it is
/// not there.
pub(super) fn open(
    home: &Path,
    under: &Path,
) -> Result<Option<Directory>> {
    let tree = tree(under)?;
    let found =
        Directory::open(home, LastLink::Refuse).and_then(|home| {
            let (dir, name) = home.parent(&tree, Missing::Fail)?;
            dir.child(name)
        });
    match found {
        Ok(folder) => Ok(Some(folder)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The folder `under` the realm folder `home`, made with its parents
/// when it is not there.
pub(super) fn make(home: &Path, under: &Path) -> Result<Directory> {
    let tree = tree(under)?;
    let home = Directory::open(home, LastLink::Refuse)?;
    let (dir, name) =
        home.parent(&tree, Missing::Make(Mode::FOLDER))?;
    Ok(dir.make(name, Mode::FOLDER, Exists::Open)?)
}

/// A folder's files, but under the paths a listing skips.
#[derive(Default)]
pub(super) struct Listing {
    /// Every regular file with one name and at most [`MOST_BYTES`],
    /// with its blob id, in path order.
    pub files: Vec<Entry>,
    /// The regular files left alone: larger, or with more names.
    pub alone: BTreeSet<String>,
}

pub(super) fn list(
    folder: &Directory,
    skip: &dyn Fn(&str) -> bool,
) -> Result<Listing> {
    let mut out = Listing::default();
    walk(folder, "", skip, &mut out)?;
    out.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk(
    dir: &Directory,
    prefix: &str,
    skip: &dyn Fn(&str) -> bool,
    out: &mut Listing,
) -> Result<()> {
    for name in dir.names()? {
        let Some(text) = name.to_str() else {
            continue;
        };
        if text.starts_with(INCOMING) {
            continue;
        }
        let path = if prefix.is_empty() {
            text.to_owned()
        } else {
            format!("{prefix}/{text}")
        };
        if skip(&path) {
            continue;
        }
        let Some(stat) = dir.stat(&name)? else {
            continue;
        };
        match stat.kind {
            Kind::Directory => {
                walk(&dir.child(&name)?, &path, skip, out)?;
            }
            Kind::File
                if stat.links == 1 && stat.size <= MOST_BYTES =>
            {
                let mut bytes = Vec::new();
                dir.file(&name, Open::Read)?
                    .into_file()
                    .read_to_end(&mut bytes)?;
                out.files.push(Entry {
                    path,
                    blob: blob_id(&bytes),
                    executable: stat.mode.bits() & 0o100 != 0,
                });
            }
            Kind::File => {
                out.alone.insert(path);
            }
            Kind::Link | Kind::Special => {}
        }
    }
    Ok(())
}

pub(super) fn read(
    folder: &Directory,
    path: &str,
) -> Result<Vec<u8>> {
    let path = TreePath::parse(path)?;
    let (dir, name) = folder.parent(&path, Missing::Fail)?;
    let mut bytes = Vec::new();
    dir.file(name, Open::Read)?
        .into_file()
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Writes `bytes` whole at `path`, making its folders, in place of
/// what was there.
pub(super) fn write(
    folder: &Directory,
    path: &str,
    bytes: &[u8],
    executable: bool,
) -> Result<()> {
    let path = TreePath::parse(path)?;
    let (dir, name) =
        folder.parent(&path, Missing::Make(Mode::FOLDER))?;
    let mode = if executable {
        Mode::new(0o700)
    } else {
        Mode::PRIVATE
    };
    let incoming = Name::in_tree(&format!(
        "{INCOMING}{}",
        uuid::Uuid::new_v4()
    ))?;
    let mut file = dir.file(&incoming, Open::New(mode))?.into_file();
    file.write_all(bytes)?;
    file.sync_all()?;
    if let Some(stat) = dir.stat(name)?
        && stat.kind == Kind::Directory
    {
        remove_tree(&dir, name)?;
    }
    dir.rename(&incoming, &dir, name, Replace::Always)?;
    Ok(())
}

/// Removes the file at `path`, and the folders it leaves empty.
pub(super) fn remove(folder: &Directory, path: &str) -> Result<()> {
    let tree = TreePath::parse(path)?;
    let Ok((dir, name)) = folder.parent(&tree, Missing::Fail) else {
        return Ok(());
    };
    match dir.remove(name, Removing::File) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut folders = path.split('/').collect::<Vec<_>>();
    folders.pop();
    while !folders.is_empty() {
        let at = TreePath::parse(&folders.join("/"))?;
        let (parent, last) = folder.parent(&at, Missing::Fail)?;
        if parent.remove(last, Removing::Directory).is_err() {
            break;
        }
        folders.pop();
    }
    Ok(())
}

/// Removes the folder `under` the realm folder `home` with everything
/// in it, when it is there.
pub(super) fn remove_folder(home: &Path, under: &Path) -> Result<()> {
    let tree = tree(under)?;
    let home = Directory::open(home, LastLink::Refuse)?;
    let Ok((dir, name)) = home.parent(&tree, Missing::Fail) else {
        return Ok(());
    };
    match dir.stat(name)? {
        Some(stat) if stat.kind == Kind::Directory => {
            remove_tree(&dir, name)
        }
        Some(_) => Ok(dir.remove(name, Removing::File)?),
        None => Ok(()),
    }
}

/// Removes the folder `name` in `dir` with everything in it.
fn remove_tree(dir: &Directory, name: &Name) -> Result<()> {
    let child = dir.child(name)?;
    for inner in child.names()? {
        match child.stat(&inner)?.map(|stat| stat.kind) {
            Some(Kind::Directory) => remove_tree(&child, &inner)?,
            Some(_) => child.remove(&inner, Removing::File)?,
            None => {}
        }
    }
    dir.remove(name, Removing::Directory)?;
    Ok(())
}
