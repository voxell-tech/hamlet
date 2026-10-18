//! The install folder: `plugins/<id>/manifest.toml` and `plugin.wasm`
//! in the home.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::interface::VERSION;
use crate::manifest::Manifest;

const MANIFEST: &str = "manifest.toml";
const WASM: &str = "plugin.wasm";

/// A plugin whose folder passed the scan.
#[derive(Debug)]
pub struct Installed {
    pub manifest: Manifest,
    /// The component's file.
    pub wasm: PathBuf,
}

/// Why a folder is not a plugin the host can load.
#[derive(Debug, PartialEq)]
pub enum Why {
    Missing(&'static str),
    BadManifest(String),
    /// The manifest names an id other than the folder's.
    WrongFolder {
        id: String,
    },
    /// The plugin was built for an interface the host does not speak.
    Interface {
        found: String,
    },
}

/// A folder of the install folder that was turned down.
#[derive(Debug, PartialEq)]
pub struct Rejected {
    pub folder: String,
    pub why: Why,
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugins/{}: ", self.folder)?;
        match &self.why {
            Why::Missing(file) => write!(f, "no {file}"),
            Why::BadManifest(why) => {
                write!(f, "{MANIFEST} cannot be read: {why}")
            }
            Why::WrongFolder { id } => {
                write!(
                    f,
                    "the manifest's id `{id}` is not the folder's"
                )
            }
            Why::Interface { found } => write!(
                f,
                "built for interface {found}, the host speaks \
                 {VERSION}"
            ),
        }
    }
}

/// Reads the plugin in `folder`, a folder of the install folder.
pub fn read(folder: &Path) -> Result<Installed, Rejected> {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let rejected = |why| Rejected {
        folder: name.clone(),
        why,
    };
    let text = std::fs::read_to_string(folder.join(MANIFEST))
        .map_err(|_| rejected(Why::Missing(MANIFEST)))?;
    let wasm = folder.join(WASM);
    if !wasm.is_file() {
        return Err(rejected(Why::Missing(WASM)));
    }
    let manifest = Manifest::parse(&text).map_err(|e| {
        rejected(Why::BadManifest(e.message().into()))
    })?;
    if manifest.id != name {
        return Err(rejected(Why::WrongFolder { id: manifest.id }));
    }
    if manifest.interface != VERSION {
        return Err(rejected(Why::Interface {
            found: manifest.interface,
        }));
    }
    Ok(Installed { manifest, wasm })
}

/// Every folder of the install folder `plugins`, read. A missing
/// install folder holds no plugins.
pub fn scan(plugins: &Path) -> Vec<Result<Installed, Rejected>> {
    let Ok(entries) = std::fs::read_dir(plugins) else {
        return Vec::new();
    };
    let mut folders = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    folders.sort();
    folders.iter().map(|folder| read(folder)).collect()
}
