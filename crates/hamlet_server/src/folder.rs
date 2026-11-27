//! A folder on this machine the server reads for an agent, reached
//! only through its open directory and never through a link: the
//! agent's filesystem.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use hamlet_git::{Entry, blob_id};
use hamlet_os::fs::{
    Directory, Kind, LastLink, Missing, Open, TreePath,
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

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let path = TreePath::parse(path)?;
        let (dir, name) = self.0.parent(&path, Missing::Fail)?;
        let mut bytes = Vec::new();
        dir.file(name, Open::Read)?
            .into_file()
            .read_to_end(&mut bytes)?;
        Ok(bytes)
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
