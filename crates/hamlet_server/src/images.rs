//! The images the realm keeps, each once under the hash of its bytes,
//! and given to a model made to fit it (S29, S30).

pub(crate) use hamlet_workspace_wire::image::{
    Bounds, Format, Unusable, check, fit,
};

/// The most bytes of an image the realm keeps.
pub(crate) const MOST_BYTES: usize = 20 << 20;
/// The longest side of a thumbnail, and the most bytes it takes.
pub(crate) const THUMBNAIL_SIDE: u32 = 320;
pub(crate) const THUMBNAIL_BYTES: u64 = 256 * 1024;

/// `words` with its first letter in capitals.
pub(crate) fn capitalised(words: &str) -> String {
    let mut letters = words.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}
