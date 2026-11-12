//! Where files sent to an agent are saved: the `inbox/` of its lodge,
//! and of its remote workspace when it works there (C33, H30).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::Result;
use hamlet_os::fs::TreePath;
use hamlet_protocol::client::AttachedFile;
use hamlet_store::ids::AgentId;
use hamlet_workspace_wire::{Bytes, MAX_FILE, Write};

use crate::attached::Caption;
use crate::core::Core;
use crate::knowledge::folder::Folder;
use crate::society::refusal::Refusal;
use crate::workspace::lodges::LODGE;
use crate::workspace::{Failure, Session};

/// The folder of a lodge that holds what was sent to its agent.
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

/// Files being saved in `dir`, a folder of a lodge, for one send.
pub(crate) struct Inbox {
    lodge: PathBuf,
    remote: Option<Session>,
    dir: String,
    /// The name each top-level name was saved under, which differs
    /// from it when something of that name was there already.
    chosen: HashMap<String, String>,
    written: HashSet<String>,
    saved: Vec<Saved>,
}

impl Core {
    /// The folder `dir` of `agent`'s lodge, where files are to be
    /// saved: under `inbox/`.
    pub(crate) fn open_inbox(
        &mut self,
        agent: AgentId,
        dir: &str,
    ) -> Result<Inbox> {
        let layout = self.realm.layout().clone();
        let lodge = crate::calls::place::of(
            self.realm.store().connection(),
            &layout,
            agent,
        )?
        .place
        .lodge;
        Ok(Inbox {
            lodge,
            remote: self.remote_session(agent, false)?,
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

fn usage(words: String) -> Refusal {
    Refusal::Usage { words }
}

/// Writes `bytes` at `path` of the remote workspace, in pieces of at
/// most `MAX_FILE` bytes, which is all one request carries.
pub(crate) async fn write_remote(
    session: &Session,
    path: &str,
    bytes: &[u8],
) -> std::result::Result<(), Failure> {
    let pieces = if bytes.is_empty() {
        vec![bytes]
    } else {
        bytes.chunks(MAX_FILE).collect()
    };
    for (piece, bytes) in pieces.into_iter().enumerate() {
        session
            .call(Write {
                path: path.to_owned(),
                data: Bytes(bytes.to_vec()),
                append: piece > 0,
                parents: true,
                no_follow: true,
            })
            .await?;
    }
    Ok(())
}

impl Inbox {
    /// Saves `bytes` as `name`, a path from the folder it was sent
    /// in, which is kept under a name nothing there has.
    pub(crate) async fn put(
        &mut self,
        name: &str,
        bytes: &[u8],
    ) -> Result<std::result::Result<(), Refusal>> {
        self.put_at(name, bytes, None).await
    }

    /// [`Self::put`] for a file that sits at `at` in a line's words.
    pub(crate) async fn put_at(
        &mut self,
        name: &str,
        bytes: &[u8],
        at: Option<u64>,
    ) -> Result<std::result::Result<(), Refusal>> {
        if TreePath::parse(name).is_err() {
            return Ok(Err(usage(format!(
                "`{name}` is not a name of a file."
            ))));
        }
        let lodge = Folder::make(&self.lodge)?;
        let (top, rest) = match name.split_once('/') {
            Some((top, rest)) => (top, Some(rest)),
            None => (name, None),
        };
        let top_name = match self.chosen.get(top) {
            Some(chosen) => chosen.clone(),
            None => {
                let mut candidate = top.to_owned();
                let mut n = 2;
                while lodge
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
        lodge.write(&local, bytes, false)?;
        if let Some(session) = &self.remote {
            let remote = format!("{LODGE}/{local}");
            if let Err(failure) =
                write_remote(session, &remote, bytes).await
            {
                return Ok(Err(usage(format!(
                    "The workspace could not take the file: \
                     {failure}"
                ))));
            }
        }
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
                let at = match &self.remote {
                    None => self
                        .lodge
                        .join(&self.dir)
                        .join(&saved.name)
                        .display()
                        .to_string(),
                    Some(_) => {
                        format!("{LODGE}/{}/{}", self.dir, saved.name)
                    }
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
