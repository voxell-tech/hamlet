//! The install folder: `plugins/<id>/manifest.toml` and the plugin's
//! `plugin.wasm`, or its `panel.ui`, in the home.

use std::fmt;
use std::path::{Path, PathBuf};

use hamlet_sdk::capability::{Asks, Refusal};
use hamlet_sdk::plugin::needs;

use crate::db::DB;
use crate::files::FILES as KEPT;
use crate::interface::VERSION;
use crate::manifest::{Kind, Manifest};
use crate::storage::DATA;

pub const MANIFEST: &str = "manifest.toml";
const WASM: &str = "plugin.wasm";
const PANEL: &str = "panel.ui";

/// The files of a plugin's folder that make up a version of it.
pub const FILES: [&str; 3] = [MANIFEST, WASM, PANEL];

/// A plugin whose folder passed the scan.
#[derive(Debug)]
pub struct Installed {
    pub manifest: Manifest,
    /// What the manifest asks for, every ask one that can be
    /// granted.
    pub asks: Asks,
    pub body: Body,
}

/// The file a plugin is made of.
#[derive(Debug)]
pub enum Body {
    /// The component's file.
    Wasm(PathBuf),
    /// The panel's file.
    Data(PathBuf),
}

impl Body {
    /// The plugin's folder.
    pub fn folder(&self) -> PathBuf {
        match self {
            Body::Wasm(file) | Body::Data(file) => file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        }
    }
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
    /// The manifest asks for what cannot be granted.
    Asks(Refusal),
    /// The plugin needs a plugin that needs it, through the plugins
    /// of this path.
    Cycle(Vec<String>),
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
                "built for interface {found}, this Hamlet speaks \
                 {VERSION}"
            ),
            Why::Asks(refusal) => write!(f, "{refusal}"),
            Why::Cycle(path) => write!(
                f,
                "its needs go in a circle: {}",
                path.join(" -> ")
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
    let manifest = Manifest::parse(&text).map_err(|e| {
        rejected(Why::BadManifest(e.message().into()))
    })?;
    let file = |file: &'static str| {
        let path = folder.join(file);
        path.is_file()
            .then_some(path)
            .ok_or_else(|| rejected(Why::Missing(file)))
    };
    let body = match manifest.kind {
        Kind::Wasm => Body::Wasm(file(WASM)?),
        Kind::Data => Body::Data(file(PANEL)?),
    };
    if manifest.id != name {
        return Err(rejected(Why::WrongFolder { id: manifest.id }));
    }
    if manifest.kind == Kind::Wasm
        && manifest.interface.as_deref() != Some(VERSION)
    {
        return Err(rejected(Why::Interface {
            found: manifest
                .interface
                .unwrap_or_else(|| "none".into()),
        }));
    }
    if let Some(why) = manifest.problem() {
        return Err(rejected(Why::BadManifest(why)));
    }
    let asks = manifest
        .asked()
        .map_err(|refusal| rejected(Why::Asks(refusal)))?;
    Ok(Installed {
        manifest,
        asks,
        body,
    })
}

/// Whether `folder` holds nothing but the data and files of a plugin
/// that was removed with its data kept.
fn only_data(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok_and(|entries| {
        let mut names = entries
            .filter_map(|entry| Some(entry.ok()?.file_name()))
            .peekable();
        names.peek().is_some()
            && names.all(|name| {
                name == DATA || name == KEPT || name == DB
            })
    })
}

/// Every folder of the install folder `plugins`, read, the plugins
/// whose needs go in a circle turned down. A missing install folder
/// holds no plugins.
pub fn scan(plugins: &Path) -> Vec<Result<Installed, Rejected>> {
    let Ok(entries) = std::fs::read_dir(plugins) else {
        return Vec::new();
    };
    let mut folders = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && !only_data(path))
        .collect::<Vec<_>>();
    folders.sort();
    let found = folders.iter().map(|folder| read(folder));
    turn_down_circles(found.collect())
}

/// `found`, with each plugin that needs itself through the others
/// turned down.
fn turn_down_circles(
    found: Vec<Result<Installed, Rejected>>,
) -> Vec<Result<Installed, Rejected>> {
    let circles = {
        let edges = found
            .iter()
            .filter_map(|found| found.as_ref().ok())
            .map(|plugin| {
                (
                    plugin.manifest.id.as_str(),
                    plugin.manifest.needs.as_slice(),
                )
            })
            .collect::<Vec<_>>();
        found
            .iter()
            .map(|found| {
                let plugin = found.as_ref().ok()?;
                needs::cycle_through(&plugin.manifest.id, &edges)
            })
            .collect::<Vec<_>>()
    };
    found
        .into_iter()
        .zip(circles)
        .map(|(found, circle)| match (found, circle) {
            (Ok(plugin), Some(path)) => Err(Rejected {
                folder: plugin.manifest.id,
                why: Why::Cycle(path),
            }),
            (found, _) => found,
        })
        .collect()
}
