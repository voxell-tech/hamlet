//! A profile's versions, and the version an agent is made or pinned
//! at.

use hamlet_git::{CommitId, MAIN, Repository};
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ProfileId};

use crate::realm::change::Done;
use crate::realm::layout::Layout;
use crate::society::addresses;
use crate::society::hamlet_agents::{self, ProfileChoice};
use crate::society::rejection::Rejection;

/// The most commits [`versions`] lists.
const MOST_SHOWN: usize = 12;

/// The versions of the profile in `repo` and the latest commits of
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

pub(crate) fn pinned(name: &str, target: Option<&str>) -> String {
    match target {
        Some(version) => format!(
            "{name} takes {version} in once idle, and no later \
             learning."
        ),
        None => format!("{name} follows its profile's main again."),
    }
}

/// An agent of the living `profile`, at its `main` or pinned to
/// `version` when that is one the profile has.
pub(crate) fn existing(
    db: &mut Db,
    layout: &Layout,
    profile: ProfileId,
    version: Option<String>,
) -> Done<ProfileChoice> {
    if let Some(version) = &version {
        let found = Repository::open(&layout.repository(profile))
            .ok()
            .and_then(|repo| repo.resolve(version).ok().flatten());
        if found.is_none() || version == MAIN {
            return Ok(Err(Rejection::NoVersion {
                name: addresses::profile_name_of(db, profile)?,
                version: version.clone(),
            }));
        }
    }
    Ok(Ok(ProfileChoice::Existing {
        profile,
        pin: version,
    }))
}

/// The version to pin `agent` to, none for `main`.
pub(crate) fn pin_target(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
    version: &str,
) -> Done<Option<String>> {
    if version == MAIN {
        return Ok(Ok(None));
    }
    let found = hamlet_agents::get(db, agent)?;
    let repo = Repository::open(&layout.repository(found.profile))?;
    if repo.resolve(version)?.is_none() {
        return Ok(Err(Rejection::NoVersion {
            name: addresses::profile_name_of(db, found.profile)?,
            version: version.into(),
        }));
    }
    Ok(Ok(Some(version.into())))
}

pub(crate) fn short(commit: CommitId) -> String {
    commit.to_string().chars().take(8).collect()
}
