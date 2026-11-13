// This file was written for Hamlet and holds no code from another
// project. See NOTICE in this crate.
//! The agent's filesystem for the Node host: a folder reached only
//! through `hamlet_os::fs::Folder`, so no call follows a link or a
//! path out of it.

use std::io;
use std::sync::Arc;

use hamlet_os::fs::{Folder, Kind, Outside, system_error};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use regex::RegexBuilder;

/// The largest file a search reads, in bytes.
const LARGEST: u64 = 1024 * 1024;

/// The most characters of a matching line a search gives.
const WIDTH: usize = 1024;

/// One name in a folder, a link itself rather than what it names.
#[napi(object)]
pub struct FolderEntry {
    pub name: String,
    /// Its path from the agent's filesystem.
    pub path: String,
    /// `file`, `directory` or `symlink`.
    pub kind: String,
    pub size: f64,
}

/// A regular file, by its path from the agent's filesystem.
#[napi(object)]
pub struct FolderFile {
    pub path: String,
    pub size: f64,
}

#[napi(object)]
pub struct FolderFiles {
    /// The path from the agent's filesystem the walk started at.
    pub start: String,
    pub files: Vec<FolderFile>,
    /// The walk stopped at its bound.
    pub capped: bool,
}

/// A line that matched a search, numbered from 1.
#[napi(object)]
pub struct FolderMatch {
    pub path: String,
    pub line: u32,
    pub text: String,
}

#[napi(object)]
pub struct FolderMatches {
    pub matches: Vec<FolderMatch>,
    /// The search stopped at a bound.
    pub capped: bool,
}

/// An agent's filesystem, held open. Each call takes a path relative
/// to it, or an absolute path below it, and fails with a message that
/// starts with the system error's name, such as `ENOENT: `, or with
/// `OUTSIDE: ` for a path that leaves the folder.
#[napi]
pub struct AgentFolder(Arc<Folder>);

#[napi]
impl AgentFolder {
    /// Opens the folder at `root`, making it and each of `folders`
    /// inside it where missing.
    #[napi(factory)]
    pub fn open(
        root: String,
        folders: Vec<String>,
    ) -> napi::Result<Self> {
        let folder = Folder::make(std::path::Path::new(&root))
            .map_err(refused)?;
        for inside in folders {
            folder.make_folder(&inside).map_err(refused)?;
        }
        Ok(Self(Arc::new(folder)))
    }

    /// The folder's absolute path, with `/` between names.
    #[napi(getter)]
    pub fn path(&self) -> String {
        self.0.path().to_owned()
    }

    #[napi]
    pub async fn read(&self, path: String) -> napi::Result<Buffer> {
        blocking(&self.0, move |folder| folder.read(&path))
            .await
            .map(Buffer::from)
    }

    #[napi]
    pub async fn write(
        &self,
        path: String,
        bytes: Buffer,
    ) -> napi::Result<()> {
        let bytes = bytes.to_vec();
        blocking(&self.0, move |folder| folder.write(&path, &bytes))
            .await
    }

    /// The names in the folder at `path`, sorted. Names that are
    /// neither a file, a folder nor a link are left out.
    #[napi]
    pub async fn list(
        &self,
        path: String,
    ) -> napi::Result<Vec<FolderEntry>> {
        blocking(&self.0, move |folder| {
            Ok(folder
                .list(&path)?
                .into_iter()
                .filter_map(|listed| {
                    let kind = match listed.stat.kind {
                        Kind::File => "file",
                        Kind::Directory => "directory",
                        Kind::Link => "symlink",
                        Kind::Special => return None,
                    };
                    Some(FolderEntry {
                        name: listed.name,
                        path: listed.path,
                        kind: kind.to_owned(),
                        size: listed.stat.size as f64,
                    })
                })
                .collect())
        })
        .await
    }

    /// Every regular file with one name at or below `path`, after at
    /// most `most` names looked at. No link is followed.
    #[napi]
    pub async fn files(
        &self,
        path: String,
        most: u32,
    ) -> napi::Result<FolderFiles> {
        blocking(&self.0, move |folder| {
            let walked = folder.files(&path, most as usize)?;
            Ok(FolderFiles {
                start: walked.start,
                files: walked
                    .files
                    .into_iter()
                    .map(|found| FolderFile {
                        path: found.path,
                        size: found.size as f64,
                    })
                    .collect(),
                capped: walked.capped,
            })
        })
        .await
    }

    /// The lines that match `pattern`, a regular expression that runs
    /// in linear time, in the text files at or below `path`, up to
    /// `most` of them, after at most `looked` names. A file over 1
    /// MiB, one that is not UTF-8 and one holding NUL are passed
    /// over.
    #[napi]
    pub async fn search(
        &self,
        path: String,
        pattern: String,
        ignore_case: bool,
        most: u32,
        looked: u32,
    ) -> napi::Result<FolderMatches> {
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|error| failure("EINVAL", &error.to_string()))?;
        blocking(&self.0, move |folder| {
            let walked = folder.files(&path, looked as usize)?;
            let mut matches = Vec::new();
            for file in walked.files {
                if file.size > LARGEST {
                    continue;
                }
                let Ok(bytes) = folder.read(&file.path) else {
                    continue;
                };
                let Ok(text) = String::from_utf8(bytes) else {
                    continue;
                };
                if text.contains('\0') {
                    continue;
                }
                for (index, line) in text.lines().enumerate() {
                    if !regex.is_match(line) {
                        continue;
                    }
                    if matches.len() == most as usize {
                        return Ok(FolderMatches {
                            matches,
                            capped: true,
                        });
                    }
                    matches.push(FolderMatch {
                        path: file.path.clone(),
                        line: u32::try_from(index + 1)
                            .unwrap_or(u32::MAX),
                        text: line.chars().take(WIDTH).collect(),
                    });
                }
            }
            Ok(FolderMatches {
                matches,
                capped: walked.capped,
            })
        })
        .await
    }
}

/// Runs `work` on the folder on a thread that may block.
async fn blocking<T: Send + 'static>(
    folder: &Arc<Folder>,
    work: impl FnOnce(&Folder) -> io::Result<T> + Send + 'static,
) -> napi::Result<T> {
    let folder = Arc::clone(folder);
    tokio::task::spawn_blocking(move || work(&folder))
        .await
        .map_err(|error| failure("EIO", &error.to_string()))?
        .map_err(refused)
}

/// `error` with its system error's name first.
fn refused(error: io::Error) -> napi::Error {
    if Outside::is(&error) {
        return failure("OUTSIDE", "outside the agent's filesystem");
    }
    match system_error(&error) {
        Some(known) => failure(known.name, known.words),
        None => match error.kind() {
            io::ErrorKind::PermissionDenied => {
                failure("EACCES", &error.to_string())
            }
            _ => failure("EINVAL", &error.to_string()),
        },
    }
}

fn failure(name: &str, words: &str) -> napi::Error {
    napi::Error::from_reason(format!("{name}: {words}"))
}
