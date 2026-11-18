//! A chief's commands on a profile's history: versions named and
//! listed, an agent pinned, a commit undone, a profile forked, and a
//! path purged from all of it.

use hamlet_git::{Author, CommitId, MAIN, Repository};
use hamlet_store::ids::{Actor, HamletId, ProfileId};

use super::Command;
use super::targets::ProfileReach;
use crate::core::{Core, UnpushedWorktree};
use crate::knowledge::folder::Folder;
use crate::realm::change::Done;
use crate::realm::jobs::Fork;
use crate::society::addresses::{self, Reach};
use crate::society::refusal::Refusal;
use crate::society::{authority, city, hamlet_agents, names};

/// The commands about a profile's versions.
pub(super) const VERSIONS: &[Command] = &[
    Command {
        name: "tag",
        usage: "/tag <profile#id>[@<commit>] <version> [note]",
        does: "Name a version of a profile",
        run: |core, line| core.tag(&line.words()),
    },
    Command {
        name: "versions",
        usage: "/versions <profile#id>",
        does: "List a profile's versions and latest commits",
        run: |core, line| core.list_versions(&line.words()),
    },
    Command {
        name: "pin",
        usage: "/pin <address> <version|main> [reason]",
        does: "Hold an agent at a version, or let it follow main",
        run: |core, line| core.pin(line.here.as_ref(), &line.words()),
    },
    Command {
        name: "revert",
        usage: "/revert <profile#id> <commit|version> [reason]",
        does: "Undo a commit of a profile's main",
        run: |core, line| core.revert(&line.words()),
    },
    Command {
        name: "fork",
        usage: "/fork <profile#id>[@<version>] <new name>",
        does: "Make a new profile from a version of another",
        run: |core, line| core.fork(&line.words()),
    },
    Command {
        name: "purge",
        usage: "/purge <profile#id> <path>",
        does: "Take a file out of a profile's whole history",
        run: |core, line| core.purge(&line.words()),
    },
];

/// The most commits `/versions` lists.
const MOST_SHOWN: usize = 12;

impl Core<'_, '_> {
    /// The living profile `text` addresses, which this chief keeps,
    /// with its repository.
    fn chiefs_profile(
        &mut self,
        text: &str,
    ) -> Done<(ProfileId, Repository)> {
        let chief = self.realm.chief();
        let id = match self.profile_at(text, ProfileReach::Living)? {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        if !authority::may_keep(db, Actor::Chief(chief), id)? {
            return Ok(Err(Refusal::ProfileNotOwn {
                name: addresses::profile_name_of(db, id)?,
            }));
        }
        let repo =
            Repository::open(&self.realm.layout().repository(id))?;
        Ok(Ok((id, repo)))
    }

    fn author(&self) -> String {
        Actor::Chief(self.realm.chief()).to_string()
    }

    pub(super) fn tag(&mut self, words: &[&str]) -> Done<String> {
        let [profile, version, note @ ..] = words else {
            return Ok(Err(self.deeds.usage("tag")));
        };
        let (profile, at) = profile
            .split_once('@')
            .map_or((*profile, None), |(s, a)| (s, Some(a)));
        if let Err(refusal) = names::check_version(version) {
            return Ok(Err(refusal));
        }
        let (id, repo) = match self.chiefs_profile(profile)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if *version == MAIN || repo.resolve(version)?.is_some() {
            return Ok(Err(Refusal::VersionTaken {
                version: (*version).into(),
            }));
        }
        let commit =
            match commit_of(&repo, profile, at.unwrap_or(MAIN))? {
                Ok(commit) => commit,
                Err(refusal) => return Ok(Err(refusal)),
            };
        let by = self.author();
        let writing = self.writing.clone();
        let _held = writing.hold();
        repo.tag(
            version,
            commit,
            &note.join(" "),
            Author {
                name: &by,
                at: crate::app::clock::now(),
            },
        )?;
        let label = addresses::profile_name_of(
            self.realm.store().connection(),
            id,
        )?;
        Ok(Ok(format!("Tagged {label} as {version}.")))
    }

    /// Lists the versions of a profile and the latest commits of its
    /// `main`.
    pub(super) fn list_versions(
        &mut self,
        words: &[&str],
    ) -> Done<String> {
        let [profile] = words else {
            return Ok(Err(self.deeds.usage("versions")));
        };
        let (_, repo) = match self.chiefs_profile(profile)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
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
        Ok(Ok(lines.join("\n")))
    }

    pub(super) fn pin(
        &mut self,
        here: Option<&HamletId>,
        words: &[&str],
    ) -> Done<String> {
        let [address, version, reason @ ..] = words else {
            return Ok(Err(self.deeds.usage("pin")));
        };
        let parsed = match addresses::parse(address) {
            Ok(parsed) => parsed,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        let agent = match addresses::resolve(
            db,
            &parsed,
            here,
            Reach::Living,
        )? {
            Ok(agent) => agent,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if let Err(refusal) = self.directs(agent)? {
            return Ok(Err(refusal));
        }
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let found = hamlet_agents::get(db, agent)?;
        let repo =
            Repository::open(&layout.repository(found.profile))?;
        let target = (*version != MAIN).then_some(*version);
        if let Some(version) = target
            && repo.resolve(version)?.is_none()
        {
            return Ok(Err(Refusal::NoVersion {
                name: addresses::profile_name_of(db, found.profile)?,
                version: version.into(),
            }));
        }
        let name = addresses::name_of(db, agent)?;
        let reason = reason.join(" ");
        let chief = self.realm.chief();
        let done = self.attempt(Actor::Chief(chief), |cx, _| {
            hamlet_agents::set_pin(cx, agent, target, &reason)
        })?;
        Ok(done.map(|()| match target {
            Some(version) => format!(
                "{name} takes {version} in once idle, and no later \
                 learning."
            ),
            None => {
                format!("{name} follows its profile's main again.")
            }
        }))
    }

    pub(super) fn revert(&mut self, words: &[&str]) -> Done<String> {
        let [profile, which, reason @ ..] = words else {
            return Ok(Err(self.deeds.usage("revert")));
        };
        let (id, repo) = match self.chiefs_profile(profile)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let commit = match commit_of(&repo, profile, which)? {
            Ok(commit) => commit,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let by = Actor::Chief(self.realm.chief());
        let author = by.to_string();
        let writing = self.writing.clone();
        let held = writing.hold();
        let merged = repo.revert(
            MAIN,
            commit,
            &format!(
                "Reverted {}: {}",
                short(commit),
                reason.join(" ")
            ),
            Author {
                name: &author,
                at: crate::app::clock::now(),
            },
        )?;
        drop(held);
        self.main_moved(by, id)?;
        let label = addresses::profile_name_of(
            self.realm.store().connection(),
            id,
        )?;
        Ok(Ok(if merged.clashes.is_empty() {
            format!("Reverted {} on {label}'s main.", short(commit))
        } else {
            format!(
                "Reverted {} on {label}'s main; these changed again \
                 since and kept their later lines: {}",
                short(commit),
                merged.clashes.join(", ")
            )
        }))
    }

    pub(super) fn fork(&mut self, words: &[&str]) -> Done<String> {
        let [from, name @ ..] = words else {
            return Ok(Err(self.deeds.usage("fork")));
        };
        let (profile, at) = from
            .split_once('@')
            .map_or((*from, MAIN), |(s, a)| (s, a));
        let (id, repo) = match self.chiefs_profile(profile)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let commit = match commit_of(&repo, profile, at)? {
            Ok(commit) => commit,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let chief = self.realm.chief();
        let name = name.join(" ");
        let done = self.attempt(Actor::Chief(chief), |cx, _| {
            city::make(
                cx,
                &name,
                chief,
                "",
                Some(Fork {
                    from: id,
                    commit: commit.to_string(),
                }),
            )
        })?;
        let label = addresses::profile_name_of(
            self.realm.store().connection(),
            id,
        )?;
        Ok(done.map(|_| {
            if at == MAIN {
                format!("Forked {label} as {name}.")
            } else {
                format!("Forked {label} at {at} as {name}.")
            }
        }))
    }

    /// Takes the path out of every commit, every branch and every
    /// worktree of the profile's agents.
    pub(super) fn purge(&mut self, words: &[&str]) -> Done<String> {
        let [profile, path] = words else {
            return Ok(Err(self.deeds.usage("purge")));
        };
        let path = path.trim_matches('/');
        if path.is_empty()
            || path.split('/').any(|part| part == ".." || part == ".")
        {
            return Ok(Err(Refusal::NoFile { path: path.into() }));
        }
        let (id, _) = match self.chiefs_profile(profile)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let layout = self.realm.layout().clone();
        let writing = self.writing.clone();
        let held = writing.hold();
        let db = self.realm.store().connection();
        for agent in hamlet_agents::living_of(db, id)? {
            let worktree =
                layout.worktree(&agent.hamlet, agent.agent);
            let Ok(folder) = Folder::open(&worktree) else {
                continue;
            };
            // The path is a file, or a folder of them.
            for entry in folder.list_under(path)? {
                folder.remove(&entry.path)?;
            }
            folder.remove(path)?;
        }
        let rewritten =
            hamlet_git::purge(&layout.repository(id), path)?;
        drop(held);
        let db = self.realm.store().connection();
        for agent in hamlet_agents::living_of(db, id)? {
            self.remote.unpushed.write(UnpushedWorktree(agent.agent));
        }
        let by = Actor::Chief(self.realm.chief());
        self.main_moved(by, id)?;
        let label = addresses::profile_name_of(
            self.realm.store().connection(),
            id,
        )?;
        Ok(Ok(format!(
            "Purged {path} from {label}: {} branches rewritten.",
            rewritten.len()
        )))
    }
}

/// The commit `which` names in `repo`: a version, a commit, or
/// `main`.
fn commit_of(
    repo: &Repository,
    profile: &str,
    which: &str,
) -> Done<CommitId> {
    Ok(match repo.resolve(which)? {
        Some(commit) => Ok(commit),
        None => Err(Refusal::NoVersion {
            name: profile.into(),
            version: which.into(),
        }),
    })
}

/// A commit's id as a chief reads it.
fn short(commit: CommitId) -> String {
    commit.to_string().chars().take(8).collect()
}
