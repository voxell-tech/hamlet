//! The Hand's reads and changes of a profile's repository: its files
//! at a version, a file of `main` written, a version named, a commit
//! undone. A change waits for a chief before it gets here.

use anyhow::Result;
use hamlet_git::{
    Author, Change as GitChange, CommitId, MAIN, Repository,
};
use hamlet_store::ids::{Actor, ProfileId};

use super::Call;
use super::args::{EditProfile, ReadProfile, Revert, Tag};
use crate::core::Core;
use crate::knowledge::CHARACTER;
use crate::realm::change::{Done, done};
use crate::society::city::{self, CHARACTER_MOST};
use crate::society::rejection::Rejection;
use crate::society::{addresses, authority, names};

/// The most of a file `read_profile` gives.
const MOST_READ: usize = 50 * 1024;

impl Core<'_, '_> {
    /// The living profile at `address`, which `call`'s agent may
    /// keep.
    fn kept_profile(
        &mut self,
        call: &Call<'_>,
        address: &str,
    ) -> Done<ProfileId> {
        let address = address.to_owned();
        let id = match addresses::parse_profile(&address) {
            Ok(id) => id,
            Err(rejection) => return Ok(Err(rejection)),
        };
        let db = self.realm.store().connection();
        let Some(found) = city::living(db, id)? else {
            return Ok(Err(Rejection::NoProfile { address }));
        };
        if !authority::may_keep(
            db,
            Actor::Agent(call.conversation.agent),
            found.id,
        )? {
            return Ok(Err(Rejection::ProfileNotOwn {
                name: found.label(),
            }));
        }
        Ok(Ok(found.id))
    }

    fn repository(&self, profile: ProfileId) -> Result<Repository> {
        Repository::open(&self.realm.layout().repository(profile))
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
            None => Err(Rejection::NoVersion {
                name: name.into(),
                version: wanted.into(),
            }),
        })
    }

    pub(super) fn read_profile(
        &mut self,
        call: &Call<'_>,
        args: ReadProfile,
    ) -> Done<String> {
        let ReadProfile {
            profile: name,
            path,
            version,
        } = args;
        let profile = done!(self.kept_profile(call, &name));
        let repo = self.repository(profile)?;
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
                let mut words =
                    String::from_utf8_lossy(&bytes).into_owned();
                if words.len() > MOST_READ {
                    let mut end = MOST_READ;
                    while !words.is_char_boundary(end) {
                        end -= 1;
                    }
                    words.truncate(end);
                    words.push_str("\n[cut here]");
                }
                Ok(words)
            }
            None => Err(Rejection::NoFile { path }),
        })
    }

    pub(super) fn edit_profile(
        &mut self,
        call: &Call<'_>,
        args: EditProfile,
    ) -> Done<String> {
        let EditProfile {
            profile: name,
            path,
            content,
            message,
        } = args;
        let profile = done!(self.kept_profile(call, &name));
        if path.is_empty()
            || path.starts_with('/')
            || path.split('/').any(|part| part == "..")
        {
            return Ok(Err(Rejection::NoFile { path }));
        }
        if path == CHARACTER
            && content.chars().count() > CHARACTER_MOST
        {
            return Ok(Err(Rejection::LongCharacter));
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
            profile,
            &[change],
            if message.is_empty() {
                "Edited by the Hand"
            } else {
                &message
            },
        )?;
        Ok(Ok(format!("Wrote {path} of {name}.")))
    }

    pub(super) fn tag_profile(
        &mut self,
        call: &Call<'_>,
        args: Tag,
    ) -> Done<String> {
        let Tag {
            profile: name,
            tag,
            note,
        } = args;
        let profile = done!(self.kept_profile(call, &name));
        let repo = self.repository(profile)?;
        if let Err(rejection) = names::check_version(&tag) {
            return Ok(Err(rejection));
        }
        if tag == MAIN || repo.resolve(&tag)?.is_some() {
            return Ok(Err(Rejection::VersionTaken { version: tag }));
        }
        let Some(at) = repo.tip(MAIN)? else {
            return Ok(Err(Rejection::NoVersion {
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

    pub(super) fn revert_profile(
        &mut self,
        call: &Call<'_>,
        args: Revert,
    ) -> Done<String> {
        let Revert {
            profile: name,
            commit,
            reason,
        } = args;
        let profile = done!(self.kept_profile(call, &name));
        let repo = self.repository(profile)?;
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
        self.main_moved(by, profile)?;
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
