//! File operations executed only inside the operation namespace.

use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::Result;
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, mkdirat, openat, statat,
    unlinkat,
};
use rustix::io::Errno;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::wire::{
    self, Bytes, Canonical, Code, Contents, Info, InitHouse, Kind,
    LIST_ENTRIES, List, Listed, MAX_BODY, MAX_FILE, Mkdir, Operation,
    Read, Remove, Reply, SetMode, Stat, TREE_BYTES, TREE_ENTRIES,
    TreeEntry, TreeKind, TreeList, TreeWrite, Walk, Walked, Write,
};

const FAILED: &str = "The workspace file operation failed. Check \
                      the path and current contents before \
                      repeating a write.";

pub fn run() -> Result<()> {
    let operation = serde_json::from_reader::<_, Operation>(
        io::stdin().take(MAX_BODY as u64),
    )?;
    let reply = Root::open(Path::new("/"))?.perform(operation);
    serde_json::to_writer(io::stdout(), &reply)?;
    Ok(())
}

/// The operation's filesystem, reached from its root folder.
pub struct Root {
    path: PathBuf,
    dir: File,
}

impl Root {
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            path: path.to_owned(),
            dir: File::open(path)?,
        })
    }

    pub fn perform(&self, operation: Operation) -> Reply {
        match operation {
            Operation::InitHouse(call) => {
                answer(self.init_house(call))
            }
            Operation::TreeList(call) => answer(self.tree_list(call)),
            Operation::TreeWrite(call) => {
                answer(self.tree_write(call))
            }
            Operation::SetMode(call) => answer(self.set_mode(call)),
            Operation::Read(call) => answer(self.read(call)),
            Operation::Info(call) => answer(self.info(call)),
            Operation::List(call) => answer(self.list(call)),
            Operation::Canonical(call) => {
                answer(self.canonical(call))
            }
            Operation::Write(call) => answer(self.write(call)),
            Operation::Mkdir(call) => answer(self.mkdir(call)),
            Operation::Remove(call) => answer(self.remove(call)),
            Operation::Walk(call) => answer(self.walk(call)),
            Operation::Exec(_) => {
                answer::<()>(Err(io::ErrorKind::Unsupported.into()))
            }
        }
    }

    /// `path` on the host, following links.
    fn real(&self, path: &str) -> PathBuf {
        self.path.join(path.trim_start_matches('/'))
    }

    /// The folder holding `path`, reached without following a link,
    /// and its last name. `create` makes the missing folders.
    fn parent(
        &self,
        path: &str,
        create: bool,
    ) -> io::Result<(File, String)> {
        let names = path
            .split('/')
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if names.iter().any(|name| [".", ".."].contains(name)) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let Some((last, folders)) = names.split_last() else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        let mut dir = self.dir.try_clone()?;
        for name in folders {
            dir = folder(&dir, name, create)?;
        }
        Ok((dir, (*last).to_owned()))
    }

    /// The regular file at `path`, opened for reading.
    fn opened(
        &self,
        path: &str,
        no_follow: bool,
    ) -> io::Result<File> {
        if no_follow {
            let (dir, name) = self.parent(path, false)?;
            return Ok(File::from(
                openat(
                    &dir,
                    name.as_str(),
                    OFlags::RDONLY
                        | OFlags::NONBLOCK
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(no_link)?,
            ));
        }
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(self.real(path))
    }

    fn init_house(&self, _: InitHouse) -> io::Result<()> {
        let house = folder(&self.dir, "house", false)?;
        for name in ["skills", "inbox"] {
            folder(&house, name, true)?;
        }
        match openat(
            &house,
            "NOTES.md",
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::NOFOLLOW
                | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o666),
        ) {
            Ok(fd) => {
                let mut file = File::from(fd);
                file.write_all(b"# Notes\n")?;
                file.sync_all()
            }
            Err(Errno::EXIST) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn tree_list(
        &self,
        call: TreeList,
    ) -> io::Result<Vec<TreeEntry>> {
        if call.entries > TREE_ENTRIES || call.bytes > TREE_BYTES {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let (parent, name) = self.parent(&call.path, false)?;
        let root = folder(&parent, &name, false)?;
        let stat = rustix::fs::fstat(&root)?;
        let mut entries = vec![TreeEntry {
            path: ".".into(),
            kind: TreeKind::Directory,
            mode: stat.st_mode & 0o777,
            size: stat.st_size as u64,
            sha256: None,
        }];
        let mut remaining = call.bytes;
        tree(&root, "", call.entries, &mut remaining, &mut entries)?;
        Ok(entries)
    }

    fn tree_write(&self, call: TreeWrite) -> io::Result<()> {
        if call.data.0.len() > MAX_FILE || call.mode > 0o777 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let (dir, name) = self.parent(&call.path, true)?;
        let mut file = File::from(
            openat(
                &dir,
                name.as_str(),
                OFlags::WRONLY
                    | OFlags::CREATE
                    | OFlags::EXCL
                    | OFlags::NOFOLLOW
                    | OFlags::CLOEXEC,
                Mode::from_raw_mode(call.mode),
            )
            .map_err(no_link)?,
        );
        file.write_all(&call.data.0)?;
        file.set_permissions(fs::Permissions::from_mode(call.mode))?;
        file.sync_all()
    }

    fn set_mode(&self, call: SetMode) -> io::Result<()> {
        if call.mode > 0o777 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let (dir, name) = self.parent(&call.path, false)?;
        // A path descriptor needs no read permission and is the link
        // itself where the name is one.
        let target = openat(
            &dir,
            name.as_str(),
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let kind = FileType::from_raw_mode(
            rustix::fs::fstat(&target)?.st_mode,
        );
        if ![FileType::RegularFile, FileType::Directory]
            .contains(&kind)
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        fs::set_permissions(
            format!("/proc/self/fd/{}", target.as_raw_fd()),
            fs::Permissions::from_mode(call.mode),
        )
    }

    fn read(&self, call: Read) -> io::Result<Contents> {
        let mut file = self.opened(&call.path, call.no_follow)?;
        let stat = rustix::fs::fstat(&file)?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::Directory => {
                return Err(io::ErrorKind::IsADirectory.into());
            }
            FileType::RegularFile
                if stat.st_size as u64 <= MAX_FILE as u64 => {}
            _ => return Err(io::ErrorKind::InvalidInput.into()),
        }
        let mut data = Vec::new();
        (&mut file)
            .take(MAX_FILE as u64 + 1)
            .read_to_end(&mut data)?;
        if data.len() > MAX_FILE {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(Contents {
            data: Bytes(data),
            info: described(&call.path, &stat)?,
        })
    }

    fn info(&self, call: Info) -> io::Result<Stat> {
        let stat = if call.no_follow {
            let (dir, name) = self.parent(&call.path, false)?;
            statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?
        } else {
            rustix::fs::lstat(self.real(&call.path))?
        };
        described(&call.path, &stat)
    }

    fn list(&self, call: List) -> io::Result<Vec<Stat>> {
        let mut entries = Vec::new();
        let mut add =
            |name: &str, stat: io::Result<rustix::fs::Stat>| {
                let path = format!(
                    "{}/{name}",
                    call.path.trim_end_matches('/')
                );
                match stat.and_then(|stat| described(&path, &stat)) {
                    Ok(entry) => entries.push(entry),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound
                                | io::ErrorKind::Unsupported
                        ) => {}
                    Err(error) => return Err(error),
                }
                if entries.len() > LIST_ENTRIES {
                    return Err(io::ErrorKind::Unsupported.into());
                }
                Ok(())
            };
        if call.no_follow {
            let (parent, name) = self.parent(&call.path, false)?;
            let dir = folder(&parent, &name, false)?;
            for name in names(&dir)? {
                let stat = statat(
                    &dir,
                    name.as_str(),
                    AtFlags::SYMLINK_NOFOLLOW,
                )
                .map_err(io::Error::from);
                add(&name, stat)?;
            }
        } else {
            for entry in fs::read_dir(self.real(&call.path))? {
                let entry = entry?;
                let stat = rustix::fs::lstat(entry.path())
                    .map_err(io::Error::from);
                add(&entry.file_name().to_string_lossy(), stat)?;
            }
        }
        Ok(entries)
    }

    fn canonical(&self, call: Canonical) -> io::Result<String> {
        let canonical = fs::canonicalize(self.real(&call.path))?;
        let inside = canonical
            .strip_prefix(&self.path)
            .map_err(|_| io::ErrorKind::PermissionDenied)?;
        Ok(format!("/{}", inside.to_string_lossy()))
    }

    fn write(&self, call: Write) -> io::Result<()> {
        if call.data.0.len() > MAX_FILE {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut file = if call.no_follow {
            let (dir, name) =
                self.parent(&call.path, call.parents)?;
            let how = if call.append {
                OFlags::APPEND
            } else {
                OFlags::TRUNC
            };
            File::from(
                openat(
                    &dir,
                    name.as_str(),
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::NOFOLLOW
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC
                        | how,
                    Mode::from_raw_mode(0o666),
                )
                .map_err(no_link)?,
            )
        } else {
            let path = self.real(&call.path);
            if call.parents
                && let Some(parent) = path.parent()
            {
                fs::create_dir_all(parent)?;
            }
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(!call.append)
                .append(call.append)
                .custom_flags(libc::O_NONBLOCK)
                .open(path)?
        };
        if !file.metadata()?.is_file() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        file.write_all(&call.data.0)
    }

    fn mkdir(&self, call: Mkdir) -> io::Result<()> {
        if call.no_follow {
            let (dir, name) =
                self.parent(&call.path, call.recursive)?;
            if call.recursive {
                folder(&dir, &name, true).map(drop)
            } else {
                Ok(mkdirat(
                    &dir,
                    name.as_str(),
                    Mode::from_raw_mode(0o777),
                )?)
            }
        } else if call.recursive {
            fs::create_dir_all(self.real(&call.path))
        } else {
            fs::create_dir(self.real(&call.path))
        }
    }

    fn remove(&self, call: Remove) -> io::Result<()> {
        let result = if call.no_follow {
            self.parent(&call.path, false).and_then(|(dir, name)| {
                let stat = statat(
                    &dir,
                    name.as_str(),
                    AtFlags::SYMLINK_NOFOLLOW,
                )?;
                if FileType::from_raw_mode(stat.st_mode)
                    == FileType::Directory
                {
                    if call.recursive {
                        clear(&folder(&dir, &name, false)?)?;
                    }
                    unlinkat(
                        &dir,
                        name.as_str(),
                        AtFlags::REMOVEDIR,
                    )?;
                } else {
                    unlinkat(&dir, name.as_str(), AtFlags::empty())?;
                }
                Ok(())
            })
        } else {
            let path = self.real(&call.path);
            fs::symlink_metadata(&path).and_then(|metadata| {
                if !metadata.is_dir() {
                    fs::remove_file(path)
                } else if call.recursive {
                    fs::remove_dir_all(path)
                } else {
                    fs::remove_dir(path)
                }
            })
        };
        match result {
            Err(error)
                if call.force
                    && error.kind() == io::ErrorKind::NotFound =>
            {
                Ok(())
            }
            result => result,
        }
    }

    fn walk(&self, call: Walk) -> io::Result<Walked> {
        Ok(wire::walk(
            &call.path,
            &call.skip,
            call.files.min(TREE_ENTRIES),
            |dir| {
                Ok::<_, io::Error>(
                    fs::read_dir(self.real(dir))?
                        .filter_map(|entry| {
                            let entry = entry.ok()?;
                            let metadata =
                                fs::symlink_metadata(entry.path())
                                    .ok()?;
                            let kind = if metadata.is_file() {
                                Listed::File(metadata.len())
                            } else if metadata.is_dir() {
                                Listed::Directory
                            } else {
                                Listed::Other
                            };
                            Some((
                                entry
                                    .file_name()
                                    .to_string_lossy()
                                    .into(),
                                kind,
                            ))
                        })
                        .collect(),
                )
            },
        ))
    }
}

fn answer<T: Serialize>(result: io::Result<T>) -> Reply {
    match result {
        Ok(value) => Reply::ok(value),
        Err(error) => {
            let code = match error.kind() {
                io::ErrorKind::NotFound => Code::NotFound,
                io::ErrorKind::AlreadyExists => Code::AlreadyExists,
                io::ErrorKind::PermissionDenied => {
                    Code::PermissionDenied
                }
                io::ErrorKind::NotADirectory => Code::NotDirectory,
                io::ErrorKind::IsADirectory => Code::IsDirectory,
                io::ErrorKind::InvalidInput => Code::Invalid,
                io::ErrorKind::Unsupported => Code::NotSupported,
                _ => Code::Unknown,
            };
            Reply::error(code, FAILED, false)
        }
    }
}

/// A link or a file where a folder was expected, refused as invalid.
fn no_link(error: Errno) -> io::Error {
    if [Errno::LOOP, Errno::NOTDIR].contains(&error) {
        io::ErrorKind::InvalidInput.into()
    } else {
        error.into()
    }
}

/// The folder `name` in `parent`, never through a link. `create`
/// makes it where it is missing.
fn folder(
    parent: &File,
    name: &str,
    create: bool,
) -> io::Result<File> {
    if create
        && let Err(error) =
            mkdirat(parent, name, Mode::from_raw_mode(0o777))
        && error != Errno::EXIST
    {
        return Err(no_link(error));
    }
    Ok(File::from(
        openat(
            parent,
            name,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(no_link)?,
    ))
}

fn names(dir: &File) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in Dir::read_from(dir)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| io::ErrorKind::InvalidInput)?;
        if ![".", ".."].contains(&name) {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

/// Removes everything in `dir`, following no link.
fn clear(dir: &File) -> io::Result<()> {
    for name in names(dir)? {
        let stat =
            statat(dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(stat.st_mode)
            == FileType::Directory
        {
            clear(&folder(dir, &name, false)?)?;
            unlinkat(dir, name.as_str(), AtFlags::REMOVEDIR)?;
        } else {
            unlinkat(dir, name.as_str(), AtFlags::empty())?;
        }
    }
    Ok(())
}

fn tree(
    dir: &File,
    prefix: &str,
    cap: u64,
    remaining: &mut u64,
    entries: &mut Vec<TreeEntry>,
) -> io::Result<()> {
    for name in names(dir)? {
        if entries.len() as u64 >= cap {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let stat =
            statat(dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        let path = format!("{prefix}{name}");
        let mut entry = TreeEntry {
            path: path.clone(),
            kind: TreeKind::Special,
            mode: stat.st_mode & 0o777,
            size: stat.st_size as u64,
            sha256: None,
        };
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::Directory => {
                entry.kind = TreeKind::Directory;
                entries.push(entry);
                let child = folder(dir, &name, false)?;
                tree(
                    &child,
                    &format!("{path}/"),
                    cap,
                    remaining,
                    entries,
                )?;
                continue;
            }
            FileType::RegularFile => {
                let (size, sha256) = digest(dir, &name, remaining)?;
                entry.kind = TreeKind::File;
                entry.size = size;
                entry.sha256 = Some(sha256);
            }
            FileType::Symlink => entry.kind = TreeKind::Link,
            _ => {}
        }
        entries.push(entry);
    }
    Ok(())
}

/// The size and SHA-256 of the regular file `name`, counted against
/// `remaining` bytes.
fn digest(
    dir: &File,
    name: &str,
    remaining: &mut u64,
) -> io::Result<(u64, String)> {
    let mut file = File::from(openat(
        dir,
        name,
        OFlags::RDONLY
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if !file.metadata()?.is_file() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        *remaining = remaining
            .checked_sub(count as u64)
            .ok_or(io::ErrorKind::InvalidInput)?;
        size += count as u64;
        digest.update(&buffer[..count]);
    }
    Ok((size, format!("{:x}", digest.finalize())))
}

/// `stat` of the file, folder or link at `path`; other kinds are not
/// supported.
fn described(
    path: &str,
    stat: &rustix::fs::Stat,
) -> io::Result<Stat> {
    let kind = match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => Kind::File,
        FileType::Directory => Kind::Directory,
        FileType::Symlink => Kind::Symlink,
        _ => return Err(io::ErrorKind::Unsupported.into()),
    };
    Ok(Stat {
        name: path.rsplit('/').next().unwrap_or_default().into(),
        path: path.into(),
        kind,
        size: stat.st_size as u64,
        mtime_ms: (stat.st_mtime as u64)
            .saturating_mul(1000)
            .saturating_add(stat.st_mtime_nsec / 1_000_000),
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "hamlet-files-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(path.join("house")).unwrap();
            fs::create_dir_all(path.join("outside")).unwrap();
            fs::write(path.join("outside/secret"), "canary").unwrap();
            symlink(path.join("outside"), path.join("house/link"))
                .unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn code(reply: &Reply) -> Option<Code> {
        match reply {
            Reply::Ok { .. } => None,
            Reply::Error { code, .. } => Some(*code),
        }
    }

    #[test]
    fn house_operations_refuse_a_link_anywhere_and_tools_follow_it() {
        let scratch = Scratch::new();
        let root = Root::open(&scratch.0).unwrap();
        let read = |no_follow| {
            root.perform(Operation::Read(Read {
                path: "/house/link/secret".into(),
                no_follow,
            }))
        };
        assert_eq!(code(&read(false)), None);
        assert_eq!(code(&read(true)), Some(Code::Invalid));
        let written = root.perform(Operation::TreeWrite(TreeWrite {
            path: "/house/link/new/file".into(),
            data: Bytes(b"escaped".to_vec()),
            mode: 0o644,
        }));
        assert_eq!(code(&written), Some(Code::Invalid));
        let removed = root.perform(Operation::Remove(Remove {
            path: "/house/link/secret".into(),
            recursive: false,
            force: false,
            no_follow: true,
        }));
        assert_eq!(code(&removed), Some(Code::Invalid));
        let mode = root.perform(Operation::SetMode(SetMode {
            path: "/house/link".into(),
            mode: 0o700,
        }));
        assert_eq!(code(&mode), Some(Code::Invalid));
        assert!(!scratch.0.join("outside/new").exists());
        assert_eq!(
            fs::read_to_string(scratch.0.join("outside/secret"))
                .unwrap(),
            "canary"
        );
        let removed = root.perform(Operation::Remove(Remove {
            path: "/house".into(),
            recursive: true,
            force: false,
            no_follow: true,
        }));
        assert_eq!(code(&removed), None);
        assert!(!scratch.0.join("house").exists());
        assert!(scratch.0.join("outside/secret").is_file());
    }

    #[test]
    fn tree_writes_are_new_files_whose_folders_take_modes_after() {
        let scratch = Scratch::new();
        let root = Root::open(&scratch.0).unwrap();
        let write = || {
            root.perform(Operation::TreeWrite(TreeWrite {
                path: "/house/a/b/run".into(),
                data: Bytes(vec![0, 255]),
                mode: 0o751,
            }))
        };
        assert_eq!(code(&write()), None);
        assert_eq!(code(&write()), Some(Code::AlreadyExists));
        let mode = root.perform(Operation::SetMode(SetMode {
            path: "/house/a".into(),
            mode: 0o555,
        }));
        assert_eq!(code(&mode), None);
        let Reply::Ok { value } =
            root.perform(Operation::TreeList(TreeList {
                path: "/house/a".into(),
                entries: 10,
                bytes: 10,
            }))
        else {
            panic!("tree list failed");
        };
        let entries =
            serde_json::from_value::<Vec<TreeEntry>>(value).unwrap();
        let modes = entries
            .iter()
            .map(|entry| {
                (entry.path.as_str(), entry.kind, entry.mode)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            modes,
            [
                (".", TreeKind::Directory, 0o555),
                ("b", TreeKind::Directory, entries[1].mode),
                ("b/run", TreeKind::File, 0o751),
            ]
        );
        assert_eq!(
            entries[2].sha256.as_deref(),
            Some(format!("{:x}", Sha256::digest([0, 255])).as_str())
        );
        fs::set_permissions(
            scratch.0.join("house/a"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
}
