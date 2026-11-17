//! What a scholar of the city made of a hire starts from (H38).

use anyhow::Result;
use hamlet_git::{MAIN, Repository};
use hamlet_store::Db;
use hamlet_store::ids::AgentId;

use super::worker::RepositoryWriteLock;
use super::{CHARACTER, branch};
use crate::realm::layout::Layout;
use crate::society::city::{self, Arrival};
use crate::society::hires;

/// What a scholar made of a hire starts from.
pub(crate) struct Source {
    pub name: String,
    /// The tip of the hire's branch, which a new scholar is forked
    /// from; none for a blank hire, whose own scholar joins the
    /// city.
    pub commit: Option<String>,
    pub character: String,
}

/// What the scholar made of `hire` starts from, named `name`, else
/// as the hire is called. A city scholar's hire has its lodge
/// committed first, so its branch's tip holds all it learned.
pub(crate) fn source(
    db: &mut Db,
    layout: &Layout,
    writing: &RepositoryWriteLock,
    hire: AgentId,
    name: &str,
) -> Result<Source> {
    let found = hires::get(db, hire)?;
    let scholar = city::get(db, found.scholar)?;
    let name = match name.trim() {
        "" => found.nickname.unwrap_or(scholar.name),
        given => given.to_owned(),
    };
    let held = writing.hold();
    // A scholar whose repository is not made yet has nothing to
    // fork, and a blank hire's joins with what its branch holds once
    // it is.
    let Ok(repository) =
        Repository::open(&layout.repository(found.scholar))
    else {
        return Ok(Source {
            name,
            commit: None,
            character: String::new(),
        });
    };
    let commit = if scholar.arrival == Arrival::Apart {
        None
    } else {
        super::run::commit(
            db,
            layout,
            hire,
            crate::app::clock::now(),
        )?;
        repository.tip(&branch(hire))?
    };
    let read_at = match commit {
        Some(commit) => Some(commit),
        None => repository.tip(MAIN)?,
    };
    let character = match read_at {
        Some(at) => {
            repository.read(at, CHARACTER)?.unwrap_or_default()
        }
        None => Vec::new(),
    };
    drop(held);
    Ok(Source {
        name,
        commit: commit.map(|commit| commit.to_string()),
        character: String::from_utf8_lossy(&character).into_owned(),
    })
}
