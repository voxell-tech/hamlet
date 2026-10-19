//! The hash of the source files of the contract, the SDK and the
//! path crates under it, which the build script writes into the
//! contract. Shared with `build.rs`.

use std::io;
use std::path::{Path, PathBuf};

pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

pub fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// The hash of every file under `folders`, of its path in the folder
/// and its bytes, in the order of the paths.
pub fn hash(folders: &[&Path]) -> io::Result<u64> {
    let mut hash = FNV_OFFSET;
    for folder in folders {
        let mut files = Vec::new();
        collect(folder, String::new(), &mut files)?;
        files.sort();
        for (name, file) in files {
            let bytes = std::fs::read(&file)?;
            hash = fnv(hash, name.as_bytes());
            hash = fnv(hash, &(bytes.len() as u64).to_le_bytes());
            hash = fnv(hash, &bytes);
        }
    }
    Ok(hash)
}

/// The files under `folder`, each with its path from the folder the
/// walk began in, written with `/` on every system.
fn collect(
    folder: &Path,
    prefix: String,
    files: &mut Vec<(String, PathBuf)>,
) -> io::Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let name = format!(
            "{prefix}{}",
            entry.file_name().to_string_lossy()
        );
        if entry.file_type()?.is_dir() {
            collect(&entry.path(), format!("{name}/"), files)?;
        } else {
            files.push((name, entry.path()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_follows_paths_and_bytes_not_the_order_of_writing() {
        let root = std::env::temp_dir()
            .join(format!("hamlet_sources_{}", std::process::id()));
        let write = |folder: &str, files: &[(&str, &str)]| {
            let folder = root.join(folder);
            for (name, text) in files {
                let file = folder.join(name);
                std::fs::create_dir_all(file.parent().unwrap())
                    .unwrap();
                std::fs::write(file, text).unwrap();
            }
            hash(&[folder.as_path()]).unwrap()
        };
        let files =
            [("lib.rs", "// a\n"), ("plugin/entry.rs", "fn f() {}")];
        let first = write("first", &files);
        let backwards = write("backwards", &[files[1], files[0]]);
        assert_eq!(first, backwards);
        let comment = write(
            "comment",
            &[("lib.rs", "// b\n"), ("plugin/entry.rs", "fn f() {}")],
        );
        assert_ne!(first, comment);
        let moved = write(
            "moved",
            &[("lib.rs", "// a\n"), ("plugin/start.rs", "fn f() {}")],
        );
        assert_ne!(first, moved);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
