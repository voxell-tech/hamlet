//! Gift admission and its chief-facing refusals (H25).

use std::collections::HashSet;

use hamlet_protocol::client::GiveFile;
use unicode_normalization::UnicodeNormalization;

pub(super) const MAX_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const DELIVERY: &str = "The gift could not be delivered \
                                   inside the inbox or skills \
                                   folder. Nothing was handed over.";
pub(super) const STAGING: &str =
    "The gift could not be staged. Nothing was handed over.";
pub(super) const UNKNOWN: &str = "There is no file gift here with \
                                  that upload id. It may have \
                                  expired.";
pub(super) const MISMATCH: &str = "A gift part must match its path, \
                                   next offset, size and base64 \
                                   bytes.";

/// The fault in a gift's list of files, for which it is refused.
#[derive(Debug, thiserror::Error)]
pub(super) enum Unfit {
    #[error("Choose a file or folder to hand over.")]
    Empty,
    #[error("A gift takes at most 4096 files at once.")]
    TooMany,
    #[error(
        "A gift's path must be relative to the inbox, with ordinary \
         file and folder names."
    )]
    Path,
    #[error(
        "The gift is larger than the 32 MB an inbox takes at once."
    )]
    TooLarge,
    #[error(
        "The gift repeats a path, ignoring case and Unicode \
         spelling."
    )]
    Repeated,
    #[error("The gift uses one path as both a file and a folder.")]
    FileAndFolder,
}

/// The bytes of a gift of `files`, or why it is refused.
pub(super) fn manifest(files: &[GiveFile]) -> Result<u64, Unfit> {
    if files.is_empty() {
        return Err(Unfit::Empty);
    }
    if files.len() > 4096 {
        return Err(Unfit::TooMany);
    }
    let mut size = 0u64;
    let mut paths = HashSet::new();
    for file in files {
        if file.path.contains(['\\', '\0'])
            || file.path.split('/').any(|p| {
                p.is_empty() || p == "." || p == ".." || p.len() > 255
            })
            || (file.path.as_bytes().get(1) == Some(&b':')
                && file.path.as_bytes()[0].is_ascii_alphabetic()
                && file.path.as_bytes().get(2) == Some(&b'/'))
        {
            return Err(Unfit::Path);
        }
        let bytes = file.size;
        size = size
            .checked_add(bytes)
            .filter(|n| *n <= MAX_BYTES)
            .ok_or(Unfit::TooLarge)?;
        let folded = file
            .path
            .nfc()
            .collect::<String>()
            .to_lowercase()
            .to_uppercase()
            .to_lowercase()
            .nfc()
            .collect::<String>();
        if !paths.insert(folded) {
            return Err(Unfit::Repeated);
        }
    }
    for path in &paths {
        let mut parent = path.as_str();
        while let Some((next, _)) = parent.rsplit_once('/') {
            if paths.contains(next) {
                return Err(Unfit::FileAndFolder);
            }
            parent = next;
        }
    }
    Ok(size)
}
