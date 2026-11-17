//! A scholar's versions as a chief or an agent reads them.

use hamlet_git::{CommitId, MAIN, Repository};

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

/// A commit's id as a chief reads it.
pub(crate) fn short(commit: CommitId) -> String {
    commit.to_string().chars().take(8).collect()
}
