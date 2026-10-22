//! An agent, a hamlet or tools of the home, packed to be sold.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use hamlet_home::Home;
use hamlet_model::market::package::{
    self, Agent, CHARACTER, File, Hamlet as Packed, NOTES, SKILLS,
    Tool, Tools, check_name,
};
use sha2::{Digest, Sha256};

use crate::hamlet::Hamlet;
use crate::realm::Realm;

/// The agent `name` of `hamlet` with its character and skills, and
/// its notes when `notes`. One with no purpose is given one, as the
/// bazaar's server takes none without.
pub fn pack_agent(
    hamlet: &Hamlet,
    name: &str,
    notes: bool,
) -> Result<Agent> {
    let resident = hamlet
        .members()
        .into_iter()
        .find(|resident| resident.name == name)
        .with_context(|| format!("no agent named `{name}`"))?;
    check_name(name)?;
    let house = hamlet.house(name)?;
    let purpose = match resident.purpose.trim() {
        "" if hamlet.mayor().name == name => {
            format!("The mayor of {}", shown(hamlet))
        }
        "" => format!("An agent of {}", shown(hamlet)),
        purpose => purpose.to_string(),
    };
    Ok(Agent {
        name: name.to_string(),
        purpose,
        model: hamlet.model_of(name),
        files: house_files(&house, notes)?,
    })
}

/// The name `hamlet` is shown under.
fn shown(hamlet: &Hamlet) -> String {
    Realm::new(hamlet.home().to_path_buf())
        .hamlets()
        .into_iter()
        .find(|place| place.name == hamlet.name())
        .map_or_else(
            || hamlet.name().to_string(),
            |place| place.shown().to_string(),
        )
}

fn house_files(house: &Home, notes: bool) -> Result<Vec<File>> {
    let mut files = Vec::new();
    if let Some(file) = one(&house.character(), CHARACTER)? {
        files.push(file);
    }
    if notes && let Some(file) = one(&house.notes(), NOTES)? {
        files.push(file);
    }
    let skills = house.skills();
    for skill in folders(&skills)? {
        check_name(&skill).with_context(|| {
            format!("the skill folder `{skill}` cannot be sold")
        })?;
        let prefix = format!("{SKILLS}{skill}/");
        walk(&skills.join(&skill), &prefix, &mut files)?;
    }
    Ok(files)
}

/// The hamlet `name` whole: its mayor and agents with all they know,
/// their notes too when `notes`. `kind` says what kind of hamlet it
/// is. One whose mayor has no purpose is given its title for one.
pub fn pack_hamlet(
    realm: &Realm,
    name: &str,
    kind: &str,
    notes: bool,
) -> Result<Packed> {
    check_name(name)?;
    let hamlet = realm.hamlet(name)?;
    let place = realm
        .hamlets()
        .into_iter()
        .find(|place| place.name == name)
        .with_context(|| format!("no hamlet named `{name}`"))?;
    let mayor = hamlet.mayor();
    let agents = hamlet
        .residents()
        .iter()
        .map(|resident| pack_agent(&hamlet, &resident.name, notes))
        .collect::<Result<Vec<_>>>()?;
    Ok(Packed {
        name: name.to_string(),
        title: place.shown().to_string(),
        kind: kind.trim().to_string(),
        purpose: match mayor.purpose.trim() {
            "" => place.shown().to_string(),
            purpose => purpose.to_string(),
        },
        seed: seed(name).to_string(),
        mayor: pack_agent(&hamlet, &mayor.name, notes)?,
        agents,
    })
}

/// A seed for the looks of the town of a hamlet sold, which shows in
/// the store.
fn seed(name: &str) -> u64 {
    let hash = Sha256::digest(name.as_bytes());
    let bytes = <[u8; 8]>::try_from(&hash[..8]).unwrap_or_default();
    // Small enough to stay whole as a number in JavaScript.
    u64::from_le_bytes(bytes) >> 12
}

/// A tool in an agent's house.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolAt {
    pub hamlet: String,
    pub agent: String,
    pub name: String,
    /// Its folder.
    pub dir: PathBuf,
}

/// The tools of every agent's house in the realm that can be sold.
pub fn tools(realm: &Realm) -> Vec<ToolAt> {
    let mut all = Vec::new();
    for place in realm.hamlets() {
        let Ok(hamlet) = realm.hamlet(&place.name) else {
            continue;
        };
        for agent in hamlet.members() {
            let Ok(house) = hamlet.house(&agent.name) else {
                continue;
            };
            for name in folders(&house.tools()).unwrap_or_default() {
                let dir = house.tools().join(&name);
                if check_name(&name).is_ok()
                    && dir.join(package::MANIFEST).is_file()
                {
                    all.push(ToolAt {
                        hamlet: place.name.clone(),
                        agent: agent.name.clone(),
                        name,
                        dir,
                    });
                }
            }
        }
    }
    all
}

/// The tools in the folders `dirs`, each named for its folder, with
/// every file in it.
pub fn pack_tools(dirs: &[PathBuf]) -> Result<Tools> {
    if dirs.is_empty() {
        bail!("choose a tool to sell");
    }
    let mut tools = Vec::<Tool>::new();
    for dir in dirs {
        let name = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        check_name(&name)?;
        if tools.iter().any(|tool| tool.name == name) {
            bail!("two tools are named `{name}`");
        }
        if !dir.join(package::MANIFEST).is_file() {
            bail!("the tool `{name}` has no manifest.json");
        }
        let mut files = Vec::new();
        walk(dir, "", &mut files)?;
        tools.push(Tool { name, files });
    }
    Ok(Tools { tools })
}

/// The folders in `dir`, by name, hidden ones left out.
fn folders(dir: &Path) -> Result<Vec<String>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') && entry.file_type()?.is_dir() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// The file at `path` as `named` in a package, if it is there.
fn one(path: &Path, named: &str) -> Result<Option<File>> {
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(File {
        path: named.to_string(),
        data: std::fs::read(path)?,
        executable: executable(path),
    }))
}

/// Every file under `dir`, each named `prefix` and its path under
/// `dir`. Hidden files and links are left out.
fn walk(
    dir: &Path,
    prefix: &str,
    files: &mut Vec<File>,
) -> Result<()> {
    let mut entries = std::fs::read_dir(dir)?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type()?;
        if name.starts_with('.') || kind.is_symlink() {
            continue;
        }
        let path = format!("{prefix}{name}");
        if kind.is_dir() {
            walk(&entry.path(), &format!("{path}/"), files)?;
        } else if kind.is_file() {
            if !package::safe(&path) {
                bail!("`{path}` cannot be sold: rename it");
            }
            files.push(File {
                path,
                data: std::fs::read(entry.path())?,
                executable: executable(&entry.path()),
            });
        }
    }
    Ok(())
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    false
}
