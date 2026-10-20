//! Writing a file whole or not at all.

use std::fs;
use std::io::Write;
use std::path::Path;

/// Writes `bytes` to `saving_<name>` beside `path` and moves it over
/// `path`, so a stop in the middle leaves the old file or the new
/// one.
pub fn write_whole(
    path: &Path,
    bytes: impl AsRef<[u8]>,
) -> std::io::Result<()> {
    let name = path.file_name().map_or_else(String::new, |name| {
        name.to_string_lossy().into_owned()
    });
    let saving = path.with_file_name(format!("saving_{name}"));
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut file = fs::File::create(&saving)?;
    file.write_all(bytes.as_ref())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&saving, path)
}
