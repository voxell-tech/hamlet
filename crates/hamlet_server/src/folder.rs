//! A folder on this machine the server reads for an agent, reached
//! only through its open directory and never through a link: the
//! agent's filesystem.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use hamlet_os::fs::{Directory, Kind, LastLink, Open};

/// The largest file read. Larger ones stay where they are.
const MOST_BYTES: u64 = 4 << 20;

pub(crate) struct Folder(Directory);

impl Folder {
    /// The folder at `path`, made with its parents when missing.
    pub fn make(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("making {}", path.display()))?;
        Ok(Self(
            Directory::open(path, LastLink::Refuse).with_context(
                || format!("opening {}", path.display()),
            )?,
        ))
    }

    /// Every regular file with one name and at most [`MOST_BYTES`],
    /// with its bytes and whether it runs, by path. Links and what
    /// they reach are left out.
    pub fn files(&self) -> Result<BTreeMap<String, (Vec<u8>, bool)>> {
        let mut out = BTreeMap::new();
        walk(&self.0, "", &mut out)?;
        Ok(out)
    }
}

fn walk(
    dir: &Directory,
    prefix: &str,
    out: &mut BTreeMap<String, (Vec<u8>, bool)>,
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
        let Some(stat) = dir.stat(&name)? else {
            continue;
        };
        match stat.kind {
            Kind::Directory => {
                walk(&dir.child(&name)?, &path, out)?;
            }
            Kind::File
                if stat.links == 1 && stat.size <= MOST_BYTES =>
            {
                let mut bytes = Vec::new();
                dir.file(&name, Open::Read)?
                    .into_file()
                    .read_to_end(&mut bytes)?;
                out.insert(
                    path,
                    (bytes, stat.mode.bits() & 0o100 != 0),
                );
            }
            Kind::File | Kind::Link | Kind::Special => {}
        }
    }
    Ok(())
}
