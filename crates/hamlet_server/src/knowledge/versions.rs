//! A scholar's versions as a chief or an agent reads them, and the
//! version a hire is made or pinned at.

use hamlet_git::{CommitId, MAIN, Repository};
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ScholarId};

use crate::realm::change::Done;
use crate::realm::layout::Layout;
use crate::society::addresses;
use crate::society::hires::{self, Hiring};
use crate::society::refusal::Refusal;

/// The most commits [`versions`] lists.
const MOST_SHOWN: usize = 12;

/// The versions of the scholar in `repo` and the latest commits of
/// its `main`.
pub(crate) fn versions(repo: &Repository) -> anyhow::Result<String> {
    let mut lines = Vec::new();
    let versions = repo.versions()?;
    if versions.is_empty() {
        lines.push("No versions yet; /tag names one.".to_owned());
    } else {
        lines.push("Versions:".into());
        for version in versions {
            let note = if version.note.is_empty() {
                String::new()
            } else {
                format!(": {}", version.note)
            };
            lines.push(format!(
                "- {} ({}){note}",
                version.name,
                short(version.commit)
            ));
        }
    }
    if let Some(tip) = repo.tip(MAIN)? {
        lines.push("Latest commits of main:".into());
        for commit in repo.history(tip, MOST_SHOWN)? {
            let first =
                commit.message.lines().next().unwrap_or_default();
            lines.push(format!(
                "- {} {} by {}",
                short(commit.id),
                first,
                commit.author
            ));
        }
    }
    Ok(lines.join("\n"))
}

/// What pinning the hire `name` to `target`, or to none, comes to.
pub(crate) fn pinned(name: &str, target: Option<&str>) -> String {
    match target {
        Some(version) => format!(
            "{name} takes {version} in once idle, and no later \
             learning."
        ),
        None => format!("{name} follows its scholar's main again."),
    }
}

/// A hire of the living `scholar`, at its `main` or pinned to
/// `version` when that is one the scholar has.
pub(crate) fn existing(
    db: &mut Db,
    layout: &Layout,
    scholar: ScholarId,
    version: Option<String>,
) -> Done<Hiring> {
    if let Some(version) = &version {
        let found = Repository::open(&layout.repository(scholar))
            .ok()
            .and_then(|repo| repo.resolve(version).ok().flatten());
        if found.is_none() || version == MAIN {
            return Ok(Err(Refusal::NoVersion {
                name: addresses::scholar_name_of(db, scholar)?,
                version: version.clone(),
            }));
        }
    }
    Ok(Ok(Hiring::Existing {
        scholar,
        pin: version,
    }))
}

/// The version `version` of the scholar of `hire` to pin it to, none
/// for `main`.
pub(crate) fn pin_target(
    db: &mut Db,
    layout: &Layout,
    hire: AgentId,
    version: &str,
) -> Done<Option<String>> {
    if version == MAIN {
        return Ok(Ok(None));
    }
    let found = hires::get(db, hire)?;
    let repo = Repository::open(&layout.repository(found.scholar))?;
    if repo.resolve(version)?.is_none() {
        return Ok(Err(Refusal::NoVersion {
            name: addresses::scholar_name_of(db, found.scholar)?,
            version: version.into(),
        }));
    }
    Ok(Ok(Some(version.into())))
}

/// A commit's id as a chief reads it.
pub(crate) fn short(commit: CommitId) -> String {
    commit.to_string().chars().take(8).collect()
}
