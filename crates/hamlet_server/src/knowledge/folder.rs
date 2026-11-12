//! A folder on this machine the server keeps for an agent, reached
//! only through its open directory and never through a link: a lodge
//! or a worker's scratch folder.

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use hamlet_git::{Entry, blob_id};
use hamlet_os::fs::{
    Directory, Kind, LastLink, Missing, Mode, Open, Removing,
    Replace, TreePath,
};

/// The largest file a lodge commits. Larger ones stay in the lodge
/// alone.
pub(crate) const MOST_BYTES: u64 = 4 << 20;

pub(crate) struct Folder(Directory);

impl Folder {
    /// The folder at `path`, made with its parents when missing.
    pub fn make(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("making {}", path.display()))?;
        Self::open(path)
    }

    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self(
            Directory::open(path, LastLink::Refuse).with_context(
                || format!("opening {}", path.display()),
            )?,
        ))
    }

    /// Every regular file with one name and at most [`MOST_BYTES`],
    /// with its blob id, but under the paths `skip` names. Links and
    /// what they reach are left out.
    pub fn list(
        &self,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<Entry>> {
        let mut out = Vec::new();
        walk(&self.0, "", skip, &mut out)?;
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Every regular file under the folder `under`, by its path from
    /// this folder, as [`Folder::list`] gives them; none when there
    /// is no such folder or it is a link.
    pub fn list_under(&self, under: &str) -> Result<Vec<Entry>> {
        let tree = TreePath::parse(under)?;
        let Ok((dir, name)) = self.0.parent(&tree, Missing::Fail)
        else {
            return Ok(Vec::new());
        };
        let Ok(folder) = dir.child(name) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        walk(&folder, under, &|_| false, &mut out)?;
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Whether anything, a link included, is at `path`.
    pub fn exists(&self, path: &str) -> bool {
        let Ok(tree) = TreePath::parse(path) else {
            return false;
        };
        let Ok((dir, name)) = self.0.parent(&tree, Missing::Fail)
        else {
            return false;
        };
        matches!(dir.stat(name), Ok(Some(_)))
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let path = TreePath::parse(path)?;
        let (dir, name) = self.0.parent(&path, Missing::Fail)?;
        let mut bytes = Vec::new();
        dir.file(name, Open::Read)?
            .into_file()
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Writes `bytes` whole at `path`, making its folders, in place
    /// of what was there.
    pub fn write(
        &self,
        path: &str,
        bytes: &[u8],
        executable: bool,
    ) -> Result<()> {
        let path = TreePath::parse(path)?;
        let (dir, name) =
            self.0.parent(&path, Missing::Make(Mode::FOLDER))?;
        let mode = if executable {
            Mode::new(0o700)
        } else {
            Mode::PRIVATE
        };
        let incoming = hamlet_os::fs::Name::in_tree(&format!(
            ".incoming-{}",
            uuid::Uuid::new_v4()
        ))?;
        let mut file =
            dir.file(&incoming, Open::New(mode))?.into_file();
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
    pub fn remove(&self, path: &str) -> Result<()> {
        let tree = TreePath::parse(path)?;
        let Ok((dir, name)) = self.0.parent(&tree, Missing::Fail)
        else {
            return Ok(());
        };
        match dir.remove(name, Removing::File) {
            Ok(()) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut folders = path.split('/').collect::<Vec<_>>();
        folders.pop();
        while !folders.is_empty() {
            let at = TreePath::parse(&folders.join("/"))?;
            let (parent, last) = self.0.parent(&at, Missing::Fail)?;
            if parent.remove(last, Removing::Directory).is_err() {
                break;
            }
            folders.pop();
        }
        Ok(())
    }
}

fn walk(
    dir: &Directory,
    prefix: &str,
    skip: &dyn Fn(&str) -> bool,
    out: &mut Vec<Entry>,
) -> Result<()> {
    for name in dir.names()? {
        let Some(text) = name.to_str() else {
            continue;
        };
        if text.starts_with(".incoming-") {
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
                out.push(Entry {
                    path,
                    blob: blob_id(&bytes),
                    executable: stat.mode.bits() & 0o100 != 0,
                });
            }
            Kind::File | Kind::Link | Kind::Special => {}
        }
    }
    Ok(())
}

/// Removes the folder `name` in `dir` with everything in it.
pub(crate) fn remove_tree(
    dir: &Directory,
    name: &hamlet_os::fs::Name,
) -> Result<()> {
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

/// Removes the folder at `path` with everything in it, when it is
/// there, never following a link.
pub(crate) fn remove_folder(path: &Path) -> Result<()> {
    let (Some(parent), Some(name)) =
        (path.parent(), path.file_name())
    else {
        return Ok(());
    };
    let Ok(dir) = Directory::open(parent, LastLink::Refuse) else {
        return Ok(());
    };
    let name = hamlet_os::fs::Name::new(
        name.to_str().context("a folder's name is not UTF-8")?,
    )?;
    match dir.stat(&name)? {
        Some(stat) if stat.kind == Kind::Directory => {
            remove_tree(&dir, &name)
        }
        Some(_) => Ok(dir.remove(&name, Removing::File)?),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_folder_lists_its_own_files_and_nothing_a_link_reaches() {
        let dir = Scratch::new("server-folder");
        let outside = dir.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.md"), "s").unwrap();
        let lodge = Folder::make(&dir.join("lodge")).unwrap();
        lodge.write("memory/notes.md", b"n", false).unwrap();
        lodge.write("skills/a/run.sh", b"#!", true).unwrap();
        lodge.write("inbox/file.bin", b"g", false).unwrap();
        hamlet_testing::links::symlink(
            &outside,
            dir.join("lodge/out"),
        )
        .unwrap();
        let listed =
            lodge.list(&|path| path.starts_with("inbox")).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|e| e.path.as_str())
                .collect::<Vec<_>>(),
            ["memory/notes.md", "skills/a/run.sh"]
        );
        // Windows keeps no executable bit.
        if cfg!(unix) {
            assert!(listed[1].executable);
        }
        assert_eq!(listed[0].blob, blob_id(b"n"));
        lodge.remove("skills/a/run.sh").unwrap();
        assert!(!dir.join("lodge/skills").exists());
        assert_eq!(lodge.read("memory/notes.md").unwrap(), b"n");
        remove_folder(&dir.join("lodge")).unwrap();
        assert!(!dir.join("lodge").exists());
        assert!(outside.join("secret.md").exists());
    }
}
