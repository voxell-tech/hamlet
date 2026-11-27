// This file was written for Hamlet and holds no code from another
// project. See NOTICE in this crate.
//! The agent's filesystem for the Node host: a folder reached only
//! through `hamlet_os::fs::Folder`, so no call follows a link or a
//! path out of it.

use std::io;
use std::sync::Arc;

use hamlet_os::fs::{Folder, Kind, Mode, Outside, system_error};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use regex::Regex;

/// The largest file a search reads, in bytes.
const LARGEST: u64 = 1024 * 1024;

/// The most characters of a matching line a search shows; a longer
/// line ends in `...` there.
const WIDTH: usize = 300;

/// The folders a walk does not go into below where it starts.
const SKIPPED: [&str; 9] = [
    ".git",
    ".hamlet",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "__pycache__",
    ".next",
];

/// One name in a folder, a link itself rather than what it names.
#[napi(object)]
pub struct FolderEntry {
    pub name: String,
    /// Its path from the agent's filesystem.
    pub path: String,
    /// `file`, `directory` or `symlink`.
    pub kind: String,
    pub size: f64,
    /// Whether it is a file that runs. Never on Windows, which keeps
    /// no such mode.
    pub executable: bool,
}

/// A regular file, by its path from the agent's filesystem.
#[napi(object)]
pub struct FolderFile {
    pub path: String,
    pub size: f64,
    /// As [`FolderEntry::executable`].
    pub executable: bool,
}

#[napi(object)]
pub struct FolderFiles {
    pub files: Vec<FolderFile>,
    /// The walk stopped at its bound.
    pub capped: bool,
}

/// A line that matched a search, numbered from 1.
#[napi(object)]
pub struct FolderMatch {
    pub path: String,
    pub line: u32,
    /// The line as a search shows it.
    pub text: String,
}

#[napi(object)]
pub struct FolderMatches {
    pub matches: Vec<FolderMatch>,
    /// The walk stopped at its bound before the search found `most`.
    pub capped: bool,
}

/// An agent's filesystem, held open. Each call takes a path relative
/// to it, or an absolute path below it, and fails with a message that
/// starts with the system error's name, such as `ENOENT: `, or with
/// `OUTSIDE: ` for a path that leaves the folder.
#[napi]
pub struct AgentFolder {
    folder: Arc<Folder>,
    /// The folders inside it that are made again where a removal
    /// leaves them out.
    kept: Arc<[String]>,
}

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
        for inside in &folders {
            folder.make_folder(inside).map_err(refused)?;
        }
        Ok(Self {
            folder: Arc::new(folder),
            kept: folders.into(),
        })
    }

    /// The folder's absolute path, with `/` between names.
    #[napi(getter)]
    pub fn path(&self) -> String {
        self.folder.path().to_owned()
    }

    #[napi]
    pub async fn read(&self, path: String) -> napi::Result<Buffer> {
        blocking(&self.folder, move |folder| folder.read(&path))
            .await
            .map(Buffer::from)
    }

    /// Writes the file whole, and answers whether nothing was there
    /// before. With `executable`, the file runs or does not; without
    /// it, the file keeps the mode of the file it replaces. Windows
    /// keeps no mode that runs, so there `executable` changes
    /// nothing.
    #[napi]
    pub async fn write(
        &self,
        path: String,
        bytes: Buffer,
        executable: Option<bool>,
    ) -> napi::Result<bool> {
        let bytes = bytes.to_vec();
        blocking(&self.folder, move |folder| {
            let created = folder.stat(&path).is_err();
            let mode = executable.map(|runs| {
                if runs { Mode::PROGRAM } else { Mode::PRIVATE }
            });
            match mode {
                Some(mode) => {
                    folder.write_with_mode(&path, &bytes, mode)
                }
                None => folder.write(&path, &bytes),
            }?;
            Ok(created)
        })
        .await
    }

    /// Removes the file at `path`, a link itself, or the folder there
    /// with all it holds, and the folders on its way it leaves empty.
    /// A folder `open` made is made again.
    #[napi]
    pub async fn remove(&self, path: String) -> napi::Result<()> {
        let kept = Arc::clone(&self.kept);
        blocking(&self.folder, move |folder| {
            folder.remove(&path)?;
            for inside in kept.iter() {
                folder.make_folder(inside)?;
            }
            Ok(())
        })
        .await
    }

    /// The names in the folder at `path`, sorted. Names that are
    /// neither a file, a folder nor a link are left out.
    #[napi]
    pub async fn list(
        &self,
        path: String,
    ) -> napi::Result<Vec<FolderEntry>> {
        blocking(&self.folder, move |folder| {
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
                        executable: listed.stat.kind == Kind::File
                            && listed.stat.mode.is_executable(),
                    })
                })
                .collect())
        })
        .await
    }

    /// Every regular file with one name at or below `path`, after at
    /// most `most` names looked at, or only those `name` matches. In
    /// `name`, `*` stands for any run of characters and `?` for one;
    /// with a `/` in it, it is matched against the file's path from
    /// `path` rather than its name. No link is followed, and no
    /// folder a walk skips is gone into.
    #[napi]
    pub async fn files(
        &self,
        path: String,
        most: u32,
        name: Option<String>,
    ) -> napi::Result<FolderFiles> {
        let name = name.as_deref().map(Name::new).transpose()?;
        blocking(&self.folder, move |folder| {
            let walked =
                folder.files(&path, most as usize, &SKIPPED)?;
            let start = walked.start;
            Ok(FolderFiles {
                files: walked
                    .files
                    .into_iter()
                    .filter(|found| {
                        Name::admits(&name, &start, &found.path)
                    })
                    .map(|found| FolderFile {
                        path: found.path,
                        size: found.size as f64,
                        executable: found.mode.is_executable(),
                    })
                    .collect(),
                capped: walked.capped,
            })
        })
        .await
    }

    /// The lines that match `pattern`, a regular expression that runs
    /// in linear time, in the text files at or below `path` that
    /// `name` matches as in `files`, up to `most` of them, after at
    /// most `looked` names. A file over 1 MiB, one that is not UTF-8
    /// and one holding NUL are passed over.
    #[napi]
    pub async fn search(
        &self,
        path: String,
        pattern: String,
        ignore_case: bool,
        name: Option<String>,
        most: u32,
        looked: u32,
    ) -> napi::Result<FolderMatches> {
        let source = if ignore_case {
            lowered(&pattern)
        } else {
            pattern
        };
        let regex = Regex::new(&source)
            .map_err(|error| failure("EINVAL", &error.to_string()))?;
        let name = name.as_deref().map(Name::new).transpose()?;
        blocking(&self.folder, move |folder| {
            let walked =
                folder.files(&path, looked as usize, &SKIPPED)?;
            let mut matches = Vec::new();
            for file in walked.files {
                if file.size > LARGEST
                    || !Name::admits(&name, &walked.start, &file.path)
                {
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
                    let matched = if ignore_case {
                        regex.is_match(&line.to_lowercase())
                    } else {
                        regex.is_match(line)
                    };
                    if !matched {
                        continue;
                    }
                    if matches.len() == most as usize {
                        return Ok(FolderMatches {
                            matches,
                            capped: false,
                        });
                    }
                    matches.push(FolderMatch {
                        path: file.path.clone(),
                        line: u32::try_from(index + 1)
                            .unwrap_or(u32::MAX),
                        text: shown(line),
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

/// `pattern` lowercased for a case-insensitive search: all but the
/// character after each `\`.
fn lowered(pattern: &str) -> String {
    let mut source = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(&next) if c == '\\' => {
                source.push(c);
                source.push(next);
                chars.next();
            }
            _ => source.extend(c.to_lowercase()),
        }
    }
    source
}

/// `line` with pi's spaces taken from its end, then cut at [`WIDTH`]
/// characters, as a search shows it.
fn shown(line: &str) -> String {
    let line = line.trim_end_matches(|c: char| {
        (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
    });
    match line.char_indices().nth(WIDTH) {
        Some((end, _)) => format!("{}...", &line[..end]),
        None => line.to_owned(),
    }
}

struct Name {
    whole: Regex,
    by_path: bool,
}

impl Name {
    fn new(glob: &str) -> napi::Result<Self> {
        let mut source = String::from("(?s)^");
        for c in glob.chars() {
            match c {
                '*' => source.push_str(".*"),
                '?' => source.push('.'),
                c => source.push_str(&regex::escape(&c.to_string())),
            }
        }
        source.push('$');
        Ok(Self {
            whole: Regex::new(&source).map_err(|error| {
                failure("EINVAL", &error.to_string())
            })?,
            by_path: glob.contains('/'),
        })
    }

    fn admits(name: &Option<Self>, start: &str, file: &str) -> bool {
        let Some(name) = name else {
            return true;
        };
        fn base(path: &str) -> &str {
            path.rsplit('/').next().unwrap_or("")
        }
        let under = if file == start {
            base(file)
        } else if start.is_empty() {
            file
        } else {
            &file[start.len() + 1..]
        };
        name.whole.is_match(if name.by_path {
            under
        } else {
            base(under)
        })
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
