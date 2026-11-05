//! Running the jobs that keep repositories and lodges, each safe to
//! run again after a crash.

use anyhow::{Context, Result, bail};
use hamlet_git::{Author, Change, CommitId, MAIN, Repository, Step};
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, OfferId, ScholarId};

use super::folder::{Folder, remove_folder};
use super::{CHARACTER, branch, lodging, outside_git};
use crate::jobs::{Fork, Job};
use crate::layout::Layout;
use crate::society::{city, hires, library as shelf};

/// The longest a failed job waits before it runs again.
const MOST_WAIT: i64 = 300_000;

/// Runs every job due at `now`, once each. A job that fails runs
/// again later, waiting longer each time.
pub(crate) fn due(
    db: &mut Db,
    layout: &Layout,
    now: i64,
) -> Result<usize> {
    let jobs = hamlet_store::job::due(db, now)?;
    let count = jobs.len();
    for kept in jobs {
        let job = serde_json::from_str::<Job>(&kept.payload)
            .with_context(|| format!("reading job {}", kept.id))?;
        match run(db, layout, &job, now) {
            Ok(()) => hamlet_store::job::finish(db, kept.id, now)?,
            Err(error) => {
                let wait =
                    (1000 << kept.tries.min(16)).min(MOST_WAIT);
                hamlet_store::job::retry(
                    db,
                    kept.id,
                    &format!("{error:#}"),
                    now + wait,
                )?;
            }
        }
    }
    Ok(count)
}

/// Runs `job` once.
pub(crate) fn run(
    db: &mut Db,
    layout: &Layout,
    job: &Job,
    at: i64,
) -> Result<()> {
    match job {
        Job::Plant {
            scholar,
            character,
            fork,
        } => {
            plant(db, layout, *scholar, character, fork.as_ref(), at)
        }
        Job::Lodge { hire } => lodge(db, layout, *hire),
        Job::Commit { hire } => {
            commit(db, layout, *hire, at).map(drop)
        }
        Job::TakeIn { hire } => take_in(db, layout, *hire, at),
        Job::EraseHire { hire } => {
            let found = lodging(db, layout, *hire)?;
            remove_folder(&found.path)?;
            if let Ok(repo) =
                Repository::open(&layout.repository(found.scholar))
            {
                repo.delete_branch(&branch(*hire))?;
            }
            Ok(())
        }
        Job::EraseScholar { scholar } => {
            remove_folder(&layout.repository(*scholar))
        }
        Job::EraseHamlet { hamlet } => {
            remove_folder(&layout.hamlet(hamlet))
        }
        Job::Merge { merge } => {
            super::merging::merge(db, layout, *merge, at)
        }
        Job::Split { merge } => {
            super::merging::split(db, layout, *merge, at)
        }
        Job::Shelve { offer } => shelve(db, layout, *offer, at),
        Job::DreamEnd {
            dream,
            last,
            summary,
        } => super::dreaming::end(
            db, layout, *dream, *last, summary, at,
        ),
    }
}

/// Shelves the skill of `offer`, which the librarian took, once, and
/// takes it out of the offerer's lodge while it is as offered.
fn shelve(
    db: &mut Db,
    layout: &Layout,
    offer: OfferId,
    at: i64,
) -> Result<()> {
    let found = shelf::get(db, offer)?;
    let tree =
        found.tree.as_deref().context("the offer kept no files")?;
    let files = super::library::offered(layout, tree)?;
    if !super::library::has(layout, &found.skill)? {
        let by = Actor::Agent(found.agent).to_string();
        let commit = super::library::shelve(
            layout,
            &found.skill,
            &files,
            &format!("Shelved {}: {}", found.skill, found.reason),
            &by,
            at,
        )?;
        shelf::event(
            db,
            at,
            shelf::Event {
                kind: shelf::EventKind::Shelved,
                skill: &found.skill,
                agent: Some(found.agent),
                chief: None,
                reason: Some(&found.reason),
                commit: Some(&commit.to_string()),
            },
        )?;
    }
    let lodge = lodging(db, layout, found.agent)?.path;
    let Ok(folder) = Folder::open(&lodge) else {
        return Ok(());
    };
    let prefix = format!("skills/{}/", found.skill);
    let present = folder
        .list_under(&format!("skills/{}", found.skill))?
        .into_iter()
        .map(|entry| {
            Ok((
                entry.path[prefix.len()..].to_owned(),
                folder.read(&entry.path)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut offered = files;
    offered.sort();
    let mut present = present;
    present.sort();
    if present == offered {
        for (path, _) in &present {
            folder.remove(&format!("{prefix}{path}"))?;
        }
    }
    Ok(())
}

/// Makes the repository of `scholar`, from its character or as a
/// fork, in place of any half-made one.
fn plant(
    db: &mut Db,
    layout: &Layout,
    scholar: ScholarId,
    character: &str,
    fork: Option<&Fork>,
    at: i64,
) -> Result<()> {
    let path = layout.repository(scholar);
    if let Ok(repo) = Repository::open(&path)
        && repo.tip(MAIN)?.is_some()
    {
        return city::planted(db, scholar);
    }
    remove_folder(&path)?;
    let repo = Repository::create(&path)?;
    let name = Actor::Realm.to_string();
    let author = Author { name: &name, at };
    match fork {
        Some(fork) => {
            let from =
                Repository::open(&layout.repository(fork.from))?;
            let commit = fork
                .commit
                .parse::<CommitId>()
                .map_err(anyhow::Error::msg)?;
            repo.start_from(MAIN, &from, commit, "Forked", author)?;
        }
        None => {
            repo.commit(
                MAIN,
                &[Change::Write {
                    path: CHARACTER.into(),
                    bytes: character.as_bytes().to_vec(),
                }],
                "Made the scholar",
                author,
            )?;
        }
    }
    city::planted(db, scholar)
}

/// Starts the branch of `agent` when it has none, and checks out its
/// lodge.
fn lodge(db: &mut Db, layout: &Layout, agent: AgentId) -> Result<()> {
    let found = lodging(db, layout, agent)?;
    if !city::get(db, found.scholar)?.planted {
        bail!("its scholar's repository is not made yet");
    }
    let repo = Repository::open(&layout.repository(found.scholar))?;
    let branch = branch(agent);
    let tip = match repo.tip(&branch)? {
        Some(tip) => tip,
        None => {
            let start = target(&repo, found.pin.as_deref())?;
            repo.branch(&branch, start)?;
            if hires::find(db, agent)?.is_some() {
                hires::took_in(db, agent, &start.to_string())?;
            }
            start
        }
    };
    let folder = Folder::make(&found.path)?;
    check_out(&repo, tip, &folder)
}

/// What a branch takes in: its pin, or `main`.
fn target(repo: &Repository, pin: Option<&str>) -> Result<CommitId> {
    match pin {
        Some(version) => repo
            .resolve(version)?
            .with_context(|| format!("no version {version}")),
        None => repo.tip(MAIN)?.context("the scholar has no main"),
    }
}

/// Makes the folder hold the files of `commit`, but the character and
/// the inbox.
pub(super) fn check_out(
    repo: &Repository,
    commit: CommitId,
    folder: &Folder,
) -> Result<()> {
    let listing = folder.list(&outside_git)?;
    for step in repo.plan(commit, &listing, &outside_git)? {
        match step {
            Step::Write {
                path,
                blob,
                executable,
            } => {
                folder.write(&path, &repo.blob(blob)?, executable)?;
            }
            Step::Remove { path } => folder.remove(&path)?,
        }
    }
    Ok(())
}

/// Commits the lodge of `agent` to its branch, returning the new tip
/// when it changed.
pub(crate) fn commit(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
    at: i64,
) -> Result<Option<CommitId>> {
    let found = lodging(db, layout, agent)?;
    let Ok(folder) = Folder::open(&found.path) else {
        return Ok(None);
    };
    let repo = Repository::open(&layout.repository(found.scholar))?;
    let branch = branch(agent);
    if repo.tip(&branch)?.is_none() {
        return Ok(None);
    }
    let listing = folder.list(&outside_git)?;
    for entry in &listing {
        if !repo.has_blob(entry.blob) {
            repo.write_blob(&folder.read(&entry.path)?)?;
        }
    }
    let name = Actor::Agent(agent).to_string();
    repo.commit_listing(
        &branch,
        &listing,
        &|path| path == CHARACTER,
        &[],
        "Kept what it learned",
        Author { name: &name, at },
    )
}

/// Takes `main`, or the pin, into the branch of the idle `agent` and
/// writes what changed into its lodge.
fn take_in(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
    at: i64,
) -> Result<()> {
    let found = lodging(db, layout, agent)?;
    if !found.living {
        return Ok(());
    }
    commit(db, layout, agent, at)?;
    let repo = Repository::open(&layout.repository(found.scholar))?;
    let branch = branch(agent);
    if repo.tip(&branch)?.is_none() {
        return Ok(());
    }
    let into = target(&repo, found.pin.as_deref())?;
    let name = Actor::Realm.to_string();
    let merged = repo.merge(
        &branch,
        into,
        "Took in what its scholar learned",
        Author { name: &name, at },
    )?;
    check_out(&repo, merged.commit, &Folder::make(&found.path)?)?;
    if hires::find(db, agent)?.is_some() {
        hires::took_in(db, agent, &into.to_string())?;
    }
    Ok(())
}
