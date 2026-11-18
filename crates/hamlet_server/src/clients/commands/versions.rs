//! A chief's commands on a profile's history.

use hamlet_git::{Author, CommitId, MAIN, Repository};
use hamlet_store::ids::{Actor, HamletId, ProfileId};

use super::Command;
use crate::core::Core;
use crate::knowledge::versions::{
    pin_target, pinned, short, versions,
};
use crate::realm::change::{Done, checked, done, reject};
use crate::realm::jobs::Fork;
use crate::society::addresses::{self, Reach};
use crate::society::rejection::Rejection;
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

impl Core<'_, '_> {
    /// The living profile `text` addresses, which this chief keeps,
    /// with its repository.
    fn chiefs_profile(
        &mut self,
        text: &str,
    ) -> Done<(ProfileId, Repository)> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        let id =
            done!(addresses::profile_at(db, text, Reach::Living));
        if !authority::may_keep(db, Actor::Chief(chief), id)? {
            reject!(Rejection::ProfileNotOwn {
                name: addresses::profile_name_of(db, id)?,
            });
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
            return Ok(Err(self.actions.usage("tag")));
        };
        let (profile, at) = profile
            .split_once('@')
            .map_or((*profile, None), |(s, a)| (s, Some(a)));
        if let Err(rejection) = names::check_version(version) {
            reject!(rejection);
        }
        let (id, repo) = done!(self.chiefs_profile(profile));
        if *version == MAIN || repo.resolve(version)?.is_some() {
            reject!(Rejection::VersionTaken {
                version: (*version).into(),
            });
        }
        let commit =
            done!(commit_of(&repo, profile, at.unwrap_or(MAIN)));
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

    pub(super) fn list_versions(
        &mut self,
        words: &[&str],
    ) -> Done<String> {
        let [profile] = words else {
            return Ok(Err(self.actions.usage("versions")));
        };
        let (_, repo) = done!(self.chiefs_profile(profile));
        Ok(Ok(versions(&repo)?))
    }

    pub(super) fn pin(
        &mut self,
        here: Option<&HamletId>,
        words: &[&str],
    ) -> Done<String> {
        let [address, version, reason @ ..] = words else {
            return Ok(Err(self.actions.usage("pin")));
        };
        let parsed = checked!(addresses::parse(address));
        let db = self.realm.store().connection();
        let agent = done!(addresses::resolve(
            db,
            &parsed,
            here,
            Reach::Living
        ));
        done!(self.directs(agent));
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let target = done!(pin_target(db, &layout, agent, version));
        let name = addresses::name_of(
            self.realm.store().connection(),
            agent,
        )?;
        let reason = reason.join(" ");
        let chief = self.realm.chief();
        let done = self.attempt(Actor::Chief(chief), |cx, _| {
            hamlet_agents::set_pin(
                cx,
                agent,
                target.as_deref(),
                &reason,
            )
        })?;
        Ok(done.map(|()| pinned(&name, target.as_deref())))
    }

    pub(super) fn revert(&mut self, words: &[&str]) -> Done<String> {
        let [profile, which, reason @ ..] = words else {
            return Ok(Err(self.actions.usage("revert")));
        };
        let (id, repo) = done!(self.chiefs_profile(profile));
        let commit = done!(commit_of(&repo, profile, which));
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
            return Ok(Err(self.actions.usage("fork")));
        };
        let (profile, at) = from
            .split_once('@')
            .map_or((*from, MAIN), |(s, a)| (s, a));
        let (id, repo) = done!(self.chiefs_profile(profile));
        let commit = done!(commit_of(&repo, profile, at));
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

    /// Takes the path out of the profile's whole history and its
    /// agents' worktrees.
    pub(super) fn purge(&mut self, words: &[&str]) -> Done<String> {
        let [profile, path] = words else {
            return Ok(Err(self.actions.usage("purge")));
        };
        let (id, _) = done!(self.chiefs_profile(profile));
        let by = Actor::Chief(self.realm.chief());
        self.purge_path(by, id, path)
    }
}

/// The commit `which` names in `repo`.
fn commit_of(
    repo: &Repository,
    profile: &str,
    which: &str,
) -> Done<CommitId> {
    Ok(match repo.resolve(which)? {
        Some(commit) => Ok(commit),
        None => Err(Rejection::NoVersion {
            name: profile.into(),
            version: which.into(),
        }),
    })
}
