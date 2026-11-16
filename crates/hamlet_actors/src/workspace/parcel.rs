//! The files of a parcel, measured and read for the realm under the
//! sending agent's reach: each named path passes the gate as a read
//! does, and no link is followed.

use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::Realm;
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::realm::parcels::{PARCEL_PART, Part};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::refusal::{InvalidPath, ReadRefusal};
use hamlet_messages::workspace::{
    AgentNumber, Parcel, ParcelAnswer, ParcelEntry, ParcelFault,
    ParcelPart, ParcelRefusal,
};
use hamlet_os::fs::TreePath;
use hamlet_os::fs::path::{self, is_absolute, text};
use hamlet_protocol::client::AttachedFile;
use hamlet_sandbox::{Env, FileError, FileInfo, FileKind};
use sha2::Digest as _;

use super::reach::{self, Gate, Op, Placement, Purpose};
use super::{Weighing, Workspace, read_only};

/// The most entries, files or not, that one parcel's walk visits.
const VISITED: usize = 50_000;

impl Handler<Parcel> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        parcel: Parcel,
    ) -> Result<ParcelAnswer> {
        let Realm = sender(&ctx, &parcel)?;
        let paths = parcel
            .paths
            .into_iter()
            .map(|enclosed| enclosed.path)
            .collect::<Vec<_>>();
        let measured =
            match self.reading(&ctx, parcel.agent, &paths).await? {
                Ok(placement) => {
                    self.on_env(placement, move |env| {
                        measure(env, &paths)
                    })
                    .await?
                }
                Err(why) => Err(why),
            };
        Ok(match measured {
            Ok(files) => ParcelAnswer::Measured { files },
            Err(why) => ParcelAnswer::Refused { why },
        })
    }
}

impl Handler<ParcelPart> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: ParcelPart,
    ) -> Result<Part<ParcelRefusal>> {
        let Realm = sender(&ctx, &asked)?;
        let ParcelPart { agent, path, part } = asked;
        let paths = [path];
        let read = match self.reading(&ctx, agent, &paths).await? {
            Ok(placement) => {
                let [path] = paths;
                self.on_env(placement, move |env| {
                    read_part(env, &path, part)
                })
                .await?
            }
            Err(why) => Err(why),
        };
        Ok(match read {
            Ok((bytes, last)) => Part::Bytes {
                bytes: Base64(bytes),
                last,
            },
            Err(why) => Part::Refused { why },
        })
    }
}

impl Workspace {
    /// Where `agent`'s file tools run, when its reach lets it read
    /// each of `paths`.
    async fn reading(
        &self,
        ctx: &Ctx<Self>,
        agent: AgentNumber,
        paths: &[PathBuf],
    ) -> Result<Result<Placement, ParcelRefusal>> {
        if let Some(path) =
            paths.iter().find(|path| !is_absolute(&text(path)))
        {
            return Ok(Err(InvalidPath::NotAbsolute {
                path: path.clone(),
            }
            .into()));
        }
        self.weighed(ctx, move || async move {
            let reach = read_only(ctx, async |tx| {
                reach::read(tx, agent).await
            })
            .await?;
            let (held, _) = self.held();
            for path in paths {
                let op = Op::Read { path };
                match reach::gate(
                    &self.home,
                    agent,
                    reach.as_ref(),
                    held,
                    &op,
                ) {
                    Gate::Run { .. } => {}
                    Gate::Refuse(refused) => {
                        let why = of_read(refused.of_read()?);
                        return Ok(Weighing::Done(Err(why)));
                    }
                    Gate::NeedsSecrets(version) => {
                        return Ok(Weighing::NeedsSecrets(version));
                    }
                    Gate::NeedsChief(_) => {
                        bail!("the gate asked a chief for a read")
                    }
                }
            }
            let reach = reach
                .context("the gate passed a read with no reach")?;
            let placement =
                self.placement(agent, &reach, Purpose::FileTool)?;
            Ok(Weighing::Done(Ok(placement)))
        })
        .await
    }

    /// The outcome of `work`, run off the runtime on the file tools'
    /// environment at `placement`.
    async fn on_env<T: Send + 'static>(
        &self,
        placement: Placement,
        work: impl FnOnce(&Env) -> Result<T, ParcelRefusal>
        + Send
        + 'static,
    ) -> Result<Result<T, ParcelRefusal>> {
        let sandbox = self.sandbox;
        tokio::task::spawn_blocking(move || {
            let env = Env::new(
                &text(&placement.cwd),
                placement.policy,
                sandbox,
            )?;
            Ok(work(&env))
        })
        .await
        .context("a parcel's thread ended before it")?
    }
}

fn of_read(why: ReadRefusal) -> ParcelRefusal {
    match why {
        ReadRefusal::Invalid(why) => why.into(),
        ReadRefusal::Path(why) => why.into(),
        ReadRefusal::Reach(why) => why.into(),
    }
}

fn unreadable(path: &Path, words: String) -> ParcelRefusal {
    ParcelFault::Unreadable {
        path: path.to_owned(),
        words,
    }
    .into()
}

fn failed(path: &Path) -> impl Fn(FileError) -> ParcelRefusal {
    move |error| unreadable(path, error.message)
}

/// A file a parcel's walk found: its name from the folder that holds
/// what was named, and its path.
struct Found {
    name: String,
    path: String,
}

/// The files under `paths`, each hashed, within today's limits.
// Ported from the server's `work/tools/parcel.rs` `find`.
fn measure(
    env: &Env,
    paths: &[PathBuf],
) -> Result<Vec<ParcelEntry>, ParcelRefusal> {
    let mut found = Vec::<Found>::new();
    let mut bytes = 0u64;
    let mut visited = 0;
    for given in paths {
        let full = text(given);
        let info = env.file_info(&full).map_err(failed(given))?;
        if info.kind == FileKind::Symlink {
            return Err(ParcelFault::Link {
                path: given.clone(),
            }
            .into());
        }
        if info.name.is_empty() {
            return Err(ParcelFault::NoName {
                path: given.clone(),
            }
            .into());
        }
        let mut stack: Vec<(String, String, FileInfo)> =
            vec![(info.name.clone(), full, info)];
        while let Some((name, full, info)) = stack.pop() {
            visited += 1;
            if visited > VISITED {
                return Err(ParcelFault::TooManyEntries.into());
            }
            if TreePath::parse(&name).is_err() {
                return Err(ParcelFault::NotAName {
                    path: name.into(),
                }
                .into());
            }
            match info.kind {
                FileKind::File => {
                    if info.size > AttachedFile::MOST_FILE {
                        return Err(ParcelFault::FileTooLarge {
                            path: name.into(),
                            most: AttachedFile::MOST_FILE,
                        }
                        .into());
                    }
                    bytes += info.size;
                    if bytes > AttachedFile::MOST_BYTES {
                        return Err(ParcelFault::TooMuch {
                            most: AttachedFile::MOST_BYTES,
                        }
                        .into());
                    }
                    if found.len() >= AttachedFile::MOST_FILES {
                        return Err(ParcelFault::TooManyFiles {
                            most: AttachedFile::MOST_FILES as u32,
                        }
                        .into());
                    }
                    found.push(Found { name, path: full });
                }
                FileKind::Directory => {
                    let mut entries = env
                        .list_dir(&full)
                        .map_err(failed(Path::new(&name)))?;
                    // Reversed, so they come off the stack in order.
                    entries.sort_by(|a, b| b.name.cmp(&a.name));
                    for entry in entries {
                        if entry.kind == FileKind::Symlink {
                            continue;
                        }
                        stack.push((
                            format!("{name}/{}", entry.name),
                            path::join(&[full.as_str(), &entry.name]),
                            entry,
                        ));
                    }
                }
                FileKind::Symlink => {}
            }
        }
    }
    if found.is_empty() {
        return Err(ParcelFault::NoFiles.into());
    }
    found.into_iter().map(|file| hashed(env, file)).collect()
}

/// `found` with its size and hash, as its bytes are now.
fn hashed(
    env: &Env,
    found: Found,
) -> Result<ParcelEntry, ParcelRefusal> {
    let name = PathBuf::from(found.name);
    let (mut file, _) =
        env.open_file(&found.path).map_err(failed(&name))?;
    let mut hash = sha2::Sha256::new();
    let size = std::io::copy(&mut file, &mut hash)
        .map_err(|error| unreadable(&name, error.to_string()))?;
    Ok(ParcelEntry {
        path: PathBuf::from(found.path),
        name,
        size,
        sha256: Sha256(hash.finalize().into()),
    })
}

/// The bytes of part `part` of the file at `path`, and whether it is
/// the file's last.
fn read_part(
    env: &Env,
    path: &Path,
    part: u32,
) -> Result<(Vec<u8>, bool), ParcelRefusal> {
    let shown = text(path);
    if env.file_info(&shown).map_err(failed(path))?.kind
        == FileKind::Symlink
    {
        return Err(ParcelFault::Link {
            path: path.to_owned(),
        }
        .into());
    }
    let (mut file, _) =
        env.open_file(&shown).map_err(failed(path))?;
    let io =
        |error: std::io::Error| unreadable(path, error.to_string());
    let size = file.metadata().map_err(io)?.len();
    let start = u64::from(part) * PARCEL_PART as u64;
    if start > size || (start == size && size > 0) {
        return Err(ParcelFault::NoSuchPart {
            path: path.to_owned(),
            part,
        }
        .into());
    }
    file.seek(SeekFrom::Start(start)).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(PARCEL_PART as u64)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    let last = start + bytes.len() as u64 >= size;
    Ok((bytes, last))
}
