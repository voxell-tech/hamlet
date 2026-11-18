//! Running the jobs that keep repositories and worktrees, each safe
//! to run again after a crash.

use anyhow::{Context, Result, bail};
use hamlet_git::{Author, Change, CommitId, MAIN, Repository, Step};
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, OfferId, ProfileId};

use super::folder::{Folder, remove_folder};
use super::worker::RepositoryWriteLock;
use super::{CHARACTER, branch, outside_git, worktree};
use crate::realm::jobs::{Fork, Job};
use crate::realm::layout::Layout;
use crate::society::{city, hamlet_agents, library as shelf};

/// The longest a failed job waits before it runs again.
const MOST_WAIT: i64 = 300_000;

/// Runs every job due at `now`, each holding `writing`, returning
/// those done.
pub(crate) fn due(
    db: &mut Db,
    layout: &Layout,
    writing: &RepositoryWriteLock,
    now: i64,
) -> Result<Vec<Job>> {
    let jobs = hamlet_store::job::due(db, now)?;
    let mut done = Vec::new();
    for kept in jobs {
        let _held = writing.hold();
        let ran = serde_json::from_str::<Job>(&kept.payload)
            .with_context(|| format!("reading job {}", kept.id))
            .and_then(|job| {
                let ran = std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| {
                        run(db, layout, &job, now)
                    }),
                );
                match ran {
                    Ok(ran) => ran.map(|()| job),
                    Err(panic) => {
                        // What the job left open goes, or the pass
                        // ends and the runner opens the store again.
                        hamlet_store::unwind(db)?;
                        bail!(
                            "{}",
                            crate::app::failure::panicked(&*panic)
                        )
                    }
                }
            });
        match ran {
            Ok(job) => {
                hamlet_store::job::finish(db, kept.id)?;
                done.push(job);
            }
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
    Ok(done)
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
            profile,
            character,
            fork,
        } => {
            plant(db, layout, *profile, character, fork.as_ref(), at)
        }
        Job::Worktree { agent } => make_worktree(db, layout, *agent),
        Job::Commit { agent } => {
            commit(db, layout, *agent, at).map(drop)
        }
        Job::TakeIn { agent } => take_in(db, layout, *agent, at),
        Job::EraseAgent { agent } => {
            let found = worktree(db, layout, *agent)?;
            remove_folder(&found.path)?;
            if let Ok(repo) =
                Repository::open(&layout.repository(found.profile))
            {
                repo.delete_branch(&branch(*agent))?;
            }
            Ok(())
        }
        Job::EraseProfile { profile } => {
            remove_folder(&layout.repository(*profile))
        }
        Job::EraseHamlet { hamlet } => {
            remove_folder(&layout.hamlet(hamlet))
        }
        Job::Merge { merge } => {
            super::merges::merge(db, layout, *merge, at)
        }
        Job::Split { merge } => {
            super::merges::split(db, layout, *merge, at)
        }
        Job::BringHome { agent } => {
            bring_home(db, layout, *agent, at)
        }
        Job::Shelve { offer } => shelve(db, layout, *offer, at),
        Job::DreamEnd {
            dream,
            last,
            summary,
        } => {
            super::dreams::end(db, layout, *dream, *last, summary, at)
        }
    }
}

/// Shelves the skill of `offer` once, and takes it out of the
/// offerer's worktree while it is as offered.
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
    let worktree = worktree(db, layout, found.agent)?.path;
    let Ok(folder) = Folder::open(&worktree) else {
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

/// Makes the repository of `profile`, from its character or as a
/// fork, in place of any half-made one.
fn plant(
    db: &mut Db,
    layout: &Layout,
    profile: ProfileId,
    character: &str,
    fork: Option<&Fork>,
    at: i64,
) -> Result<()> {
    let path = layout.repository(profile);
    if let Ok(repo) = Repository::open(&path)
        && repo.tip(MAIN)?.is_some()
    {
        return city::planted(db, profile);
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
                "Made the profile",
                author,
            )?;
        }
    }
    city::planted(db, profile)
}

/// Starts the branch of `agent` when it has none, and checks out its
/// worktree.
fn make_worktree(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
) -> Result<()> {
    let found = worktree(db, layout, agent)?;
    if !city::get(db, found.profile)?.planted {
        bail!("its profile's repository is not made yet");
    }
    let repo = Repository::open(&layout.repository(found.profile))?;
    let branch = branch(agent);
    let tip = match repo.tip(&branch)? {
        Some(tip) => tip,
        None => {
            let start = target(&repo, found.pin.as_deref())?;
            repo.branch(&branch, start)?;
            if hamlet_agents::find(db, agent)?.is_some() {
                hamlet_agents::took_in(
                    db,
                    agent,
                    &start.to_string(),
                )?;
            }
            start
        }
    };
    let folder = Folder::make(&found.path)?;
    check_out(&repo, tip, &folder)
}

/// The commit a branch takes in: its pin's, or `main`'s.
fn target(repo: &Repository, pin: Option<&str>) -> Result<CommitId> {
    match pin {
        Some(version) => repo
            .resolve(version)?
            .with_context(|| format!("no version {version}")),
        None => repo.tip(MAIN)?.context("the profile has no main"),
    }
}

/// Makes the folder hold the files of `commit`, but the character and
/// the inbox.
pub(super) fn check_out(
    repo: &Repository,
    commit: CommitId,
    folder: &Folder,
) -> Result<()> {
    write_tree(repo, commit, folder, &outside_git)
}

/// Makes the folder hold the files of `commit`, leaving the paths
/// `outside` names alone.
pub(super) fn write_tree(
    repo: &Repository,
    commit: CommitId,
    folder: &Folder,
    outside: &dyn Fn(&str) -> bool,
) -> Result<()> {
    let listing = folder.list(outside)?;
    for step in repo.plan(commit, &listing, outside)? {
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

/// Commits the worktree of `agent` to its branch, returning the new
/// tip when it changed.
pub(crate) fn commit(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
    at: i64,
) -> Result<Option<CommitId>> {
    let found = worktree(db, layout, agent)?;
    let Ok(folder) = Folder::open(&found.path) else {
        return Ok(None);
    };
    let repo = Repository::open(&layout.repository(found.profile))?;
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

/// Merges the branch of `agent`, its worktree committed, into its
/// profile's `main`, and takes the new `main` in.
fn bring_home(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
    at: i64,
) -> Result<()> {
    let found = worktree(db, layout, agent)?;
    commit(db, layout, agent, at)?;
    let repo = Repository::open(&layout.repository(found.profile))?;
    let Some(tip) = repo.tip(&branch(agent))? else {
        return Ok(());
    };
    let name = Actor::Realm.to_string();
    repo.merge(
        MAIN,
        tip,
        &format!(
            "Brought home what {} learned",
            crate::society::addresses::name_of(db, agent)?
        ),
        Author { name: &name, at },
    )?;
    take_in(db, layout, agent, at)
}

/// Takes `main`, or the pin, into the branch of the idle `agent` and
/// writes what changed into its worktree.
fn take_in(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
    at: i64,
) -> Result<()> {
    let found = worktree(db, layout, agent)?;
    if !found.living {
        return Ok(());
    }
    commit(db, layout, agent, at)?;
    let repo = Repository::open(&layout.repository(found.profile))?;
    let branch = branch(agent);
    if repo.tip(&branch)?.is_none() {
        return Ok(());
    }
    let into = target(&repo, found.pin.as_deref())?;
    let name = Actor::Realm.to_string();
    let merged = repo.merge(
        &branch,
        into,
        "Took in what its profile learned",
        Author { name: &name, at },
    )?;
    check_out(&repo, merged.commit, &Folder::make(&found.path)?)?;
    if hamlet_agents::find(db, agent)?.is_some() {
        hamlet_agents::took_in(db, agent, &into.to_string())?;
    }
    Ok(())
}
