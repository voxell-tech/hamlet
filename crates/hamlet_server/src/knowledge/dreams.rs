//! The repositories' side of a dream.

use std::path::PathBuf;

use anyhow::{Context, Result};
use hamlet_git::{Author, MAIN, Repository};
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, DreamId};

use super::folder::{Folder, remove_folder};
use super::run::{check_out, commit};
use super::{CHARACTER, branch, outside_git};
use crate::realm::jobs::Job;
use crate::realm::layout::Layout;
use crate::society::dreams::{self, DreamKind, Outcome};
use crate::society::{addresses, hamlet_agents};

/// The checkout of the profile's `main` that dream `id` writes.
pub(crate) fn checkout(layout: &Layout, id: DreamId) -> PathBuf {
    layout
        .root()
        .join("dreams")
        .join(id.to_string())
        .join("main")
}

/// Whether the branch of `agent` holds what its profile's `main`
/// lacks and its last dream did not already take home.
pub(crate) fn has_learned(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
) -> Result<bool> {
    let found = hamlet_agents::get(db, agent)?;
    let Ok(repo) =
        Repository::open(&layout.repository(found.profile))
    else {
        return Ok(false);
    };
    let (Some(tip), Some(main)) =
        (repo.tip(&branch(agent))?, repo.tip(MAIN)?)
    else {
        return Ok(false);
    };
    let dreamt = dreams::last_of_agent(db, agent)?
        .and_then(|dream| dream.branch_commit)
        == Some(tip.to_string());
    Ok(!dreamt && repo.merge_base(tip, main)? != Some(tip))
}

/// Readies dream `id`; `false` when it has nothing to dream about.
pub(crate) fn prepare(
    db: &mut Db,
    layout: &Layout,
    id: DreamId,
    at: i64,
) -> Result<bool> {
    let dream = dreams::get(db, id)?;
    let repo = Repository::open(&layout.repository(dream.profile))?;
    let main = repo.tip(MAIN)?.context("the profile has no main")?;
    if let Some(agent) = dream.agent {
        commit(db, layout, agent, at)?;
        let Some(tip) = repo.tip(&branch(agent))? else {
            return Ok(false);
        };
        if repo.merge_base(tip, main)? == Some(tip) {
            return Ok(false);
        }
    }
    let path = checkout(layout, id);
    remove_folder(&path)?;
    check_out(&repo, main, &Folder::make(&path)?)?;
    // Git keeps no empty folder. With these, a relative `memory/` or
    // `skills/` is the checkout's, never the worktree's beside it.
    for part in ["memory", "skills"] {
        Folder::make(&path.join(part))?;
    }
    Ok(true)
}

/// Ends dream `id` once, committing what it left; `last` is how its
/// turn ended.
pub(super) fn end(
    db: &mut Db,
    layout: &Layout,
    id: DreamId,
    last: Outcome,
    summary: &str,
    at: i64,
) -> Result<()> {
    let dream = dreams::get(db, id)?;
    if dream.ended.is_some() {
        return Ok(());
    }
    let path = checkout(layout, id);
    if last != Outcome::Done {
        remove_folder(&path)?;
        return dreams::end(db, id, last, None, None, at);
    }
    let repo = Repository::open(&layout.repository(dream.profile))?;
    let before =
        repo.tip(MAIN)?.context("the profile has no main")?;
    let folder = Folder::open(&path)?;
    let listing = folder.list(&outside_git)?;
    for entry in &listing {
        if !repo.has_blob(entry.blob) {
            repo.write_blob(&folder.read(&entry.path)?)?;
        }
    }
    let author = format!("dream:{id}");
    let message = match (dream.kind, dream.agent) {
        (DreamKind::Agent, Some(agent)) => format!(
            "Dream of {}: {summary}",
            addresses::name_of(db, agent)?
        ),
        _ => format!("Dream: {summary}"),
    };
    let after = repo.commit_listing(
        MAIN,
        &listing,
        &|path| path == CHARACTER,
        &[],
        &message,
        Author { name: &author, at },
    )?;
    let main = after.filter(|tip| *tip != before);
    let mut branch_tip = None;
    if let Some(agent) = dream.agent {
        commit(db, layout, agent, at)?;
        if hamlet_agents::find(db, agent)?.is_some() {
            super::run::run(db, layout, &Job::TakeIn { agent }, at)?;
        }
        branch_tip = repo.tip(&branch(agent))?;
    }
    if main.is_some() {
        for other in hamlet_agents::living_of(db, dream.profile)? {
            if Some(other.agent) != dream.agent {
                let job = Job::TakeIn { agent: other.agent };
                hamlet_store::job::add(
                    db,
                    job.kind(),
                    Some(&job.key()),
                    &job,
                    at,
                )?;
            }
        }
    }
    remove_folder(&path)?;
    let outcome = if main.is_some() {
        Outcome::Done
    } else {
        Outcome::Nothing
    };
    dreams::end(
        db,
        id,
        outcome,
        main.map(|tip| tip.to_string()).as_deref(),
        branch_tip.map(|tip| tip.to_string()).as_deref(),
        at,
    )
}
