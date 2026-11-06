//! A chief's commands on a scholar's history: versions named and
//! listed, a hire pinned, a commit undone, a scholar forked, and a
//! path purged from all of it.

use std::sync::PoisonError;

use anyhow::Result;
use hamlet_git::{Author, CommitId, MAIN, Repository};
use hamlet_store::ids::{Actor, HamletId, ScholarId};

use crate::core::Core;
use crate::jobs::Fork;
use crate::knowledge::folder::Folder;
use crate::society::addresses::{self, Reach};
use crate::society::refusal::Refusal;
use crate::society::{authority, city, hires, names};

/// The most commits `/versions` lists.
const MOST_SHOWN: usize = 12;

impl Core {
    /// Runs the command `name` with `words` when it is one of these.
    pub(super) async fn versions_command(
        &mut self,
        here: Option<&HamletId>,
        name: &str,
        words: &[&str],
    ) -> Result<Option<Result<String, Refusal>>> {
        Ok(Some(match name {
            "tag" => self.tag(words)?,
            "versions" => self.list_versions(words)?,
            "pin" => self.pin(here, words).await?,
            "revert" => self.revert(words).await?,
            "fork" => self.fork(words).await?,
            "purge" => self.purge(words).await?,
            _ => return Ok(None),
        }))
    }

    /// The living scholar `text` addresses, which this chief keeps,
    /// with its repository.
    fn chiefs_scholar(
        &mut self,
        text: &str,
    ) -> Result<Result<(ScholarId, Repository), Refusal>> {
        let chief = self.realm.chief();
        let id = match self.scholar_at(text, false)? {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
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

    /// `/tag <scholar>[@<commit>] <version> [note]`
    fn tag(
        &mut self,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [scholar, version, note @ ..] = words else {
            return Ok(Err(super::usage("tag")));
        };
        let (scholar, at) = scholar
            .split_once('@')
            .map_or((*scholar, None), |(s, a)| (s, Some(a)));
        if let Err(refusal) = names::check_version(version) {
            return Ok(Err(refusal));
        }
        let (_, repo) = match self.chiefs_scholar(scholar)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if *version == MAIN || repo.resolve(version)?.is_some() {
            return Ok(Err(Refusal::BadVersion {
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
        let _held =
            writing.lock().unwrap_or_else(PoisonError::into_inner);
        repo.tag(
            version,
            commit,
            &note.join(" "),
            Author {
                name: &by,
                at: crate::clock::now(),
            },
        )?;
        Ok(Ok(format!("Tagged {scholar} as {version}.")))
    }

    /// `/versions <scholar>`: its versions and its latest commits.
    fn list_versions(
        &mut self,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [scholar] = words else {
            return Ok(Err(super::usage("versions")));
        };
        let (_, repo) = match self.chiefs_scholar(scholar)? {
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

    /// `/pin <address> <version|main> [reason]`
    async fn pin(
        &mut self,
        here: Option<&HamletId>,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [address, version, reason @ ..] = words else {
            return Ok(Err(super::usage("pin")));
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
        let found = hires::get(db, hire)?;
        let repo =
            Repository::open(&layout.repository(found.scholar))?;
        let target = (*version != MAIN).then_some(*version);
        if let Some(version) = target
            && repo.resolve(version)?.is_none()
        {
            return Ok(Err(Refusal::NoVersion {
                name: addresses::scholar_name_of(db, found.scholar)?,
                version: version.into(),
            }));
        }
        let name = addresses::name_of(db, hire)?;
        let reason = reason.join(" ");
        let chief = self.realm.chief();
        let done = self
            .attempt(Actor::Chief(chief), |cx, _| {
                hires::set_pin(cx, hire, target, &reason)
            })
            .await?;
        Ok(done.map(|()| match target {
            Some(version) => format!(
                "{name} takes {version} in once idle, and no later \
                 learning."
            ),
            None => {
                format!("{name} follows its scholar's main again.")
            }
        }))
    }

    /// `/revert <scholar> <commit|version> [reason]`
    async fn revert(
        &mut self,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [scholar, which, reason @ ..] = words else {
            return Ok(Err(super::usage("revert")));
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
        let held =
            writing.lock().unwrap_or_else(PoisonError::into_inner);
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
                at: crate::clock::now(),
            },
        )?;
        drop(held);
        self.main_moved(by, id).await?;
        Ok(Ok(if merged.clashes.is_empty() {
            format!("Reverted {} on {scholar}'s main.", short(commit))
        } else {
            format!(
                "Reverted {} on {scholar}'s main; these changed \
                 again since and kept their later lines: {}",
                short(commit),
                merged.clashes.join(", ")
            )
        }))
    }

    /// `/fork <scholar>[@<version>] <new-name>`
    async fn fork(
        &mut self,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [from, name @ ..] = words else {
            return Ok(Err(super::usage("fork")));
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
        let done = self
            .attempt(Actor::Chief(chief), |cx, _| {
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
            })
            .await?;
        Ok(done.map(|_| format!("Forked {from} as {name}.")))
    }

    /// `/purge <scholar> <path>`: the path leaves every commit, every
    /// branch and every lodge of the scholar's hires.
    async fn purge(
        &mut self,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [scholar, path] = words else {
            return Ok(Err(super::usage("purge")));
        };
        let path = path.trim_matches('/');
        if path.is_empty()
            || path.split('/').any(|part| part == ".." || part == ".")
        {
            return Ok(Err(Refusal::NoFile { path: path.into() }));
        }
        let (id, _) = match self.chiefs_scholar(scholar)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let layout = self.realm.layout().clone();
        let writing = self.writing.clone();
        let held =
            writing.lock().unwrap_or_else(PoisonError::into_inner);
        let db = self.realm.store().connection();
        for hire in hires::living_of(db, id)? {
            let lodge = layout.lodge(&hire.hamlet, hire.number);
            let Ok(folder) = Folder::open(&lodge) else {
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
        for hire in hires::living_of(db, id)? {
            self.push_lodge(hire.agent).await?;
        }
        let by = Actor::Chief(self.realm.chief());
        self.main_moved(by, id).await?;
        Ok(Ok(format!(
            "Purged {path} from {scholar}: {} branches rewritten.",
            rewritten.len()
        )))
    }
}

/// The commit `which` names in `repo`: a version, a commit, or
/// `main`.
fn commit_of(
    repo: &Repository,
    scholar: &str,
    which: &str,
) -> Result<Result<CommitId, Refusal>> {
    Ok(match repo.resolve(which)? {
        Some(commit) => Ok(commit),
        None => Err(Refusal::NoVersion {
            name: scholar.into(),
            version: which.into(),
        }),
    })
}

/// A commit's id as a chief reads it.
fn short(commit: CommitId) -> String {
    commit.to_string().chars().take(8).collect()
}
