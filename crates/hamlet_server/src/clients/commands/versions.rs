//! A chief's commands on a scholar's history: versions named and
//! listed, a hire pinned, a commit undone, a scholar forked, and a
//! path purged from all of it.

use hamlet_git::{Author, CommitId, MAIN, Repository};
use hamlet_store::ids::{Actor, HamletId, ScholarId};

use super::Command;
use crate::core::Core;
use crate::knowledge::versions::{
    pin_target, pinned, short, versions,
};
use crate::realm::change::Done;
use crate::realm::jobs::Fork;
use crate::society::addresses::{self, Reach};
use crate::society::refusal::Refusal;
use crate::society::{authority, city, hires, names};

/// The commands about a scholar's versions.
pub(super) const VERSIONS: &[Command] = &[
    Command {
        name: "tag",
        usage: "/tag <scholar#id>[@<commit>] <version> [note]",
        does: "Name a version of a scholar",
        run: |core, line| core.tag(&line.words()),
    },
    Command {
        name: "versions",
        usage: "/versions <scholar#id>",
        does: "List a scholar's versions and latest commits",
        run: |core, line| core.list_versions(&line.words()),
    },
    Command {
        name: "pin",
        usage: "/pin <address> <version|main> [reason]",
        does: "Hold a hire at a version, or let it follow main",
        run: |core, line| core.pin(line.here.as_ref(), &line.words()),
    },
    Command {
        name: "revert",
        usage: "/revert <scholar#id> <commit|version> [reason]",
        does: "Undo a commit of a scholar's main",
        run: |core, line| core.revert(&line.words()),
    },
    Command {
        name: "fork",
        usage: "/fork <scholar#id>[@<version>] <new name>",
        does: "Make a new scholar from a version of another",
        run: |core, line| core.fork(&line.words()),
    },
    Command {
        name: "purge",
        usage: "/purge <scholar#id> <path>",
        does: "Take a file out of a scholar's whole history",
        run: |core, line| core.purge(&line.words()),
    },
];

impl Core<'_, '_> {
    /// The living scholar `text` addresses, which this chief keeps,
    /// with its repository.
    fn chiefs_scholar(
        &mut self,
        text: &str,
    ) -> Done<(ScholarId, Repository)> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        let id = match addresses::scholar_at(db, text, Reach::Living)?
        {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if !authority::may_keep(db, Actor::Chief(chief), id)? {
            return Ok(Err(Refusal::ScholarNotOwn {
                name: addresses::scholar_name_of(db, id)?,
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
        let [scholar, version, note @ ..] = words else {
            return Ok(Err(self.deeds.usage("tag")));
        };
        let (scholar, at) = scholar
            .split_once('@')
            .map_or((*scholar, None), |(s, a)| (s, Some(a)));
        if let Err(refusal) = names::check_version(version) {
            return Ok(Err(refusal));
        }
        let (id, repo) = match self.chiefs_scholar(scholar)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if *version == MAIN || repo.resolve(version)?.is_some() {
            return Ok(Err(Refusal::VersionTaken {
                version: (*version).into(),
            }));
        }
        let commit =
            match commit_of(&repo, scholar, at.unwrap_or(MAIN))? {
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
        let label = addresses::scholar_name_of(
            self.realm.store().connection(),
            id,
        )?;
        Ok(Ok(format!("Tagged {label} as {version}.")))
    }

    /// Lists the versions of a scholar and the latest commits of its
    /// `main`.
    pub(super) fn list_versions(
        &mut self,
        words: &[&str],
    ) -> Done<String> {
        let [scholar] = words else {
            return Ok(Err(self.deeds.usage("versions")));
        };
        let (_, repo) = match self.chiefs_scholar(scholar)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        Ok(Ok(versions(&repo)?))
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
        let hire = match addresses::resolve(
            db,
            &parsed,
            here,
            Reach::Living,
        )? {
            Ok(hire) => hire,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if let Err(refusal) = self.leads(hire)? {
            return Ok(Err(refusal));
        }
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let target = match pin_target(db, &layout, hire, version)? {
            Ok(target) => target,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let name = addresses::name_of(
            self.realm.store().connection(),
            hire,
        )?;
        let reason = reason.join(" ");
        let chief = self.realm.chief();
        let done = self.attempt(Actor::Chief(chief), |cx, _| {
            hires::set_pin(cx, hire, target.as_deref(), &reason)
        })?;
        Ok(done.map(|()| pinned(&name, target.as_deref())))
    }

    pub(super) fn revert(&mut self, words: &[&str]) -> Done<String> {
        let [scholar, which, reason @ ..] = words else {
            return Ok(Err(self.deeds.usage("revert")));
        };
        let (id, repo) = match self.chiefs_scholar(scholar)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let commit = match commit_of(&repo, scholar, which)? {
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
        let label = addresses::scholar_name_of(
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
        let (scholar, at) = from
            .split_once('@')
            .map_or((*from, MAIN), |(s, a)| (s, a));
        let (id, repo) = match self.chiefs_scholar(scholar)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let commit = match commit_of(&repo, scholar, at)? {
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
        let label = addresses::scholar_name_of(
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
    /// lodge of the scholar's hires.
    pub(super) fn purge(&mut self, words: &[&str]) -> Done<String> {
        let [scholar, path] = words else {
            return Ok(Err(self.deeds.usage("purge")));
        };
        let (id, _) = match self.chiefs_scholar(scholar)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let by = Actor::Chief(self.realm.chief());
        self.purge_path(by, id, path)
    }
}

/// The commit `which` names in `repo`: a version, a commit, or
/// `main`.
fn commit_of(
    repo: &Repository,
    scholar: &str,
    which: &str,
) -> Done<CommitId> {
    Ok(match repo.resolve(which)? {
        Some(commit) => Ok(commit),
        None => Err(Refusal::NoVersion {
            name: scholar.into(),
            version: which.into(),
        }),
    })
}
