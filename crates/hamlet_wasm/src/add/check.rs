//! Reading the folder the chief chose: what it holds, what is copied
//! from it and whether it takes the place of a plugin that is there.

use std::path::{Path, PathBuf};

use hamlet_sdk::capability::ReadNative;
use hamlet_sdk::plugin::{is_native, plain};

use crate::install::{
    self, MANIFEST, PANEL, WASM, cap, manifest_text, only_data,
};
use crate::limits::Limits;
use crate::manifest::{Kind, Manifest};

/// The file that makes a folder the source of a plugin.
const CARGO: &str = "Cargo.toml";

/// A plugin already here, as the chief's folder is checked against
/// it.
#[derive(Clone, Debug)]
pub struct Known {
    pub id: String,
    pub name: String,
    pub version: String,
    pub builtin: bool,
    pub native: bool,
    pub installed: bool,
}

/// What a folder is made into.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Make {
    /// Its files are copied as they are.
    Copy,
    /// It is built first: the name of the library its build makes.
    Build(String),
}

/// A folder that may be added.
#[derive(Clone, Debug)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub version: String,
    pub native: bool,
    pub make: Make,
    /// The folder chosen, made canonical.
    pub folder: PathBuf,
    /// Each file to copy: its name in the plugin's folder, where it
    /// is read from and the most bytes it may have. The manifest is
    /// last.
    pub files: Vec<(String, PathBuf, u64)>,
    /// The plugin it takes the place of, in words, and whether that
    /// one is installed.
    pub replaces: Option<(String, bool)>,
}

/// What the folder `chosen` holds, checked as the scan checks a
/// folder of the install folder but for its name, or why it cannot be
/// added. `home` is Hamlet's home and `known` the plugins there.
pub fn read(
    chosen: &Path,
    home: &Path,
    limits: &Limits,
    known: &[Known],
    native: Option<ReadNative>,
) -> Result<Plan, String> {
    let folder = chosen
        .canonicalize()
        .map_err(|_| "the folder cannot be read".to_string())?;
    if !folder.is_dir() {
        return Err("what was chosen is not a folder".into());
    }
    let real_home =
        home.canonicalize().unwrap_or_else(|_| home.into());
    if folder.starts_with(&real_home)
        || real_home.starts_with(&folder)
    {
        return Err("the folder is in Hamlet's home, or holds it: \
                    choose the plugin's own folder"
            .into());
    }
    let text = manifest_text(&folder, limits)
        .map_err(|why| why.to_string())?;
    let mut plan = match is_native(&text) {
        true => native_plan(&folder, &text, native)?,
        false => plan(&folder, &text, limits)?,
    };
    plan.files.push((
        MANIFEST.into(),
        folder.join(MANIFEST),
        cap(MANIFEST, limits),
    ));
    plan.replaces = replaces(&plan, home, known)?;
    Ok(plan)
}

/// The plan for a folder of a plugin of WASM or a panel.
fn plan(
    folder: &Path,
    text: &str,
    limits: &Limits,
) -> Result<Plan, String> {
    let manifest = Manifest::parse(text).map_err(|e| {
        format!("{MANIFEST} cannot be read: {}", e.message())
    })?;
    let (manifest, _) = install::examine(manifest, None)
        .map_err(|why| why.to_string())?;
    let has = |file: &str| folder.join(file).is_file();
    let (make, files) = match manifest.kind {
        Kind::Data if has(PANEL) => (
            Make::Copy,
            vec![(
                PANEL.into(),
                folder.join(PANEL),
                cap(PANEL, limits),
            )],
        ),
        Kind::Data => {
            return Err(format!("the folder has no {PANEL}"));
        }
        Kind::Wasm if has(WASM) => (
            Make::Copy,
            vec![(WASM.into(), folder.join(WASM), cap(WASM, limits))],
        ),
        Kind::Wasm if has(CARGO) => {
            (Make::Build(library(folder, limits)?), Vec::new())
        }
        Kind::Wasm => {
            return Err(format!(
                "the folder has neither {WASM} nor {CARGO}: it is \
                 not a built plugin or the source of one"
            ));
        }
    };
    Ok(Plan {
        id: manifest.id,
        name: plain(&manifest.name),
        version: manifest.version,
        native: false,
        make,
        folder: folder.to_path_buf(),
        files,
        replaces: None,
    })
}

/// The plan for a folder of a native plugin.
fn native_plan(
    folder: &Path,
    text: &str,
    native: Option<ReadNative>,
) -> Result<Plan, String> {
    let read =
        native.ok_or("this Hamlet does not run native plugins")?;
    let contents = read(folder, text, false)?;
    Ok(Plan {
        files: contents
            .files
            .into_iter()
            .map(|(name, most)| {
                (name.clone(), folder.join(name), most)
            })
            .collect(),
        id: contents.id,
        name: contents.name,
        version: contents.version,
        native: true,
        make: Make::Copy,
        folder: folder.to_path_buf(),
        replaces: None,
    })
}

/// The name of the library the build of the source in `folder` makes,
/// as its `Cargo.toml` names it.
fn library(folder: &Path, limits: &Limits) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Cargo {
        package: Option<Named>,
        lib: Option<Lib>,
    }
    #[derive(serde::Deserialize)]
    struct Named {
        name: String,
    }
    #[derive(serde::Deserialize)]
    struct Lib {
        name: Option<String>,
    }
    let bytes = install::read_capped(
        &folder.join(CARGO),
        limits.manifest_bytes as u64,
    )
    .ok()
    .flatten()
    .ok_or_else(|| format!("{CARGO} cannot be read"))?;
    let cargo = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| toml::from_str::<Cargo>(text).ok())
        .ok_or_else(|| format!("{CARGO} cannot be read"))?;
    let package = cargo.package.ok_or_else(|| {
        format!(
            "{CARGO} names no package: choose the folder of one \
             plugin"
        )
    })?;
    let name =
        cargo.lib.and_then(|lib| lib.name).unwrap_or(package.name);
    Ok(name.replace('-', "_"))
}

/// The plugin `plan` takes the place of, or why it cannot: one built
/// in, or one of the other kind, has the id.
fn replaces(
    plan: &Plan,
    home: &Path,
    known: &[Known],
) -> Result<Option<(String, bool)>, String> {
    let id = &plan.id;
    let Some(there) = known.iter().find(|known| known.id == *id)
    else {
        let folder = home.join("plugins").join(id);
        let filled = folder.is_dir() && !only_data(&folder);
        return Ok(filled
            .then(|| (format!("what is in plugins/{id}"), false)));
    };
    if there.builtin {
        return Err(format!(
            "`{id}` is the id of a plugin built into Hamlet"
        ));
    }
    if there.native != plan.native {
        let kind = match there.native {
            true => "a native plugin",
            false => "a WASM plugin",
        };
        return Err(format!(
            "`{id}` is here as {kind}; uninstall it first to add \
             this one"
        ));
    }
    Ok(Some((
        format!("{} {}", there.name, there.version),
        there.installed,
    )))
}
