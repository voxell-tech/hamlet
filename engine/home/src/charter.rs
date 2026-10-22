//! The charter: the words every agent's prompt opens with. The one
//! built in stands unless the home keeps its own in `charter.md`.

use std::path::{Path, PathBuf};

/// The charter built into the app.
pub const DEFAULT: &str = include_str!("../../../assets/charter.md");

/// The file of the charter the home at `root` keeps of its own.
pub fn file(root: &Path) -> PathBuf {
    root.join("charter.md")
}

/// The charter the home at `root` keeps of its own. `None` for a
/// missing or an empty one.
pub fn own(root: &Path) -> Option<String> {
    std::fs::read_to_string(file(root))
        .ok()
        .filter(|text| !text.trim().is_empty())
}

/// The charter of the home at `root`: its own, or the one built in.
pub fn read(root: &Path) -> String {
    own(root).unwrap_or_else(|| DEFAULT.to_string())
}

/// Gives the home at `root` the built-in charter as a file of its
/// own to change, unless it keeps one. Returns the file.
pub fn adopt(root: &Path) -> std::io::Result<PathBuf> {
    let file = file(root);
    if own(root).is_none() {
        std::fs::write(&file, DEFAULT)?;
    }
    Ok(file)
}

/// Removes the charter the home at `root` keeps of its own.
pub fn reset(root: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(file(root)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_home_keeps_its_own_charter_unless_it_is_empty() {
        let home = Scratch::new("charter");
        assert_eq!(read(&home), DEFAULT);
        std::fs::write(file(&home), "  \n").unwrap();
        assert_eq!(read(&home), DEFAULT);
        std::fs::write(file(&home), "Be kind.\n").unwrap();
        assert_eq!(read(&home), "Be kind.\n");
        reset(&home).unwrap();
        assert_eq!(read(&home), DEFAULT);
        assert_eq!(adopt(&home).unwrap(), file(&home));
        assert_eq!(own(&home).as_deref(), Some(DEFAULT));
    }
}
