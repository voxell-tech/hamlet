//! The Hand's reads and changes of a scholar: its files at a version,
//! a file of `main` written, a version named or listed, a commit
//! undone, a fork, a purge, and its place in the city. A reserved
//! change waits for a chief before it gets here.

use anyhow::Result;
use hamlet_git::{
    Author, Change as GitChange, CommitId, MAIN, Repository,
};
use hamlet_store::ids::{Actor, ScholarId};

use super::args::{
    Called, EditScholar, EraseScholar, Fork, MoveHouse, NameScholar,
    Purge, ReadScholar, RemoveScholar, RestoreScholar, Revert, Tag,
    TurnAway, Versions, Welcome,
};
use super::{Call, cut, task_chief};
use crate::core::Core;
use crate::knowledge::CHARACTER;
use crate::knowledge::versions::versions;
use crate::realm::change::{Done, done, refuse};
use crate::realm::jobs;
use crate::society::addresses::{self, Reach};
use crate::society::city::{self, CHARACTER_MOST};
use crate::society::refusal::Refusal;
use crate::society::{authority, names};

impl Core<'_, '_> {
    /// The scholar at `address` within `reach`, which `call`'s agent
    /// may keep.
    fn kept_scholar(
        &mut self,
        call: &Call<'_>,
        address: &str,
        reach: Reach,
    ) -> Done<ScholarId> {
        let db = self.realm.store().connection();
        let id = done!(addresses::scholar_at(db, address, reach));
        if !authority::may_keep(
            db,
            Actor::Agent(call.conversation.agent),
            id,
        )? {
            return Ok(Err(Refusal::ScholarNotOwn {
                name: city::get(db, id)?.label(),
            }));
        }
        Ok(Ok(id))
    }

    /// Carries out the Hand's call `called` on a scholar of the city:
    /// letting it in or turning it away, naming, moving, removing,
    /// restoring or erasing it, listing its versions, forking it, or
    /// purging a path from it.
    pub(super) fn scholar_tool(
        &mut self,
        call: &Call<'_>,
        called: Called,
    ) -> Done<String> {
        let (address, reach) = match &called {
            Called::RestoreScholar(RestoreScholar {
                scholar,
                ..
            })
            | Called::EraseScholar(EraseScholar { scholar }) => {
                (scholar.clone(), Reach::Kept)
            }
            Called::Versions(Versions { scholar })
            | Called::Fork(Fork { scholar, .. })
            | Called::Purge(Purge { scholar, .. })
            | Called::Welcome(Welcome { scholar, .. })
            | Called::TurnAway(TurnAway { scholar, .. })
            | Called::NameScholar(NameScholar { scholar, .. })
            | Called::MoveHouse(MoveHouse { scholar, .. })
            | Called::RemoveScholar(RemoveScholar {
                scholar, ..
            }) => (scholar.clone(), Reach::Living),
            _ => refuse!(Refusal::Unsupported {
                what: format!("the tool {}", call.tool),
            }),
        };
        let scholar = done!(self.kept_scholar(call, &address, reach));
        let shown = addresses::scholar_name_of(
            self.realm.store().connection(),
            scholar,
        )?;
        match called {
            Called::Versions(_) => {
                Ok(Ok(versions(&self.repository(scholar)?)?))
            }
            Called::Purge(Purge { path, .. }) => {
                self.begin(call)?;
                let by = Actor::Agent(call.conversation.agent);
                self.purge_path(by, scholar, &path)
            }
            Called::Fork(Fork { version, name, .. }) => {
                let repo = self.repository(scholar)?;
                let commit =
                    done!(Self::version_of(&repo, &shown, &version));
                let task = self.task_of(call)?;
                let db = self.realm.store().connection();
                let overseer = task_chief(db, task)?;
                let owner = authority::owner_for(
                    db,
                    Actor::Agent(call.conversation.agent),
                    None,
                    overseer,
                )?;
                self.carry(call, |cx, _| {
                    let fork = jobs::Fork {
                        from: scholar,
                        commit: commit.to_string(),
                    };
                    let made = done!(city::make(
                        cx,
                        &name,
                        owner,
                        "",
                        Some(fork)
                    ));
                    let made =
                        addresses::scholar_name_of(cx.db, made)?;
                    Ok(Ok(format!("Forked {shown} as {made}.")))
                })
            }
            Called::NameScholar(NameScholar { name, .. }) => self
                .carry(call, |cx, _| {
                    done!(city::rename(cx, scholar, &name));
                    let named =
                        addresses::scholar_name_of(cx.db, scholar)?;
                    Ok(Ok(format!("The scholar is {named} now.")))
                }),
            Called::MoveHouse(MoveHouse {
                x, z, degrees, ..
            }) => {
                let turn = degrees.map(f64::to_radians);
                self.carry(call, |cx, _| {
                    Ok(city::place(cx, scholar, x, z, turn)?.map(
                        |()| {
                            format!(
                                "{shown}'s house stands at {x}, {z}."
                            )
                        },
                    ))
                })
            }
            Called::Welcome(Welcome { reason, .. }) => {
                self.carry(call, |cx, _| {
                    Ok(city::welcome(cx, scholar, &reason)?.map(
                        |()| {
                            format!(
                                "{shown} moved into its house in \
                                 the city."
                            )
                        },
                    ))
                })
            }
            Called::TurnAway(TurnAway { reason, .. }) => {
                self.carry(call, |cx, _| {
                    Ok(city::turn_away(cx, scholar, &reason)?.map(
                        |()| format!("{shown} waits at the harbor."),
                    ))
                })
            }
            Called::RemoveScholar(RemoveScholar {
                reason, ..
            }) => self.carry(call, |cx, _| {
                Ok(city::archive(cx, scholar, &reason)?.map(|()| {
                    format!("{shown} left the city, with its hires.")
                }))
            }),
            Called::RestoreScholar(RestoreScholar {
                reason, ..
            }) => self.carry(call, |cx, _| {
                Ok(city::restore(cx, scholar, &reason)?.map(|()| {
                    format!("{shown} is back in the city.")
                }))
            }),
            _ => self.carry(call, |cx, _| {
                Ok(city::erase(cx, scholar)?.map(|()| {
                    format!("{shown} was erased for good.")
                }))
            }),
        }
    }

    fn repository(&self, scholar: ScholarId) -> Result<Repository> {
        Repository::open(&self.realm.layout().repository(scholar))
    }

    /// The commit `version` names in `repo`: a version, a commit, or
    /// `main` when empty.
    fn version_of(
        repo: &Repository,
        name: &str,
        version: &str,
    ) -> Done<CommitId> {
        let wanted = if version.is_empty() { MAIN } else { version };
        Ok(match repo.resolve(wanted)? {
            Some(commit) => Ok(commit),
            None => Err(Refusal::NoVersion {
                name: name.into(),
                version: wanted.into(),
            }),
        })
    }

    pub(super) fn read_scholar(
        &mut self,
        call: &Call<'_>,
        args: ReadScholar,
    ) -> Done<String> {
        let ReadScholar {
            scholar: name,
            path,
            version,
        } = args;
        let scholar =
            done!(self.kept_scholar(call, &name, Reach::Living));
        let repo = self.repository(scholar)?;
        let at = done!(Self::version_of(&repo, &name, &version));
        if path.is_empty() {
            let files = repo
                .entries(at)?
                .into_iter()
                .map(|entry| format!("- {}", entry.path))
                .collect::<Vec<_>>();
            return Ok(Ok(if files.is_empty() {
                format!("{name} has no files.")
            } else {
                files.join("\n")
            }));
        }
        Ok(match repo.read(at, &path)? {
            Some(bytes) => {
                Ok(cut(String::from_utf8_lossy(&bytes).into_owned()))
            }
            None => Err(Refusal::NoFile { path }),
        })
    }

    pub(super) fn edit_scholar(
        &mut self,
        call: &Call<'_>,
        args: EditScholar,
    ) -> Done<String> {
        let EditScholar {
            scholar: name,
            path,
            content,
            message,
        } = args;
        let scholar =
            done!(self.kept_scholar(call, &name, Reach::Living));
        if path.is_empty()
            || path.starts_with('/')
            || path.split('/').any(|part| part == "..")
        {
            return Ok(Err(Refusal::NoFile { path }));
        }
        if path == CHARACTER
            && content.chars().count() > CHARACTER_MOST
        {
            return Ok(Err(Refusal::LongCharacter));
        }
        let change = if content.is_empty() {
            GitChange::Remove { path: path.clone() }
        } else {
            GitChange::Write {
                path: path.clone(),
                bytes: content.into_bytes(),
            }
        };
        self.begin(call)?;
        self.commit_main(
            Actor::Agent(call.conversation.agent),
            scholar,
            &[change],
            if message.is_empty() {
                "Edited by the Hand"
            } else {
                &message
            },
        )?;
        Ok(Ok(format!("Wrote {path} of {name}.")))
    }

    pub(super) fn tag_scholar(
        &mut self,
        call: &Call<'_>,
        args: Tag,
    ) -> Done<String> {
        let Tag {
            scholar: name,
            tag,
            note,
        } = args;
        let scholar =
            done!(self.kept_scholar(call, &name, Reach::Living));
        let repo = self.repository(scholar)?;
        if let Err(refusal) = names::check_version(&tag) {
            return Ok(Err(refusal));
        }
        if tag == MAIN || repo.resolve(&tag)?.is_some() {
            return Ok(Err(Refusal::VersionTaken { version: tag }));
        }
        let Some(at) = repo.tip(MAIN)? else {
            return Ok(Err(Refusal::NoVersion {
                name,
                version: MAIN.into(),
            }));
        };
        let by = Actor::Agent(call.conversation.agent).to_string();
        self.begin(call)?;
        let writing = self.writing.clone();
        let _held = writing.hold();
        repo.tag(
            &tag,
            at,
            &note,
            Author {
                name: &by,
                at: crate::app::clock::now(),
            },
        )?;
        Ok(Ok(format!("Tagged {name}'s main as {tag}.")))
    }

    pub(super) fn revert_scholar(
        &mut self,
        call: &Call<'_>,
        args: Revert,
    ) -> Done<String> {
        let Revert {
            scholar: name,
            commit,
            reason,
        } = args;
        let scholar =
            done!(self.kept_scholar(call, &name, Reach::Living));
        let repo = self.repository(scholar)?;
        let commit = done!(Self::version_of(&repo, &name, &commit));
        let by = Actor::Agent(call.conversation.agent);
        let author = by.to_string();
        self.begin(call)?;
        let writing = self.writing.clone();
        let held = writing.hold();
        let merged = repo.revert(
            MAIN,
            commit,
            &format!("Reverted: {reason}"),
            Author {
                name: &author,
                at: crate::app::clock::now(),
            },
        )?;
        drop(held);
        self.main_moved(by, scholar)?;
        Ok(Ok(if merged.clashes.is_empty() {
            format!("Reverted the commit on {name}'s main.")
        } else {
            format!(
                "Reverted the commit on {name}'s main; these files \
                 changed again since and kept their later lines: {}",
                merged.clashes.join(", ")
            )
        }))
    }
}
