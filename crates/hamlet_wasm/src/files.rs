//! A plugin's folder of files, named by paths that cannot leave it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::limits::Limits;

/// The folder of a plugin's folder that holds its files.
pub const FILES: &str = "files";

/// The start of the name of a file being written. A name that starts
/// with a dot is never a path, so a file cannot be mistaken for it.
const WRITING: &str = ".tmp-";

/// What a folder of files may hold.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Caps {
    pub bytes: u64,
    pub files: usize,
    pub file_bytes: usize,
    pub path_bytes: usize,
}

impl Caps {
    pub fn of(limits: &Limits) -> Self {
        Self {
            bytes: limits.files_bytes,
            files: limits.files_count,
            file_bytes: limits.file_bytes,
            path_bytes: limits.file_path_bytes,
        }
    }
}

/// The files of one plugin, under `folder`. A path is relative, of
/// names made of letters, digits, `.`, `_`, `-` and spaces, none
/// starting with a dot, and no link on the way is followed.
#[derive(Clone, PartialEq, Debug)]
pub struct Files {
    folder: PathBuf,
    caps: Caps,
}

impl Files {
    pub fn new(folder: PathBuf, caps: Caps) -> Self {
        Self { folder, caps }
    }

    /// The bytes of the file at `path`.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let file = self.resolve(path)?;
        let meta = fs::symlink_metadata(&file)
            .map_err(|_| format!("`{path}` is not there"))?;
        if !meta.is_file() {
            return Err(format!("`{path}` is not a file"));
        }
        fs::read(&file)
            .map_err(|_| format!("`{path}` could not be read"))
    }

    /// Sets the file at `path` to `data`. A file is written beside it
    /// and renamed over it, so it is whole or not there.
    pub fn write(
        &self,
        path: &str,
        data: &[u8],
    ) -> Result<(), String> {
        let file = self.resolve(path)?;
        if data.len() > self.caps.file_bytes {
            return Err(format!(
                "a file has {} bytes, the most is {}",
                data.len(),
                self.caps.file_bytes
            ));
        }
        let old = fs::symlink_metadata(&file).ok();
        if old.as_ref().is_some_and(|meta| !meta.is_file()) {
            return Err(format!("`{path}` is not a file"));
        }
        let (mut count, mut bytes) = (0, 0);
        for (_, size) in self.entries() {
            count += 1;
            bytes += size;
        }
        if old.is_none() && count >= self.caps.files {
            return Err(format!(
                "the folder holds {count} files, the most is {}",
                self.caps.files
            ));
        }
        let after = bytes - old.map_or(0, |meta| meta.len())
            + data.len() as u64;
        if after > self.caps.bytes {
            return Err(format!(
                "the folder would hold {after} bytes, the most is {}",
                self.caps.bytes
            ));
        }
        self.put(&file, data)
            .map_err(|_| format!("`{path}` could not be written"))
    }

    /// Removes the file at `path`, if it is there.
    pub fn delete(&self, path: &str) -> Result<(), String> {
        let file = self.resolve(path)?;
        match fs::symlink_metadata(&file) {
            Ok(meta) if meta.is_file() => fs::remove_file(&file)
                .map_err(|_| {
                    format!("`{path}` could not be removed")
                }),
            Ok(_) => Err(format!("`{path}` is not a file")),
            Err(_) => Ok(()),
        }
    }

    /// The paths of the files held, in order.
    pub fn list(&self) -> Vec<String> {
        let mut paths = self
            .entries()
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    /// Where `path` is on disk, or why it cannot be a path.
    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let bad = || {
            format!(
                "`{}` is not a path: up to {} bytes of names of \
                 letters, digits, `.`, `_`, `-` and spaces, joined \
                 by `/`, none starting with `.`",
                path.escape_debug(),
                self.caps.path_bytes
            )
        };
        if path.is_empty() || path.len() > self.caps.path_bytes {
            return Err(bad());
        }
        let mut file = self.folder.clone();
        for name in path.split('/') {
            let good = |byte: &u8| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'.' | b'_' | b'-' | b' ')
            };
            if name.is_empty()
                || name.starts_with('.')
                || !name.as_bytes().iter().all(good)
            {
                return Err(bad());
            }
            file.push(name);
            if fs::symlink_metadata(&file)
                .is_ok_and(|meta| meta.file_type().is_symlink())
            {
                return Err(format!("`{path}` goes through a link"));
            }
        }
        Ok(file)
    }

    /// Each file held, as its path and its bytes.
    fn entries(&self) -> Vec<(String, u64)> {
        let mut found = Vec::new();
        walk(&self.folder, "", &mut found);
        found
    }

    fn put(&self, file: &Path, data: &[u8]) -> std::io::Result<()> {
        let parent = file.parent().unwrap_or(&self.folder);
        fs::create_dir_all(parent)?;
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temp = parent.join(format!("{WRITING}{name}"));
        let mut out = fs::File::create(&temp)?;
        let written =
            out.write_all(data).and_then(|()| out.sync_all());
        drop(out);
        let done = written.and_then(|()| fs::rename(&temp, file));
        if done.is_err() {
            let _ = fs::remove_file(&temp);
        }
        done
    }
}

/// Adds each file under `dir`, named by `base` and its name, to
/// `found`. Links and names that are not paths are passed over.
fn walk(dir: &Path, base: &str, found: &mut Vec<(String, u64)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let path = match base.is_empty() {
            true => name,
            false => format!("{base}/{name}"),
        };
        if meta.is_file() {
            found.push((path, meta.len()));
        } else if meta.is_dir() {
            walk(&entry.path(), &path, found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hamlet_files_{name}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn files(root: &Path) -> Files {
        Files::new(root.join(FILES), Caps::of(&Limits::default()))
    }

    #[test]
    fn a_file_is_written_read_listed_and_deleted() {
        let root = folder("round");
        let files = files(&root);
        files.write("a.txt", b"one").unwrap();
        files.write("notes/b.txt", b"two").unwrap();
        files.write("a.txt", b"three").unwrap();
        assert_eq!(files.read("a.txt").unwrap(), b"three");
        assert_eq!(files.list(), ["a.txt", "notes/b.txt"]);
        files.delete("a.txt").unwrap();
        assert!(files.read("a.txt").is_err());
        assert_eq!(files.list(), ["notes/b.txt"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_path_reaches_outside_the_folder() {
        let root = folder("outside");
        let files = files(&root);
        fs::write(root.join("secret"), b"x").unwrap();
        let outside = root.join("secret");
        for path in [
            "../secret",
            "a/../../secret",
            "..",
            ".",
            "/secret",
            outside.to_str().unwrap(),
            "a//b",
            "a/",
            "a\\b",
            "C:/x",
            ".hidden",
            "",
        ] {
            assert!(files.read(path).is_err(), "{path}");
            assert!(files.write(path, b"y").is_err(), "{path}");
            assert!(files.delete(path).is_err(), "{path}");
        }
        assert_eq!(fs::read(root.join("secret")).unwrap(), b"x");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_followed() {
        use std::os::unix::fs::symlink;

        let root = folder("links");
        let files = files(&root);
        files.write("kept.txt", b"in").unwrap();
        let elsewhere = root.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("secret"), b"x").unwrap();
        let inside = root.join(FILES);
        symlink(&elsewhere, inside.join("door")).unwrap();
        symlink(elsewhere.join("secret"), inside.join("peek"))
            .unwrap();

        assert!(files.read("door/secret").is_err());
        assert!(files.write("door/new", b"y").is_err());
        assert!(files.read("peek").is_err());
        assert!(files.write("peek", b"y").is_err());
        assert!(files.delete("peek").is_err());
        assert_eq!(files.list(), ["kept.txt"]);
        assert!(!elsewhere.join("new").exists());
        assert_eq!(fs::read(elsewhere.join("secret")).unwrap(), b"x");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_folder_holds_no_more_than_its_caps() {
        let root = folder("caps");
        let caps = Caps {
            bytes: 10,
            files: 2,
            file_bytes: 6,
            path_bytes: 8,
        };
        let files = Files::new(root.join(FILES), caps);
        assert!(files.write("a", b"1234567").is_err());
        assert!(files.write("toolongname", b"1").is_err());
        files.write("a", b"123456").unwrap();
        assert!(files.write("b", b"12345").is_err());
        files.write("b", b"1234").unwrap();
        assert!(files.write("c", b"1").is_err());
        files.write("a", b"12").unwrap();
        let _ = fs::remove_dir_all(&root);
    }
}
