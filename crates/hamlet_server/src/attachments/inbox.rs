//! The `inbox/` of an agent's worktree, where files sent to it are
//! saved (C33, H30). An agent that works in a remote workspace finds
//! them there once its worktree is next pushed, before its next call.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::Result;
use hamlet_os::fs::TreePath;
use hamlet_protocol::client::AttachedFile;
use hamlet_store::ids::AgentId;

use super::captions::Caption;
use crate::core::{Core, UnpushedWorktree};
use crate::knowledge::folder::Folder;
use crate::realm::workspace::worktrees::WORKTREE;
use crate::society::refusal::Refusal;

/// The folder of a worktree that holds what was sent to its agent.
pub(crate) const INBOX: &str = "inbox";

/// A file or folder saved, as the agent finds it.
struct Saved {
    /// The name it was saved under, in the folder it is in.
    name: String,
    /// The place in a line's words it sits at, by the first file
    /// saved.
    at: Option<u64>,
    files: usize,
    bytes: u64,
    folder: bool,
}

/// Files being saved in `dir`, a folder of a worktree, for one send.
pub(crate) struct Inbox {
    worktree: PathBuf,
    /// The agent works in a remote workspace, where its worktree is
    /// at `/worktree`.
    remote: bool,
    dir: String,
    /// The name each top-level name was saved under, which differs
    /// from it when something of that name was there already.
    chosen: HashMap<String, String>,
    written: HashSet<String>,
    saved: Vec<Saved>,
}

impl Core<'_, '_> {
    /// The folder `dir` of `agent`'s worktree, where files are to be
    /// saved: under `inbox/`. Its remote copy is owed a push.
    pub(crate) fn open_inbox(
        &mut self,
        agent: AgentId,
        dir: &str,
    ) -> Result<Inbox> {
        let layout = self.realm.layout().clone();
        let worktree = crate::calls::place::of(
            self.realm.store().connection(),
            &layout,
            agent,
        )?
        .place
        .worktree;
        let remote = self.remote.workspace.is_some();
        if remote {
            self.remote.unpushed.write(UnpushedWorktree(agent));
        }
        Ok(Inbox {
            worktree,
            remote,
            dir: dir.to_owned(),
            chosen: HashMap::new(),
            written: HashSet::new(),
            saved: Vec::new(),
        })
    }
}

/// `name` made different by `n`: `report-2.pdf` for `report.pdf`.
fn numbered(name: &str, n: usize) -> String {
    match name.rfind('.').filter(|dot| *dot > 0) {
        Some(dot) => format!("{}-{n}{}", &name[..dot], &name[dot..]),
        None => format!("{name}-{n}"),
    }
}

impl Inbox {
    /// Saves `bytes` as `name`, a path from the folder it was sent
    /// in, which is kept under a name nothing there has.
    pub(crate) fn put(
        &mut self,
        name: &str,
        bytes: &[u8],
    ) -> Result<std::result::Result<(), Refusal>> {
        self.put_at(name, bytes, None)
    }

    /// [`Self::put`] for a file that sits at `at` in a line's words.
    pub(crate) fn put_at(
        &mut self,
        name: &str,
        bytes: &[u8],
        at: Option<u64>,
    ) -> Result<std::result::Result<(), Refusal>> {
        if TreePath::parse(name).is_err() {
            return Ok(Err(Refusal::NotFileName {
                name: name.into(),
            }));
        }
        let worktree = Folder::make(&self.worktree)?;
        let (top, rest) = match name.split_once('/') {
            Some((top, rest)) => (top, Some(rest)),
            None => (name, None),
        };
        let top_name = match self.chosen.get(top) {
            Some(chosen) => chosen.clone(),
            None => {
                let mut candidate = top.to_owned();
                let mut n = 2;
                while worktree
                    .exists(&format!("{}/{candidate}", self.dir))
                    || self
                        .chosen
                        .values()
                        .any(|taken| *taken == candidate)
                {
                    candidate = numbered(top, n);
                    n += 1;
                }
                self.chosen.insert(top.to_owned(), candidate.clone());
                self.saved.push(Saved {
                    name: candidate.clone(),
                    at,
                    files: 0,
                    bytes: 0,
                    folder: false,
                });
                candidate
            }
        };
        let mut path = match rest {
            Some(rest) => format!("{top_name}/{rest}"),
            None => top_name.clone(),
        };
        // A path sent twice in one send keeps both.
        let mut n = 2;
        while !self.written.insert(path.clone()) {
            path = match rest {
                Some(rest) => {
                    let (folders, last) =
                        rest.rsplit_once('/').unwrap_or(("", rest));
                    let last = numbered(last, n);
                    match folders.is_empty() {
                        true => format!("{top_name}/{last}"),
                        false => {
                            format!("{top_name}/{folders}/{last}")
                        }
                    }
                }
                None => numbered(&top_name, n),
            };
            n += 1;
        }
        let local = format!("{}/{path}", self.dir);
        worktree.write(&local, bytes, false)?;
        if let Some(saved) =
            self.saved.iter_mut().find(|saved| saved.name == top_name)
        {
            saved.files += 1;
            saved.bytes += bytes.len() as u64;
            saved.folder |= rest.is_some();
        }
        Ok(Ok(()))
    }

    /// The lines that name where each file or folder saved is, as it
    /// is `of`: "of this message".
    pub(crate) fn captions(&self, of: &str) -> String {
        self.placed_captions(of)
            .into_iter()
            .map(|caption| caption.words)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// [`Self::captions`], one for each file or folder saved, with
    /// the place it sits at.
    pub(crate) fn placed_captions(&self, of: &str) -> Vec<Caption> {
        self.saved
            .iter()
            .enumerate()
            .map(|(index, saved)| {
                let at = if self.remote {
                    format!("{WORKTREE}/{}/{}", self.dir, saved.name)
                } else {
                    self.worktree
                        .join(&self.dir)
                        .join(&saved.name)
                        .display()
                        .to_string()
                };
                let size = AttachedFile::size_words(saved.bytes);
                let n = index + 1;
                let words = match saved.folder {
                    true => format!(
                        "[Folder {n} {of}, {} files, {size}, saved \
                         at {at}]",
                        saved.files
                    ),
                    false => format!(
                        "[File {n} {of}, {size}, saved at {at}]"
                    ),
                };
                Caption {
                    at: saved.at,
                    words,
                }
            })
            .collect()
    }
}
