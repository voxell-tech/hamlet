//! The install folder of native plugins: `plugins/<id>/` with a
//! `manifest.toml` of `kind = "native"` and the library it names.

use std::path::{Path, PathBuf};

use hamlet_sdk::plugin::is_native;
use sha2::{Digest, Sha256};

use crate::manifest::Manifest;

pub const MANIFEST: &str = "manifest.toml";

/// A native plugin whose folder passed the scan. Nothing of the
/// library has been opened.
#[derive(Debug)]
pub struct Found {
    pub manifest: Manifest,
    pub folder: PathBuf,
    pub library: PathBuf,
    /// The hash of the library's file, in hex.
    pub hash: String,
}

/// A native plugin's folder that was turned down.
#[derive(Debug, PartialEq)]
pub struct Rejected {
    pub folder: String,
    pub why: String,
}

/// Every folder of the install folder `plugins` that holds a native
/// plugin, read. A missing install folder holds none.
pub fn scan(plugins: &Path) -> Vec<Result<Found, Rejected>> {
    let Ok(entries) = std::fs::read_dir(plugins) else {
        return Vec::new();
    };
    let mut folders = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    folders.sort();
    folders
        .iter()
        .filter_map(|folder| {
            let text = std::fs::read_to_string(folder.join(MANIFEST))
                .ok()?;
            is_native(&text).then(|| read(folder, &text))
        })
        .collect()
}

fn read(folder: &Path, text: &str) -> Result<Found, Rejected> {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let rejected = |why: String| Rejected {
        folder: name.clone(),
        why,
    };
    let manifest = Manifest::parse(text).map_err(|e| {
        rejected(format!(
            "{MANIFEST} cannot be read: {}",
            e.message()
        ))
    })?;
    if let Some(why) = manifest.problem() {
        return Err(rejected(why));
    }
    if manifest.id != name {
        return Err(rejected(format!(
            "the manifest's id `{}` is not the folder's",
            manifest.id
        )));
    }
    let library = folder.join(&manifest.library);
    let hash = hash(&library).map_err(|e| {
        rejected(format!("the library cannot be read: {e}"))
    })?;
    Ok(Found {
        manifest,
        folder: folder.to_path_buf(),
        library,
        hash,
    })
}

/// The hash of a file, in hex.
pub fn hash(file: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(file)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_folders_are_found_and_the_others_left() {
        let root = std::env::temp_dir()
            .join(format!("hamlet_install_{}", std::process::id()));
        let library =
            format!("libspark.{}", std::env::consts::DLL_EXTENSION);
        let put = |id: &str, manifest: String| {
            let folder = root.join(id);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(MANIFEST), manifest).unwrap();
            std::fs::write(folder.join(&library), b"v1").unwrap();
        };
        let native = |id: &str| {
            format!(
                "id = \"{id}\"\nname = \"Spark\"\nversion = \
                 \"0.1.0\"\nkind = \"native\"\nlibrary = \
                 \"{library}\"\n"
            )
        };
        put("spark", native("spark"));
        put("moved", native("spark"));
        put("other", "id = \"other\"\nkind = \"wasm\"\n".into());

        let found = scan(&root);
        assert_eq!(found.len(), 2);
        let moved = found[0].as_ref().unwrap_err();
        assert!(moved.why.contains("not the folder's"), "{moved:?}");
        let spark = found[1].as_ref().unwrap();
        let before = spark.hash.clone();

        std::fs::write(&spark.library, b"v2").unwrap();
        let again = scan(&root)
            .into_iter()
            .filter_map(Result::ok)
            .next()
            .unwrap();
        assert_ne!(again.hash, before);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
