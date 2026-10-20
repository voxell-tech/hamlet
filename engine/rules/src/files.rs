//! Folders and files as the rules handle them: listed, copied,
//! compared and measured.

use std::path::{Path, PathBuf};

use anyhow::Result;
use sha2::{Digest, Sha256};

/// The bytes of every file under `dir`.
pub(crate) fn size_of(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => size_of(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

/// The folders directly under `dir`, in name order.
pub(crate) fn folders(dir: &Path) -> Vec<PathBuf> {
    let mut found = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect::<Vec<_>>();
    found.sort();
    found
}

/// Every file under `root`, as paths from it, in order. Links are
/// left out.
pub(crate) fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        for entry in std::fs::read_dir(root.join(&rel))
            .into_iter()
            .flatten()
            .flatten()
        {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let rel = rel.join(entry.file_name());
            if kind.is_dir() {
                todo.push(rel);
            } else if kind.is_file() {
                found.push(rel);
            }
        }
    }
    found.sort();
    found
}

/// Copies every file under `from` to the same place under `to`.
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for file in files_under(from) {
        let dst = to.join(&file);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from.join(&file), dst)?;
    }
    Ok(())
}

/// The SHA-256 of a file's contents. `None` when it cannot be read.
pub(crate) fn digest(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(
        Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// True when two folders hold the same files with the same contents.
pub(crate) fn same_tree(a: &Path, b: &Path) -> bool {
    let files = files_under(a);
    files == files_under(b)
        && files
            .iter()
            .all(|f| digest(&a.join(f)) == digest(&b.join(f)))
}
