//! A scholar's or a lodge's papers.

use hamlet_git::{MAIN, Repository};
use hamlet_protocol::client::{self as protocol, Keeper};
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ScholarId};

use crate::knowledge::folder::Folder;
use crate::knowledge::library::field;
use crate::knowledge::{CHARACTER, lodging, outside_git};
use crate::realm::change::Done;
use crate::realm::layout::Layout;
use crate::society::refusal::Refusal;

/// The papers of `of`: a scholar's `main`, or an agent's lodge.
pub(crate) fn papers(
    db: &mut Db,
    layout: &Layout,
    of: Keeper,
) -> Done<protocol::Papers> {
    let (scholar, lodge) = match of {
        Keeper::Scholar(scholar) => {
            (ScholarId(i64::try_from(scholar.0)?), None)
        }
        Keeper::Agent(agent) => {
            let agent = AgentId(i64::try_from(agent.0)?);
            let Ok(lodging) = lodging(db, layout, agent) else {
                return Ok(Err(Refusal::NoOne {
                    address: agent.to_string(),
                }));
            };
            (lodging.scholar, Some(lodging.path))
        }
    };
    let repo = Repository::open(&layout.repository(scholar))?;
    let main = repo.tip(MAIN)?;
    let character = match main {
        Some(tip) => repo
            .read(tip, CHARACTER)?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default(),
        None => String::new(),
    };
    let mut files = Vec::<(String, Vec<u8>)>::new();
    let mut learned = Vec::new();
    match &lodge {
        Some(path) => {
            let Ok(folder) = Folder::open(path) else {
                return Ok(Ok(protocol::Papers {
                    character,
                    learned,
                    memory: Vec::new(),
                    skills: Vec::new(),
                }));
            };
            let listing = folder.list(&outside_git)?;
            let base = match main {
                Some(tip) => repo.entries(tip)?,
                None => Vec::new(),
            };
            for entry in &listing {
                if !base.contains(entry) {
                    learned.push(entry.path.clone());
                }
                files.push((
                    entry.path.clone(),
                    folder.read(&entry.path)?,
                ));
            }
            for entry in &base {
                if !outside_git(&entry.path)
                    && !listing.iter().any(|e| e.path == entry.path)
                {
                    learned.push(entry.path.clone());
                }
            }
        }
        None => {
            if let Some(tip) = main {
                files = repo.files(tip)?;
            }
        }
    }
    let mut memory = Vec::new();
    let mut skills = Vec::new();
    for (path, bytes) in files {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if let Some(rest) = path.strip_prefix("memory/") {
            memory.push(protocol::MemoryFile {
                description: field(&text, "description:"),
                path: rest.to_owned(),
                text,
            });
        } else if let Some(rest) = path.strip_prefix("skills/")
            && let Some((name, "SKILL.md")) = rest.split_once('/')
        {
            skills.push(protocol::Skill {
                description: field(&text, "description:"),
                name: name.to_owned(),
            });
        }
    }
    learned.sort();
    Ok(Ok(protocol::Papers {
        character,
        learned,
        memory,
        skills,
    }))
}
