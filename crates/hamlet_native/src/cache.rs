//! The private copy of a library: the file that is hashed against the
//! chief's answer is the file that is opened, so the original cannot
//! change between the two.

use std::path::{Path, PathBuf};

use crate::install::hash;

/// The folder of the copies in the home, one folder for each plugin.
pub const FOLDER: &str = "native-cache";

/// Copies the library `source` of the plugin `id` into the cache of
/// the home `root` and gives the copy, once its hash is `granted`.
/// A library that is a link is refused.
pub fn private_copy(
    root: &Path,
    id: &str,
    source: &Path,
    granted: &str,
) -> Result<PathBuf, String> {
    let kind = std::fs::symlink_metadata(source)
        .map_err(|e| format!("the library cannot be read: {e}"))?;
    if !kind.is_file() {
        return Err("the library is a link or not a file".into());
    }
    let folder = root.join(FOLDER).join(id);
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("the cache cannot be made: {e}"))?;
    let name =
        format!("{granted}.{}", std::env::consts::DLL_EXTENSION);
    let copy = folder.join(name);
    // A file left there by an earlier run is never trusted.
    let _ = std::fs::remove_file(&copy);
    std::fs::copy(source, &copy)
        .map_err(|e| format!("the library cannot be copied: {e}"))?;
    let copied = hash(&copy)
        .map_err(|e| format!("the copy cannot be read: {e}"))?;
    if copied != granted {
        let _ = std::fs::remove_file(&copy);
        return Err(
            "the library is not the one the chief let in".into()
        );
    }
    Ok(copy)
}

/// Deletes the copies of the plugin `id`.
pub fn clear(root: &Path, id: &str) {
    let _ = std::fs::remove_dir_all(root.join(FOLDER).join(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("hamlet_{name}_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn only_the_library_the_chief_saw_is_copied() {
        let root = scratch("cache");
        let source = root.join("libspark.so");
        std::fs::write(&source, b"v1").unwrap();
        let seen = hash(&source).unwrap();
        let copy =
            private_copy(&root, "spark", &source, &seen).unwrap();
        assert!(copy.starts_with(root.join(FOLDER).join("spark")));
        assert_eq!(std::fs::read(&copy).unwrap(), b"v1");

        std::fs::write(&source, b"v2").unwrap();
        let refused = private_copy(&root, "spark", &source, &seen);
        assert!(refused.is_err());
        assert!(!copy.exists());

        clear(&root, "spark");
        assert!(!root.join(FOLDER).join("spark").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_library_is_refused() {
        let root = scratch("cache_link");
        let real = root.join("real.so");
        std::fs::write(&real, b"v1").unwrap();
        let link = root.join("link.so");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let seen = hash(&real).unwrap();
        assert!(private_copy(&root, "spark", &link, &seen).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
