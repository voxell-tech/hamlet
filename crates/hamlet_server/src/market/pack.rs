//! An agent, a hamlet or tools of the home, packed to be sold at the
//! bazaar's server.

use std::path::Path;

use anyhow::Result;
use hamlet_protocol::client::{Address, PackableTool, Packing};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::package::{
    Agent, CHARACTER, Contents, File, Hamlet, MANIFEST, NOTES,
    Package, SKILLS, Tool, Tools, check_name,
};
use crate::protocol::{Refusable, Refusal};
use crate::record;
use crate::service::Service;

/// The most bytes and files of a house's skills packed.
const SKILL_BYTES: u64 = 10 * 1024 * 1024;
const SKILL_FILES: usize = 4096;

/// Why something of the home was not packed for sale.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error,
)]
pub(crate) enum Unpacked {
    #[error(
        "`{0}` is not a name the bazaar takes: lowercase letters, \
         digits, `-` and `_`, at most 32"
    )]
    Name(String),
    #[error("no tool added by hand is named `{0}`")]
    NoTool(String),
    #[error("the package is too large: {0}")]
    TooLarge(String),
}

/// The package of `packing`, as the bazaar's server takes it.
pub(crate) async fn pack(
    service: &mut Service,
    packing: &Packing,
) -> Result<Refusable<Vec<u8>>> {
    let contents = match packing {
        Packing::Agent { agent, notes } => {
            match pack_agent(service, agent, *notes).await? {
                Ok(agent) => Contents::Agent { agent },
                Err(refusal) => return Ok(Err(refusal)),
            }
        }
        Packing::Hamlet {
            hamlet,
            kind,
            notes,
        } => {
            match pack_hamlet(service, hamlet, kind, *notes).await? {
                Ok(hamlet) => Contents::Hamlet { hamlet },
                Err(refusal) => return Ok(Err(refusal)),
            }
        }
        Packing::Tools { tools } => {
            match pack_tools(service.home.layout().root(), tools)? {
                Ok(tools) => Contents::Tools { tools },
                Err(unpacked) => {
                    return Ok(Err(Refusal::Unpacked(unpacked)));
                }
            }
        }
    };
    let package = Package::new(contents);
    if let Err(error) = package.check() {
        return Ok(Err(Refusal::Unpacked(Unpacked::Name(
            error.to_string(),
        ))));
    }
    Ok(package.bytes().map_err(|error| {
        Refusal::Unpacked(Unpacked::TooLarge(error.to_string()))
    }))
}

/// The agent at `address` with its character and skills, its notes
/// too with `notes`. One with no purpose is given one, as the
/// bazaar's server takes none without.
async fn pack_agent(
    service: &mut Service,
    address: &Address,
    notes: bool,
) -> Result<Refusable<Agent>> {
    let agent = match service.agent(address, "pack an agent")? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let Some(record) =
        service.home.store().record(&address.hamlet)?
    else {
        return Ok(Err(Refusal::NoHamlet(address.hamlet.clone())));
    };
    packed(service, &record, &agent, notes).await
}

async fn packed(
    service: &Service,
    record: &record::Hamlet,
    agent: &record::Agent,
    notes: bool,
) -> Result<Refusable<Agent>> {
    if check_name(&agent.name).is_err() {
        return Ok(Err(Refusal::Unpacked(Unpacked::Name(
            agent.name.clone(),
        ))));
    }
    let hamlet = record.id.as_str();
    let mut files = Vec::new();
    if !agent.character.trim().is_empty() {
        files.push(File {
            path: CHARACTER.into(),
            data: agent.character.clone().into_bytes(),
            executable: false,
        });
    }
    if notes {
        let text =
            service.houses.read_notes(hamlet, agent.number).await?;
        files.push(File {
            path: NOTES.into(),
            data: text.into_bytes(),
            executable: false,
        });
    }
    let house = service.houses.house(hamlet, agent.number)?;
    if house.kind("skills").await?.is_some() {
        let skills = house.directory("skills", false).await?;
        for (path, entry) in
            skills.read(SKILL_BYTES, SKILL_FILES).await?
        {
            let Some((skill, _)) = path.split_once('/') else {
                continue;
            };
            if check_name(skill).is_err() {
                return Ok(Err(Refusal::Unpacked(Unpacked::Name(
                    skill.into(),
                ))));
            }
            files.push(File {
                path: format!("{SKILLS}{path}"),
                data: entry.bytes,
                executable: entry.mode & 0o111 != 0,
            });
        }
    }
    let purpose = match agent.purpose.trim() {
        "" if agent.number == record.mayor => {
            format!("The mayor of {hamlet}")
        }
        "" => format!("An agent of {hamlet}"),
        purpose => purpose.into(),
    };
    Ok(Ok(Agent {
        name: agent.name.clone(),
        purpose,
        model: agent.model.clone(),
        files,
    }))
}

/// The hamlet `name` whole: its mayor and agents with all they know,
/// their notes too with `notes`.
async fn pack_hamlet(
    service: &mut Service,
    name: &str,
    kind: &str,
    notes: bool,
) -> Result<Refusable<Hamlet>> {
    if let Err(refusal) = service.access(name, "pack a hamlet")? {
        return Ok(Err(refusal));
    }
    let Some(record) = service.home.store().record(name)? else {
        return Ok(Err(Refusal::NoHamlet(name.into())));
    };
    if check_name(name).is_err() {
        return Ok(Err(Refusal::Unpacked(Unpacked::Name(
            name.into(),
        ))));
    }
    let title = crate::realm::read(service.db())?
        .hamlets
        .into_iter()
        .find(|place| place.id == name)
        .and_then(|place| place.title)
        .unwrap_or_else(|| name.into());
    let mut mayor = None;
    let mut agents = Vec::new();
    for agent in &record.agents {
        let agent =
            match packed(service, &record, agent, notes).await? {
                Ok(packed) => (agent.number, packed),
                Err(refusal) => return Ok(Err(refusal)),
            };
        if agent.0 == record.mayor {
            mayor = Some(agent.1);
        } else {
            agents.push(agent.1);
        }
    }
    let Some(mayor) = mayor else {
        anyhow::bail!(
            "the hamlet `{name}` has no mayor in its record"
        )
    };
    Ok(Ok(Hamlet {
        name: name.into(),
        purpose: mayor.purpose.clone(),
        title,
        kind: kind.trim().into(),
        seed: seed(name).to_string(),
        mayor,
        agents,
    }))
}

/// A seed for the looks of the town of a hamlet sold, which shows at
/// the bazaar.
fn seed(name: &str) -> u64 {
    let hash = Sha256::digest(name.as_bytes());
    let bytes = <[u8; 8]>::try_from(&hash[..8]).unwrap_or_default();
    // Small enough to stay whole as a number in JavaScript.
    u64::from_le_bytes(bytes) >> 12
}

/// The tools added by hand that load and can be sold, by name.
pub(crate) fn tools(root: &Path) -> Vec<PackableTool> {
    crate::hand::load(root)
        .0
        .into_iter()
        .filter(|tool| check_name(&tool.spec.name).is_ok())
        .map(|tool| PackableTool {
            description: tool.spec.description.clone(),
            name: tool.spec.name.clone(),
        })
        .collect()
}

/// The tools added by hand named `names`, each with every file of its
/// folder.
fn pack_tools(
    root: &Path,
    names: &[String],
) -> Result<Result<Tools, Unpacked>> {
    let loaded = crate::hand::load(root).0;
    let mut tools = Vec::new();
    for name in names {
        if check_name(name).is_err() {
            return Ok(Err(Unpacked::Name(name.clone())));
        }
        let Some(tool) =
            loaded.iter().find(|tool| &tool.spec.name == name)
        else {
            return Ok(Err(Unpacked::NoTool(name.clone())));
        };
        let mut files = Vec::new();
        walk(tool.dir(), "", &mut files)?;
        // The server takes a tool whose manifest names it.
        if !files.iter().any(|file| file.path == MANIFEST) {
            return Ok(Err(Unpacked::NoTool(name.clone())));
        }
        tools.push(Tool {
            name: name.clone(),
            files,
        });
    }
    Ok(Ok(Tools { tools }))
}

/// Every regular file under `dir`, by its path after `prefix`, hidden
/// ones and links left out.
fn walk(
    dir: &Path,
    prefix: &str,
    files: &mut Vec<File>,
) -> Result<()> {
    let mut entries = std::fs::read_dir(dir)?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let kind = entry.file_type()?;
        let path = format!("{prefix}{name}");
        if kind.is_dir() {
            walk(&entry.path(), &format!("{path}/"), files)?;
        } else if kind.is_file() {
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                entry.metadata()?.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            files.push(File {
                path,
                data: std::fs::read(entry.path())?,
                executable,
            });
        }
    }
    Ok(())
}
