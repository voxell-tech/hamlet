//! What a profile of the city made of an agent starts from (H38).

use anyhow::Result;
use hamlet_git::{MAIN, Repository};
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChiefId, ProfileId};

use super::worker::RepositoryWriteLock;
use super::{CHARACTER, branch};
use crate::realm::change::{Change, Done, done};
use crate::realm::layout::Layout;
use crate::society::city::{self, Arrival};
use crate::society::hamlet_agents;

pub(crate) struct Made {
    pub profile: ProfileId,
    pub character: String,
}

/// The change that makes a profile of the city of what `agent`
/// learned, read now.
pub(crate) fn make_profile(
    db: &mut Db,
    layout: &Layout,
    writing: &RepositoryWriteLock,
    agent: AgentId,
    name: &str,
    owner: ChiefId,
    reason: String,
) -> Result<impl FnOnce(&mut Change<'_>) -> Done<Made> + use<>> {
    let source = source(db, layout, writing, agent, name)?;
    Ok(move |cx: &mut Change<'_>| {
        let profile = done!(city::make_of_agent(
            cx,
            agent,
            &source.name,
            owner,
            source.commit,
            &reason,
        ));
        Ok(Ok(Made {
            profile,
            character: source.character,
        }))
    })
}

/// What a profile made of an agent starts from.
struct Source {
    name: String,
    /// The tip of the agent's branch to fork; none for a blank
    /// agent.
    commit: Option<String>,
    character: String,
}

fn source(
    db: &mut Db,
    layout: &Layout,
    writing: &RepositoryWriteLock,
    agent: AgentId,
    name: &str,
) -> Result<Source> {
    let found = hamlet_agents::get(db, agent)?;
    let profile = city::get(db, found.profile)?;
    let name = match name.trim() {
        "" => found.name.unwrap_or(profile.name),
        given => given.to_owned(),
    };
    let held = writing.hold();
    // A profile whose repository is not made yet has nothing to
    // fork, and a blank agent's joins with what its branch holds once
    // it is.
    let Ok(repository) =
        Repository::open(&layout.repository(found.profile))
    else {
        return Ok(Source {
            name,
            commit: None,
            character: String::new(),
        });
    };
    let commit = if profile.arrival == Arrival::Apart {
        None
    } else {
        // So the branch's tip holds all it learned.
        super::run::commit(
            db,
            layout,
            agent,
            crate::app::clock::now(),
        )?;
        repository.tip(&branch(agent))?
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
