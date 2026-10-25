//! Links that tests plant, made the same way on every OS.

use std::io;
use std::path::Path;

/// A symbolic link at `link` to `target`. On Windows `target` is a
/// folder or a file as it is now, read from beside `link`.
pub fn symlink(
    target: impl AsRef<Path>,
    link: impl AsRef<Path>,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        let (target, link) = (target.as_ref(), link.as_ref());
        let beside = link.parent().unwrap_or(Path::new("."));
        if beside.join(target).is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        }
    }
}

/// A second name `link` for the file `original`.
pub fn hard_link(
    original: impl AsRef<Path>,
    link: impl AsRef<Path>,
) -> io::Result<()> {
    std::fs::hard_link(original, link)
}

/// A junction at `link` to the folder `target`, which Windows makes
/// without privilege. `mklink` would store a verbatim `\\?\` target
/// as it is, which no walk reads as a folder.
#[cfg(windows)]
pub fn junction(
    target: impl AsRef<Path>,
    link: impl AsRef<Path>,
) -> io::Result<()> {
    let target = target.as_ref().as_os_str().to_string_lossy();
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link.as_ref())
        .arg(target.strip_prefix(r"\\?\").unwrap_or(&target))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "mklink /J ended with {status}"
        )))
    }
}
