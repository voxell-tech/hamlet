//! The Hand's reads and changes of a scholar's repository: its files
//! at a version, a file of `main` written, a version named, a commit
//! undone. A change waits for a chief before it gets here.

use std::sync::PoisonError;

use anyhow::Result;
use hamlet_git::{
    Author, Change as GitChange, CommitId, MAIN, Repository,
};
use hamlet_store::ids::{Actor, ScholarId};

use super::{Call, text};
use crate::core::Core;
use crate::knowledge::CHARACTER;
use crate::society::city::{self, CHARACTER_MOST};
use crate::society::refusal::Refusal;
use crate::society::{addresses, authority};

/// The most of a file `read_scholar` gives.
const MOST_READ: usize = 50 * 1024;

impl Core {
    /// The living scholar `call`'s argument `scholar` names, which
    /// the caller may keep.
    fn kept_scholar(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Result<ScholarId, Refusal>> {
        let address = text(call, "scholar");
        let id = match addresses::parse_scholar(&address) {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        let Some(found) = city::living(db, id)? else {
            return Ok(Err(Refusal::NoScholar { address }));
        };
        if !authority::may_keep(
            db,
            Actor::Agent(call.conversation.agent),
            found.id,
        )? {
            return Ok(Err(Refusal::ScholarNotOwn {
                name: found.label(),
            }));
        }
        Ok(Ok(found.id))
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
    ) -> Result<Result<CommitId, Refusal>> {
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
    ) -> Result<Result<String, Refusal>> {
        let scholar = match self.kept_scholar(call)? {
            Ok(scholar) => scholar,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let name = text(call, "scholar");
        let repo = self.repository(scholar)?;
        let at = match Self::version_of(
            &repo,
            &name,
            &text(call, "version"),
        )? {
            Ok(at) => at,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let path = text(call, "path");
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
            None => Err(Refusal::NoFile { path }),
        })
    }

    pub(super) async fn edit_scholar(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Result<String, Refusal>> {
        let scholar = match self.kept_scholar(call)? {
            Ok(scholar) => scholar,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let path = text(call, "path");
        let content = call
            .arguments
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
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
        let message = text(call, "message");
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
        )
        .await?;
        Ok(Ok(format!("Wrote {path} of {}.", text(call, "scholar"))))
    }

    pub(super) fn tag_scholar(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Result<String, Refusal>> {
        let scholar = match self.kept_scholar(call)? {
            Ok(scholar) => scholar,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let name = text(call, "scholar");
        let tag = text(call, "tag");
        let repo = self.repository(scholar)?;
        if tag.is_empty()
            || tag == MAIN
            || repo.resolve(&tag)?.is_some()
        {
            return Ok(Err(Refusal::BadVersion { version: tag }));
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
        let _held =
            writing.lock().unwrap_or_else(PoisonError::into_inner);
        repo.tag(
            &tag,
            at,
            &text(call, "note"),
            Author {
                name: &by,
                at: crate::clock::now(),
            },
        )?;
        Ok(Ok(format!("Tagged {name}'s main as {tag}.")))
    }

    pub(super) async fn revert_scholar(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Result<String, Refusal>> {
        let scholar = match self.kept_scholar(call)? {
            Ok(scholar) => scholar,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let name = text(call, "scholar");
        let repo = self.repository(scholar)?;
        let commit = match Self::version_of(
            &repo,
            &name,
            &text(call, "commit"),
        )? {
            Ok(commit) => commit,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let by = Actor::Agent(call.conversation.agent);
        let author = by.to_string();
        let reason = text(call, "reason");
        self.begin(call)?;
        let writing = self.writing.clone();
        let held =
            writing.lock().unwrap_or_else(PoisonError::into_inner);
        let merged = repo.revert(
            MAIN,
            commit,
            &format!("Reverted: {reason}"),
            Author {
                name: &author,
                at: crate::clock::now(),
            },
        )?;
        drop(held);
        self.main_moved(by, scholar).await?;
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
