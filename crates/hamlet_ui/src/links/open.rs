//! The files a link may lead to, and the programs of the system
//! that show one.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The endings of the files the system runs when it is asked to open
/// them, in small letters.
const RUNS: [&str; 34] = [
    "action", "app", "appimage", "bat", "cmd", "com", "command",
    "desktop", "dmg", "exe", "fileloc", "hta", "inetloc", "jar",
    "lnk", "mpkg", "msi", "pif", "pkg", "ps1", "reg", "run", "scpt",
    "scr", "sh", "terminal", "tool", "url", "vbs", "webloc",
    "workflow", "wsf", "wsh", "zsh",
];

/// The way a file is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Way {
    /// By the program the system keeps for its kind.
    Open,
    /// In its folder, unopened.
    Reveal,
}

/// `written` with a leading `~` as the chief's own folder.
fn expand(written: &str) -> PathBuf {
    match (written.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(written),
    }
}

/// The file or folder `written` names, with every link on the way to
/// it followed: itself, or the first of it under `roots`. Nothing if
/// it is not there or lies outside every one of `roots`, which have
/// their own links followed already.
pub(crate) fn locate(
    written: &str,
    roots: &[PathBuf],
) -> Option<PathBuf> {
    let written = expand(written);
    let tries = if written.is_absolute() {
        vec![written]
    } else {
        roots.iter().map(|root| root.join(&written)).collect()
    };
    tries
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .find(|path| roots.iter().any(|root| path.starts_with(root)))
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|file| {
        file.is_file() && file.permissions().mode() & 0o111 != 0
    })
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    false
}

/// The way `path` is shown: a file the system would run is only
/// revealed.
pub(crate) fn way(path: &Path) -> Way {
    let listed = path
        .extension()
        .and_then(|ending| ending.to_str())
        .is_some_and(|ending| {
            RUNS.contains(&ending.to_ascii_lowercase().as_str())
        });
    if listed || executable(path) {
        Way::Reveal
    } else {
        Way::Open
    }
}

/// The program that shows `path`, which is given the path as one
/// argument and no shell reads.
fn program(path: &Path, way: Way) -> Command {
    if cfg!(target_os = "macos") {
        let mut open = Command::new("open");
        if way == Way::Reveal {
            open.arg("-R");
        }
        open.arg(path);
        open
    } else if cfg!(target_os = "windows") {
        // The explorer does not read the long form of a path.
        let path = path.to_string_lossy();
        let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
        let mut open = Command::new("explorer");
        match way {
            Way::Open => open.arg(path),
            Way::Reveal => open.arg(format!("/select,{path}")),
        };
        open
    } else {
        let mut open = Command::new("xdg-open");
        match (way, path.parent()) {
            (Way::Reveal, Some(folder)) => open.arg(folder),
            _ => open.arg(path),
        };
        open
    }
}

/// Shows the file `path` if it is still under `roots`. False when it
/// is not, or nothing could show it.
pub(crate) fn show(path: &Path, roots: &[PathBuf]) -> bool {
    let Some(path) =
        path.to_str().and_then(|path| locate(path, roots))
    else {
        return false;
    };
    program(&path, way(&path)).spawn().is_ok()
}

/// Opens `address` in the chief's browser or mail program.
pub(crate) fn browse(address: &str) -> bool {
    let mut open = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(target_os = "windows") {
        let mut open = Command::new("rundll32");
        open.arg("url.dll,FileProtocolHandler");
        open
    } else {
        Command::new("xdg-open")
    };
    open.arg(address).spawn().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_path_is_found_only_inside_its_roots() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let all = hamlet_testing::Scratch::new("links-open");
        let (project, house) =
            (all.join("project"), all.join("house"));
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(&house).unwrap();
        std::fs::write(project.join("src/main.rs"), "").unwrap();
        std::fs::write(house.join("NOTES.md"), "").unwrap();
        std::fs::write(all.join("secret.txt"), "").unwrap();
        symlink(all.join("secret.txt"), project.join("door.txt"))
            .unwrap();
        let roots = [project.clone(), house.clone()];
        let found = |written: &str| locate(written, &roots);

        assert_eq!(
            found("src/main.rs"),
            Some(project.join("src/main.rs"))
        );
        assert_eq!(found("NOTES.md"), Some(house.join("NOTES.md")));
        assert_eq!(found("src/../src"), Some(project.join("src")));
        assert_eq!(found("src/lib.rs"), None);
        assert_eq!(found("../secret.txt"), None);
        assert_eq!(found("src/../../secret.txt"), None);
        assert_eq!(found("door.txt"), None);
        let outside = all.join("secret.txt");
        assert_eq!(found(outside.to_str().unwrap()), None);
        assert!(!show(&outside, &roots));

        let run = project.join("run");
        std::fs::write(&run, "").unwrap();
        assert_eq!(way(&run), Way::Open);
        std::fs::set_permissions(
            &run,
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(way(&run), Way::Reveal);
        assert_eq!(way(&project.join("Tool.APP")), Way::Reveal);
        assert_eq!(way(&project.join("src")), Way::Open);
    }
}
